//! The session work the command task drives.
//!
//! The task reaches the engine through one trait, so its exit paths can be
//! exercised without a network or an audio device. The production adapter is
//! thin: a call-through or one `Option` check.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use librespot::core::SessionConfig;
use takofy_core::connect::{ClusterEvent, ClusterView, ConnectError, Steer};
use takofy_core::credentials::AccessToken;
use takofy_core::engine::{Engine, EngineError, Orders, RootlistRead};
use takofy_core::playback::{Playback, PlaybackError, PlaybackEvent, Volume};
use takofy_core::receive::Inbound;
use takofy_core::rootlist::Walked;
use takofy_core::{Resolution, TransferPlan};
use takofy_model::home::{Station, StationSeed};
use takofy_model::{
    Account, AlbumDetail, ArtistDetail, ArtistPortrait, CoverSet, FollowedArtist, FollowedShow,
    Landing, LikedTrack, Move, PlaylistAccess, PlaylistEntry, Removal, Report, RequestId, Revision,
    SavedAlbum, ShowDetail, TrackEntry, TrackLyrics,
};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::oneshot::Receiver;

/// How long a rebuild waits for the dead session's player thread before going
/// ahead without it.
const RETIRE_BUDGET: Duration = Duration::from_secs(3);

/// Why a session-side operation failed, including the wiring's own state.
#[derive(Debug)]
pub(crate) enum SessionError {
    /// No engine exists yet, so the call had nothing to run on.
    NotConnected,
    /// The engine exists but its session is known to be dead, so the call would
    /// fail on the wire and read as a fault of the track.
    LinkLost,
    /// The engine call itself failed.
    Engine(EngineError),
    /// The player could not be started, driven or shut down.
    Playback(PlaybackError),
}

/// How retiring a dead session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Retired {
    /// The old player's thread was joined.
    Joined,
    /// The join outlasted the budget, and the thread finishes on its own.
    Lingering,
}

impl SessionError {
    /// Whether this failure means the device watch can never start again.
    pub(crate) fn devices_terminal(&self) -> bool {
        matches!(
            self,
            Self::Engine(EngineError::Connect(ConnectError::DealerSpent))
        )
    }

    /// Whether this failure means the move had no page to carry.
    pub(crate) fn no_context(&self) -> bool {
        matches!(
            self,
            Self::Engine(EngineError::Connect(ConnectError::NoContext))
        )
    }

    /// A fixed name for the log. The source error is never formatted: it can
    /// embed a response body.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::NotConnected => "not connected",
            Self::LinkLost => "link lost",
            Self::Engine(EngineError::Session(_)) => "session",
            Self::Engine(EngineError::Rootlist(_)) => "rootlist",
            Self::Engine(EngineError::Context(_)) => "context",
            Self::Engine(EngineError::Album(_)) => "album",
            Self::Engine(EngineError::Artist(_)) => "artist",
            Self::Engine(EngineError::Show(_)) => "show",
            Self::Engine(EngineError::Station(_)) => "station",
            Self::Engine(EngineError::Lyrics(_)) => "lyrics",
            Self::Engine(EngineError::Playlist(_)) => "playlist",
            Self::Engine(EngineError::Followed(_)) => "followed",
            Self::Engine(EngineError::Connect(_)) => "connect",
            Self::Playback(error) => match error {
                PlaybackError::NoAudioBackend => "no audio backend",
                PlaybackError::NoAudioDevice => "no audio device",
                PlaybackError::PlayerShared => "player shared",
                PlaybackError::PlayerClosed => "player closed",
                PlaybackError::NotPlayable(_) => "not playable",
            },
        }
    }
}

/// One page of a playlist's tracks, plus the context's own image.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TrackPage {
    pub tracks: Vec<TrackEntry>,
    pub cover: Option<CoverSet>,
    pub access: Option<PlaylistAccess>,
}

/// One artist's page data, plus whether the user follows it.
///
/// The followed state is `None` when the collection probe did not answer: the
/// label then reads `Follow` and stays pressable, because the write is the only
/// thing that can confirm it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArtistPage {
    pub artist: ArtistDetail,
    pub followed: Option<bool>,
}

/// The session work the command task drives.
///
/// Returns `impl Future + Send` rather than being an `async fn` because an
/// `async fn` in a trait does not declare its future `Send`, and the task has
/// to be spawnable. `&mut self` is only where the lazily started player is
/// written; `shutdown` consumes, because that is what joins the thread. `Sync`
/// is what lets several reads hold `&self` at once.
pub(crate) trait SessionHost: Send + Sync + 'static {
    /// Opens the session with `token`.
    ///
    /// A caller that replaces a live-but-dead engine retires it first: this
    /// assigns the new engine over the old one, which would otherwise be left
    /// running with its own player and sockets.
    fn connect(
        &mut self,
        token: &AccessToken,
    ) -> impl Future<Output = Result<(), SessionError>> + Send;

    /// The account this session connected as, once it is connected.
    ///
    /// A host that never learns a name saves no library copy; the default
    /// keeps such a host honest without a fake implementation.
    fn account(&self) -> Option<Account> {
        None
    }

    /// The rootlist as the account walks it, before its playlists are named.
    ///
    /// Needs only a connected session, so a read that draws no playlist name
    /// can start before the labels are read.
    fn walk_rootlist(&self) -> impl Future<Output = Result<Walked, SessionError>> + Send;

    /// The walked playlists named, with what the sidebar may write with.
    ///
    /// Cannot fail on the engine's side: a playlist that cannot be named keeps
    /// its URI.
    fn label_rootlist(
        &self,
        walked: Walked,
    ) -> impl Future<Output = Result<RootlistRead, SessionError>> + Send;

    /// One playlist as the sidebar draws it, or `None` when the read does not
    /// answer.
    fn playlist_entry(&self, playlist: &str) -> impl Future<Output = Option<PlaylistEntry>> + Send;

    /// Writes one move to the sidebar's playlists, answering the revision to
    /// hold next.
    fn move_playlist(
        &self,
        revision: &Revision,
        from: usize,
        to: usize,
    ) -> impl Future<Output = Result<Revision, SessionError>> + Send;

    /// Unlinks one row of the sidebar's playlists, answering the revision to
    /// hold next.
    fn remove_playlist(
        &self,
        revision: &Revision,
        index: usize,
    ) -> impl Future<Output = Result<Revision, SessionError>> + Send;

    /// Whether one album or show is in the library.
    fn saved_state(&self, uri: &str) -> impl Future<Output = Option<bool>> + Send;

    /// Writes one album or show into the library, or out of it.
    fn set_saved(
        &self,
        uri: &str,
        saved: bool,
    ) -> impl Future<Output = Result<(), SessionError>> + Send;

    /// Appends tracks to a playlist as one write, answering the revision to
    /// hold next.
    fn add_tracks(
        &self,
        playlist: &str,
        revision: &Revision,
        from_index: usize,
        tracks: &[String],
    ) -> impl Future<Output = Result<Revision, SessionError>> + Send;

    /// Creates a playlist called `name`, answering its URI.
    fn make_playlist(
        &self,
        name: &str,
    ) -> impl Future<Output = Result<String, SessionError>> + Send;

    /// Renames `playlist`, answering the revision to hold next.
    fn rename_playlist(
        &self,
        playlist: &str,
        revision: &Revision,
        name: &str,
    ) -> impl Future<Output = Result<Revision, SessionError>> + Send;

    /// Puts `playlist` at the front of the user's rootlist, answering the
    /// revision to hold next.
    fn link_playlist(
        &self,
        revision: &Revision,
        playlist: &str,
    ) -> impl Future<Output = Result<Revision, SessionError>> + Send;

    /// What a playlist read says about writing to it; `None` when it cannot be
    /// read.
    fn playlist_state(&self, playlist: &str)
    -> impl Future<Output = Option<PlaylistAccess>> + Send;

    fn tracks(
        &self,
        playlist: &str,
    ) -> impl Future<Output = Result<TrackPage, SessionError>> + Send;

    /// Removes rows from a playlist, answering the revision to hold next.
    fn remove_tracks(
        &self,
        playlist: &str,
        revision: &Revision,
        removals: &[Removal],
    ) -> impl Future<Output = Result<Revision, SessionError>> + Send;

    /// Writes one move to a playlist, answering the revision to hold next.
    fn move_track(
        &self,
        playlist: &str,
        revision: &Revision,
        moves: &[Move],
    ) -> impl Future<Output = Result<Revision, SessionError>> + Send;

    fn album(&self, album: &str) -> impl Future<Output = Result<AlbumDetail, SessionError>> + Send;

    fn show(&self, show: &str) -> impl Future<Output = Result<ShowDetail, SessionError>> + Send;

    /// The newest episodes of every show in `shows`, labelled in one read.
    ///
    /// A show that does not answer is missing from the answer rather than
    /// answered empty.
    fn episodes(
        &self,
        shows: &[String],
    ) -> impl Future<Output = Result<Vec<ShowDetail>, SessionError>> + Send;

    /// One station read from its seed, its rows labelled.
    fn station(
        &self,
        seed: &StationSeed,
    ) -> impl Future<Output = Result<Station, SessionError>> + Send;

    /// A radio read from one seed, its rows labelled in the station's order.
    fn radio(
        &self,
        seed: &StationSeed,
    ) -> impl Future<Output = Result<Vec<TrackEntry>, SessionError>> + Send;

    /// The artists a station names beside `artist`: this client's one read of an
    /// artist's neighbours.
    fn neighbours(
        &self,
        artist: &str,
    ) -> impl Future<Output = Result<Vec<FollowedArtist>, SessionError>> + Send;

    /// The shows the user follows, newest follow first, each as its card draws
    /// it.
    fn followed_shows(
        &self,
    ) -> impl Future<Output = Result<Vec<FollowedShow>, SessionError>> + Send;

    /// The artists the user follows, newest follow first, each as its card
    /// draws it.
    fn followed_artists(
        &self,
    ) -> impl Future<Output = Result<Vec<FollowedArtist>, SessionError>> + Send;

    /// The albums the user saved, newest save first, each as its card draws it.
    fn saved_albums(&self) -> impl Future<Output = Result<Vec<SavedAlbum>, SessionError>> + Send;

    /// The tracks the user has liked, newest save first.
    fn liked_tracks(&self) -> impl Future<Output = Result<Vec<LikedTrack>, SessionError>> + Send;

    /// Writes these tracks into the liked-songs set, or out of it.
    fn set_liked(
        &self,
        tracks: Vec<String>,
        liked: bool,
    ) -> impl Future<Output = Result<(), SessionError>> + Send;

    fn artist(&self, artist: &str)
    -> impl Future<Output = Result<ArtistPage, SessionError>> + Send;

    /// One track's lyrics, empty when it has none.
    fn lyrics(&self, track: &str)
    -> impl Future<Output = Result<TrackLyrics, SessionError>> + Send;

    /// The tracks one search query answers, labelled.
    fn search(
        &self,
        query: &str,
    ) -> impl Future<Output = Result<Vec<TrackEntry>, SessionError>> + Send;

    /// The artists' portraits, in the order asked for, from one batched read.
    ///
    /// `None` when the read failed: it is speculative, so it has no failure to
    /// report to the window.
    fn portraits(
        &self,
        artists: Vec<String>,
    ) -> impl Future<Output = Option<Vec<ArtistPortrait>>> + Send;

    /// The artists an artist is heard with, named.
    fn related(
        &self,
        uris: Vec<String>,
    ) -> impl Future<Output = Result<Vec<FollowedArtist>, SessionError>> + Send;

    /// One release's track names, in the release's own order.
    fn release_tracks(
        &self,
        tracks: &[String],
    ) -> impl Future<Output = Result<Vec<TrackEntry>, SessionError>> + Send;

    /// Writes the artist into the user's followed set, or out of it.
    fn set_follow(
        &self,
        artist: &str,
        follow: bool,
    ) -> impl Future<Output = Result<(), SessionError>> + Send;

    /// Starts watching the account's devices, ignoring a watch already running.
    fn watch_devices(&mut self) -> impl Future<Output = Result<(), SessionError>> + Send;

    /// The event stream of a watch started since the last call, so the one
    /// drain thread is handed it exactly once.
    fn take_devices(&mut self) -> Option<UnboundedReceiver<ClusterEvent>>;

    /// The request stream of a watch started since the last call, so the one
    /// command loop is handed it exactly once.
    fn take_inbound(&mut self) -> Option<UnboundedReceiver<Inbound>>;

    /// Says whether this device can play. A call with no session is a no-op, so
    /// a play before sign-in reports nothing here.
    fn set_playable(&self, playable: bool);

    /// Publishes what the window is playing. A call with no session is a no-op,
    /// like the stage above it.
    fn report(&self, report: Option<Report>);

    /// Answers one incoming request `Failure`. A call with no session is a
    /// no-op, like the stage above it.
    fn refuse(&self, id: RequestId);

    /// Asks the cluster for its roster again, answering what it says.
    fn refresh_devices(&self) -> impl Future<Output = Result<ClusterView, SessionError>> + Send;

    /// Sends one transport action to the device that holds the sound.
    fn steer(
        &self,
        device: &str,
        steer: Steer,
    ) -> impl Future<Output = Result<(), SessionError>> + Send;

    /// Moves the account's sound to one device.
    fn hand_over(&self, device: &str) -> impl Future<Output = Result<(), SessionError>> + Send;

    /// Takes this client off the account's device list. Best effort: the exit
    /// draws nothing, so a failure is logged and dropped.
    fn withdraw_devices(&self) -> impl Future<Output = Result<(), SessionError>> + Send;

    fn play(&mut self, uri: &str) -> Result<(), SessionError>;

    /// Starts the player if it is not running, keeping its event stream for the
    /// drain.
    fn prepare_player(&mut self) -> Result<(), SessionError>;

    /// Every track URI a context holds, in the walker's order.
    fn resolve_context(
        &self,
        uri: &str,
    ) -> impl Future<Output = Result<Vec<String>, SessionError>> + Send;

    /// Takes the account's sound for one hand-over, answering the phone.
    fn accept(
        &self,
        id: RequestId,
        plan: &TransferPlan,
        resolution: Resolution,
    ) -> impl Future<Output = Result<Landing, SessionError>> + Send;

    /// Loads one track at `position_ms`, playing only when asked to.
    fn load(&mut self, uri: &str, position_ms: u32, play: bool) -> Result<(), SessionError>;

    /// Gives the account's sound back, announcing what the device is now.
    fn release_sound(&self) -> impl Future<Output = Result<(), SessionError>> + Send;

    /// Whether this device holds the account's sound.
    fn holding(&self) -> bool;

    /// Answers one incoming request with this window's verdict: taken, or
    /// refused.
    ///
    /// `true` says the request was received and carried out as far as this
    /// window could take it, which for a queue edit means the rows were adopted,
    /// not that the phone's own action finished. A pending request's ack rides
    /// the next state put either way.
    fn answer_order(&self, id: RequestId, ok: bool) -> bool;

    /// A handle that answers an order without borrowing this host, so a rebuild
    /// that owns the host can still answer a phone.
    fn orders(&self) -> Option<Orders>;

    /// When a pending request's budget answers it.
    fn expires(&self, id: RequestId) -> Option<Instant>;

    /// Jumps within the loaded track. A call with no player is a no-op.
    fn seek(&self, position_ms: u32);

    /// Sets the level the player reads. 0 is silence, 100 is unity gain.
    fn set_volume(&self, percent: u16);

    /// The event stream of a player started since the last call, so the one
    /// drain thread can be handed it exactly once.
    fn take_events(&mut self) -> Option<UnboundedReceiver<PlaybackEvent>>;

    fn pause(&self);

    fn resume(&self);

    fn stop(&self);

    /// Whether the engine's session is known to be dead. A window that never
    /// connected has no link to lose.
    fn is_down(&self) -> bool;

    fn shutdown(self) -> impl Future<Output = Result<(), SessionError>> + Send;

    /// Ends the dead session: its engine, its streams and its player.
    ///
    /// The player is joined under a budget, because a thread mid-decode can
    /// outlast it; the answer says which happened.
    fn retire(&mut self) -> impl Future<Output = Retired> + Send;
}

/// Runs one host call, or gives up on it if the window closes first.
pub(crate) async fn stop_or<T>(
    stop: &mut Receiver<()>,
    work: impl Future<Output = T>,
) -> Option<T> {
    tokio::select! {
        biased;
        _ = stop => None,
        value = work => Some(value),
    }
}

/// The production adapter. Holds the config so `connect` can run once the
/// token arrives, and the player so it can be started lazily.
pub(crate) struct LiveSession {
    cfg: SessionConfig,
    engine: Option<Engine>,
    playback: Option<Playback>,
    events: Option<UnboundedReceiver<PlaybackEvent>>,
    /// The watch's stream, kept until the drain thread takes it.
    devices: Option<UnboundedReceiver<ClusterEvent>>,
    /// The watch's request stream, kept until the command loop takes it.
    inbound: Option<UnboundedReceiver<Inbound>>,
    /// The level the player will read, kept here so a change before the player
    /// exists is still the level it starts with.
    volume: Volume,
    /// Whether this device announced it can play, kept so a rebuilt engine can
    /// be told again.
    playable: AtomicBool,
    /// The newest report published, kept for the same reason.
    report: Mutex<Option<Report>>,
}

impl LiveSession {
    pub(crate) fn new(cfg: SessionConfig) -> Self {
        Self {
            cfg,
            engine: None,
            playback: None,
            events: None,
            devices: None,
            inbound: None,
            volume: Volume::default(),
            playable: AtomicBool::new(false),
            report: Mutex::new(None),
        }
    }

    fn held_report(&self) -> Option<Report> {
        self.report
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Starts the player on first use, keeping its event stream so the one drain
    /// thread can be handed it once.
    fn ensure_player(&mut self) -> Result<&Playback, SessionError> {
        if self.playback.is_none() {
            let engine = self.engine.as_ref().ok_or(SessionError::NotConnected)?;
            let (playback, events) = engine
                .playback(self.volume.clone())
                .map_err(SessionError::Playback)?;
            self.playback = Some(playback);
            self.events = Some(events);
        }
        self.playback.as_ref().ok_or(SessionError::NotConnected)
    }
}

impl SessionHost for LiveSession {
    async fn connect(&mut self, token: &AccessToken) -> Result<(), SessionError> {
        let engine = Engine::connect(self.cfg.clone(), token)
            .await
            .map_err(SessionError::Engine)?;
        // A fresh engine starts with an empty stage and no report, so what the
        // window already said is told again before anything reads it.
        engine.restore(self.playable.load(Ordering::SeqCst), self.held_report());
        self.engine = Some(engine);
        Ok(())
    }

    async fn walk_rootlist(&self) -> Result<Walked, SessionError> {
        match &self.engine {
            Some(engine) => engine.walk_rootlist().await.map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    fn account(&self) -> Option<Account> {
        self.engine
            .as_ref()
            .and_then(|engine| Account::from_wire(&engine.username()))
    }

    async fn label_rootlist(&self, walked: Walked) -> Result<RootlistRead, SessionError> {
        match &self.engine {
            Some(engine) => Ok(engine.label_rootlist(walked).await),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn portraits(&self, artists: Vec<String>) -> Option<Vec<ArtistPortrait>> {
        match &self.engine {
            Some(engine) => engine.portraits(&artists).await,
            None => None,
        }
    }

    async fn playlist_entry(&self, playlist: &str) -> Option<PlaylistEntry> {
        match &self.engine {
            Some(engine) => engine.playlist_entry(playlist).await,
            None => None,
        }
    }

    async fn move_playlist(
        &self,
        revision: &Revision,
        from: usize,
        to: usize,
    ) -> Result<Revision, SessionError> {
        match &self.engine {
            Some(engine) => engine
                .move_playlist(revision, from, to)
                .await
                .map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn remove_playlist(
        &self,
        revision: &Revision,
        index: usize,
    ) -> Result<Revision, SessionError> {
        match &self.engine {
            Some(engine) => engine
                .remove_playlist(revision, index)
                .await
                .map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn saved_state(&self, uri: &str) -> Option<bool> {
        match &self.engine {
            Some(engine) => engine.saved_state(uri).await,
            None => None,
        }
    }

    async fn set_saved(&self, uri: &str, saved: bool) -> Result<(), SessionError> {
        match &self.engine {
            Some(engine) => engine
                .set_saved(uri, saved)
                .await
                .map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn add_tracks(
        &self,
        playlist: &str,
        revision: &Revision,
        from_index: usize,
        tracks: &[String],
    ) -> Result<Revision, SessionError> {
        match &self.engine {
            Some(engine) => engine
                .add_tracks(playlist, revision, from_index, tracks)
                .await
                .map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn make_playlist(&self, name: &str) -> Result<String, SessionError> {
        match &self.engine {
            Some(engine) => engine
                .make_playlist(name)
                .await
                .map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn rename_playlist(
        &self,
        playlist: &str,
        revision: &Revision,
        name: &str,
    ) -> Result<Revision, SessionError> {
        match &self.engine {
            Some(engine) => engine
                .rename_playlist(playlist, revision, name)
                .await
                .map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn link_playlist(
        &self,
        revision: &Revision,
        playlist: &str,
    ) -> Result<Revision, SessionError> {
        match &self.engine {
            Some(engine) => engine
                .link_playlist(revision, playlist)
                .await
                .map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn playlist_state(&self, playlist: &str) -> Option<PlaylistAccess> {
        match &self.engine {
            Some(engine) => engine.playlist_state(playlist).await,
            None => None,
        }
    }

    async fn tracks(&self, playlist: &str) -> Result<TrackPage, SessionError> {
        let Some(engine) = &self.engine else {
            return Err(SessionError::NotConnected);
        };
        let page = engine
            .playlist_page(playlist)
            .await
            .map_err(SessionError::Engine)?;
        Ok(TrackPage {
            tracks: page.tracks,
            cover: page.cover,
            access: page.access,
        })
    }

    async fn move_track(
        &self,
        playlist: &str,
        revision: &Revision,
        moves: &[Move],
    ) -> Result<Revision, SessionError> {
        match &self.engine {
            Some(engine) => engine
                .move_track(playlist, revision, moves)
                .await
                .map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn remove_tracks(
        &self,
        playlist: &str,
        revision: &Revision,
        removals: &[Removal],
    ) -> Result<Revision, SessionError> {
        match &self.engine {
            Some(engine) => engine
                .remove_tracks(playlist, revision, removals)
                .await
                .map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn album(&self, album: &str) -> Result<AlbumDetail, SessionError> {
        match &self.engine {
            Some(engine) => engine.album(album).await.map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn show(&self, show: &str) -> Result<ShowDetail, SessionError> {
        match &self.engine {
            Some(engine) => engine.show(show).await.map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn episodes(&self, shows: &[String]) -> Result<Vec<ShowDetail>, SessionError> {
        match &self.engine {
            Some(engine) => engine.episodes(shows).await.map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn station(&self, seed: &StationSeed) -> Result<Station, SessionError> {
        match &self.engine {
            Some(engine) => engine.station(seed).await.map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn radio(&self, seed: &StationSeed) -> Result<Vec<TrackEntry>, SessionError> {
        match &self.engine {
            Some(engine) => engine.radio(seed).await.map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn neighbours(&self, artist: &str) -> Result<Vec<FollowedArtist>, SessionError> {
        match &self.engine {
            Some(engine) => engine
                .neighbours(artist)
                .await
                .map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn followed_shows(&self) -> Result<Vec<FollowedShow>, SessionError> {
        match &self.engine {
            Some(engine) => engine.followed_shows().await.map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn followed_artists(&self) -> Result<Vec<FollowedArtist>, SessionError> {
        match &self.engine {
            Some(engine) => engine
                .followed_artists()
                .await
                .map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn saved_albums(&self) -> Result<Vec<SavedAlbum>, SessionError> {
        match &self.engine {
            Some(engine) => engine.saved_albums().await.map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn liked_tracks(&self) -> Result<Vec<LikedTrack>, SessionError> {
        match &self.engine {
            Some(engine) => engine.liked_tracks().await.map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn set_liked(&self, tracks: Vec<String>, liked: bool) -> Result<(), SessionError> {
        match &self.engine {
            Some(engine) => engine
                .set_liked(tracks, liked)
                .await
                .map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn artist(&self, artist: &str) -> Result<ArtistPage, SessionError> {
        match &self.engine {
            Some(engine) => {
                let (artist, followed) =
                    engine.artist(artist).await.map_err(SessionError::Engine)?;
                Ok(ArtistPage { artist, followed })
            }
            None => Err(SessionError::NotConnected),
        }
    }

    async fn lyrics(&self, track: &str) -> Result<TrackLyrics, SessionError> {
        match &self.engine {
            Some(engine) => engine.lyrics(track).await.map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn search(&self, query: &str) -> Result<Vec<TrackEntry>, SessionError> {
        match &self.engine {
            Some(engine) => engine.search(query).await.map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn related(&self, uris: Vec<String>) -> Result<Vec<FollowedArtist>, SessionError> {
        match &self.engine {
            Some(engine) => engine.related(uris).await.map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn release_tracks(&self, tracks: &[String]) -> Result<Vec<TrackEntry>, SessionError> {
        match &self.engine {
            Some(engine) => Ok(engine.release_tracks(tracks).await),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn set_follow(&self, artist: &str, follow: bool) -> Result<(), SessionError> {
        match &self.engine {
            Some(engine) => engine
                .set_follow(artist, follow)
                .await
                .map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn watch_devices(&mut self) -> Result<(), SessionError> {
        // A second request hands back the streams the first one already holds.
        // Once the drains have taken them, the engine's refusal stands.
        if self.devices.is_some() {
            return Ok(());
        }
        let Some(engine) = &mut self.engine else {
            return Err(SessionError::NotConnected);
        };
        let watching = engine.watch_devices().await.map_err(SessionError::Engine)?;
        self.devices = Some(watching.events);
        self.inbound = Some(watching.inbound);
        Ok(())
    }

    fn take_devices(&mut self) -> Option<UnboundedReceiver<ClusterEvent>> {
        self.devices.take()
    }

    fn take_inbound(&mut self) -> Option<UnboundedReceiver<Inbound>> {
        self.inbound.take()
    }

    fn set_playable(&self, playable: bool) {
        self.playable.store(playable, Ordering::SeqCst);
        if let Some(engine) = &self.engine {
            engine.set_playable(playable);
        }
    }

    fn report(&self, report: Option<Report>) {
        if let Some(engine) = &self.engine {
            engine.report(report.clone());
        }
        *self.report.lock().unwrap_or_else(PoisonError::into_inner) = report;
    }

    fn refuse(&self, id: RequestId) {
        if let Some(engine) = &self.engine {
            engine.refuse(id);
        }
    }

    async fn refresh_devices(&self) -> Result<ClusterView, SessionError> {
        match &self.engine {
            Some(engine) => engine.refresh_devices().await.map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn steer(&self, device: &str, steer: Steer) -> Result<(), SessionError> {
        match &self.engine {
            Some(engine) => engine
                .steer_device(device, &steer)
                .await
                .map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn withdraw_devices(&self) -> Result<(), SessionError> {
        let Some(engine) = &self.engine else {
            return Err(SessionError::NotConnected);
        };
        if let Err(error) = engine.withdraw_devices().await {
            // The window is closing; there is nothing left to draw this into.
            log::warn!("device withdraw: {}", SessionError::Engine(error).kind());
        }
        Ok(())
    }

    async fn hand_over(&self, device: &str) -> Result<(), SessionError> {
        match &self.engine {
            Some(engine) => engine.hand_over(device).await.map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    fn play(&mut self, uri: &str) -> Result<(), SessionError> {
        self.load(uri, 0, true)
    }

    fn prepare_player(&mut self) -> Result<(), SessionError> {
        self.ensure_player().map(|_| ())
    }

    async fn resolve_context(&self, uri: &str) -> Result<Vec<String>, SessionError> {
        match &self.engine {
            Some(engine) => engine
                .resolve_context(uri)
                .await
                .map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn accept(
        &self,
        id: RequestId,
        plan: &TransferPlan,
        resolution: Resolution,
    ) -> Result<Landing, SessionError> {
        match &self.engine {
            Some(engine) => engine
                .accept_transfer(id, plan, resolution)
                .await
                .map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    fn load(&mut self, uri: &str, position_ms: u32, play: bool) -> Result<(), SessionError> {
        // A dead session would fail this load on its key request and read as a
        // track that cannot be played, so the player never sees it.
        if self.is_down() {
            return Err(SessionError::LinkLost);
        }
        self.ensure_player()?
            .load(uri, position_ms, play)
            .map_err(SessionError::Playback)
    }

    async fn release_sound(&self) -> Result<(), SessionError> {
        match &self.engine {
            Some(engine) => engine.release_sound().await.map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    fn holding(&self) -> bool {
        match &self.engine {
            Some(engine) => engine.holding(),
            None => false,
        }
    }

    fn answer_order(&self, id: RequestId, ok: bool) -> bool {
        match &self.engine {
            Some(engine) => engine.answer_order(id, ok),
            None => false,
        }
    }

    fn orders(&self) -> Option<Orders> {
        self.engine.as_ref().map(Engine::orders)
    }

    fn expires(&self, id: RequestId) -> Option<Instant> {
        match &self.engine {
            Some(engine) => engine.expires(id),
            None => None,
        }
    }

    fn take_events(&mut self) -> Option<UnboundedReceiver<PlaybackEvent>> {
        self.events.take()
    }

    fn pause(&self) {
        if let Some(playback) = &self.playback {
            playback.pause();
        }
    }

    fn resume(&self) {
        if let Some(playback) = &self.playback {
            playback.resume();
        }
    }

    fn seek(&self, position_ms: u32) {
        if let Some(playback) = &self.playback {
            playback.seek(position_ms);
        }
    }

    fn set_volume(&self, percent: u16) {
        self.volume.set(percent);
    }

    fn stop(&self) {
        if let Some(playback) = &self.playback {
            playback.stop();
        }
    }

    fn is_down(&self) -> bool {
        self.engine.as_ref().is_some_and(Engine::is_down)
    }

    async fn shutdown(self) -> Result<(), SessionError> {
        // Consuming the player is what joins its thread; the engine drops with
        // the adapter.
        match self.playback {
            Some(playback) => playback.shutdown().await.map_err(SessionError::Playback),
            None => Ok(()),
        }
    }

    async fn retire(&mut self) -> Retired {
        if let Some(engine) = self.engine.take() {
            engine.close();
        }
        // The old streams end with the engine. Dropping them here is what keeps
        // a retired drain quiet, down to its end-of-stream device notice.
        self.events = None;
        self.devices = None;
        self.inbound = None;

        let Some(playback) = self.playback.take() else {
            return Retired::Joined;
        };
        match tokio::time::timeout(RETIRE_BUDGET, playback.shutdown()).await {
            Ok(Ok(())) => Retired::Joined,
            // A player that did not hand its thread back is still running: the
            // rebuild goes ahead and finds out whether the device is free.
            Ok(Err(_)) | Err(_) => Retired::Lingering,
        }
    }
}

#[cfg(test)]
mod tests;
