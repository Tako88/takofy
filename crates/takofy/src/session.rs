//! The session work the command task drives.
//!
//! The task reaches the engine through one trait, so its exit paths can be
//! exercised without a network or an audio device. The production adapter is
//! thin: a call-through or one `Option` check.

use std::future::Future;
use std::time::Instant;

use librespot::core::SessionConfig;
use takofy_core::connect::{ClusterEvent, ClusterView, ConnectError, Steer};
use takofy_core::credentials::AccessToken;
use takofy_core::engine::{Engine, EngineError, RootlistRead};
use takofy_core::playback::{Playback, PlaybackError, PlaybackEvent, Volume};
use takofy_core::receive::Inbound;
use takofy_core::{Resolution, TransferPlan};
use takofy_model::{
    AlbumDetail, ArtistDetail, ArtistPortrait, CoverSet, FollowedArtist, FollowedShow, Landing,
    Move, PlaylistAccess, PlaylistEntry, Removal, Report, RequestId, Revision, SavedAlbum,
    ShowDetail, TrackEntry, TrackLyrics,
};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::oneshot::Receiver;

/// Why a session-side operation failed, including the wiring's own state.
#[derive(Debug)]
pub(crate) enum SessionError {
    /// No engine exists yet, so the call had nothing to run on.
    NotConnected,
    /// The engine call itself failed.
    Engine(EngineError),
    /// The player could not be started, driven or shut down.
    Playback(PlaybackError),
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
            Self::Engine(EngineError::Session(_)) => "session",
            Self::Engine(EngineError::Rootlist(_)) => "rootlist",
            Self::Engine(EngineError::Context(_)) => "context",
            Self::Engine(EngineError::Album(_)) => "album",
            Self::Engine(EngineError::Artist(_)) => "artist",
            Self::Engine(EngineError::Show(_)) => "show",
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
/// written; `shutdown` consumes, because that is what joins the thread.
pub(crate) trait SessionHost: Send + 'static {
    fn connect(
        &mut self,
        token: &AccessToken,
    ) -> impl Future<Output = Result<(), SessionError>> + Send;

    fn rootlist(&self) -> impl Future<Output = Result<RootlistRead, SessionError>> + Send;

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
        &mut self,
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
    fn liked_tracks(&self) -> impl Future<Output = Result<Vec<String>, SessionError>> + Send;

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

    fn shutdown(self) -> impl Future<Output = Result<(), SessionError>> + Send;
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
        }
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
        self.engine = Some(engine);
        Ok(())
    }

    async fn rootlist(&self) -> Result<RootlistRead, SessionError> {
        match &self.engine {
            Some(engine) => engine.rootlist().await.map_err(SessionError::Engine),
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

    async fn tracks(&mut self, playlist: &str) -> Result<TrackPage, SessionError> {
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

    async fn liked_tracks(&self) -> Result<Vec<String>, SessionError> {
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
        if let Some(engine) = &self.engine {
            engine.set_playable(playable);
        }
    }

    fn report(&self, report: Option<Report>) {
        if let Some(engine) = &self.engine {
            engine.report(report);
        }
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

    async fn shutdown(self) -> Result<(), SessionError> {
        // Consuming the player is what joins its thread; the engine drops with
        // the adapter.
        match self.playback {
            Some(playback) => playback.shutdown().await.map_err(SessionError::Playback),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use librespot::core::SessionConfig;
    use librespot::playback::mixer::VolumeGetter;
    use takofy_core::connect::Steer;
    use takofy_core::playback::Volume;
    use takofy_core::{Resolution, TransferPlan};
    use takofy_model::RequestId;
    use tokio::sync::oneshot;

    use super::{LiveSession, SessionError, SessionHost, stop_or};

    #[tokio::test]
    async fn stop_or_returns_the_work_when_it_finishes_first() {
        let (_tx, mut stop) = oneshot::channel();
        assert_eq!(stop_or(&mut stop, async { 7 }).await, Some(7));
    }

    #[tokio::test]
    async fn stop_or_gives_up_when_the_stop_arrives_first() {
        let (tx, mut stop) = oneshot::channel();
        tx.send(()).expect("stop");
        assert_eq!(
            stop_or(&mut stop, std::future::pending::<u32>()).await,
            None
        );
    }

    #[test]
    fn a_level_set_before_the_player_exists_is_the_level_a_later_handle_reads() {
        let live = LiveSession::new(SessionConfig::default());
        live.set_volume(35);

        // The player is built with a clone of this cell, so what the clone
        // reads is what the player starts at.
        let handle = live.volume.clone();
        assert_eq!(
            handle.attenuation_factor(),
            Volume::at(35).attenuation_factor(),
            "a clone taken later reads the level set before the player existed"
        );

        live.set_volume(60);
        assert_eq!(
            handle.attenuation_factor(),
            Volume::at(60).attenuation_factor(),
            "a later change through the session is seen through the clone"
        );
    }

    #[tokio::test]
    async fn the_device_calls_before_sign_in_report_the_missing_engine() {
        let mut live = LiveSession::new(SessionConfig::default());

        assert!(matches!(
            SessionHost::watch_devices(&mut live).await,
            Err(SessionError::NotConnected)
        ));
        assert!(matches!(
            SessionHost::refresh_devices(&live).await,
            Err(SessionError::NotConnected)
        ));
        assert!(matches!(
            SessionHost::steer(&live, "phone", Steer::Pause).await,
            Err(SessionError::NotConnected)
        ));
        assert!(matches!(
            SessionHost::hand_over(&live, "phone").await,
            Err(SessionError::NotConnected)
        ));
        assert!(matches!(
            SessionHost::withdraw_devices(&live).await,
            Err(SessionError::NotConnected)
        ));
        assert!(SessionHost::take_devices(&mut live).is_none());
    }

    #[tokio::test]
    async fn a_watch_asked_for_twice_hands_back_the_stream_it_has() {
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut live = LiveSession::new(SessionConfig::default());
        live.devices = Some(rx);

        // The stream is the guard: a second request cannot start a second
        // watch, and a window that asks again gets what already exists.
        assert!(SessionHost::watch_devices(&mut live).await.is_ok());
        assert!(SessionHost::take_devices(&mut live).is_some());
        assert!(SessionHost::take_devices(&mut live).is_none());
    }

    #[tokio::test]
    async fn a_second_watch_after_success_returns_ok_without_a_second_start() {
        let (_devices_tx, devices) = tokio::sync::mpsc::unbounded_channel();
        let (_inbound_tx, inbound) = tokio::sync::mpsc::unbounded_channel();
        let mut live = LiveSession::new(SessionConfig::default());
        live.devices = Some(devices);
        live.inbound = Some(inbound);

        // The stored streams are the guard: a second request answers `Ok`
        // without reaching the engine, whose dealer start is already spent.
        assert!(SessionHost::watch_devices(&mut live).await.is_ok());
        assert!(SessionHost::take_devices(&mut live).is_some());
        assert!(SessionHost::take_inbound(&mut live).is_some());
        assert!(
            SessionHost::take_inbound(&mut live).is_none(),
            "the request stream is handed out once"
        );
    }

    #[tokio::test]
    async fn take_inbound_hands_the_stream_out_once() {
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut live = LiveSession::new(SessionConfig::default());
        live.inbound = Some(rx);

        assert!(SessionHost::take_inbound(&mut live).is_some());
        assert!(SessionHost::take_inbound(&mut live).is_none());
    }

    #[test]
    fn set_playable_and_refuse_before_sign_in_do_nothing_and_do_not_panic() {
        let live = LiveSession::new(SessionConfig::default());

        // Both are safe before there is an engine: the playability report and
        // the refusal have nowhere to go, and neither panics.
        SessionHost::set_playable(&live, true);
        SessionHost::set_playable(&live, false);
        SessionHost::refuse(&live, RequestId(1));
    }

    #[test]
    fn a_report_before_sign_in_is_ignored() {
        use takofy_model::{Report, Status, TrackEntry};

        let live = LiveSession::new(SessionConfig::default());
        let report = Report {
            status: Status::Playing,
            track: Some(TrackEntry {
                uri: "spotify:track:x".to_owned(),
                title: "x".to_owned(),
                artists: String::new(),
                album: None,
                album_uri: None,
                artist_uris: Vec::new(),
                artist_names: Vec::new(),
                added_at: None,
                duration_ms: None,
                named: true,
                cover: None,
            }),
            prev: Vec::new(),
            next: Vec::new(),
            context: None,
            queue_revision: String::new(),
            options: None,
            position_ms: 0,
            duration_ms: None,
        };

        // There is no engine to report to yet, so the call has nowhere to go and
        // must not panic.
        SessionHost::report(&live, Some(report));
        SessionHost::report(&live, None);
    }

    #[test]
    fn play_before_sign_in_fails_and_keeps_the_level() {
        let mut live = LiveSession::new(SessionConfig::default());
        live.set_volume(35);

        let error = SessionHost::play(&mut live, "spotify:track:x")
            .expect_err("a play with no engine must fail");
        assert!(
            matches!(error, SessionError::NotConnected),
            "a play before sign-in reports the missing engine"
        );
        assert_eq!(
            live.volume.attenuation_factor(),
            Volume::at(35).attenuation_factor(),
            "a failed play leaves the level alone"
        );
    }

    /// A plan can only be built from the wire's own shape, so a test outside
    /// core decodes one the way the request loop does.
    fn plan() -> TransferPlan {
        use librespot::core::dealer::protocol::{
            Command, LoggingParams, Request, TransferCommand, TransferOptions,
        };
        use librespot::protocol::context_track::ContextTrack;
        use librespot::protocol::playback::Playback as WirePlayback;
        use librespot::protocol::transfer_state::TransferState;
        use takofy_core::instruction::{Instruction, instruction_of};

        let track = ContextTrack {
            uri: Some("spotify:track:4uLU6hMCjMI75M1A2tKUQC".to_owned()),
            ..Default::default()
        };
        let playback = WirePlayback {
            current_track: Some(track).into(),
            ..Default::default()
        };
        let data = TransferState {
            playback: Some(playback).into(),
            ..Default::default()
        };
        let request = Request {
            message_id: 1,
            sent_by_device_id: "phone".to_owned(),
            command: Command::Transfer(TransferCommand {
                data: Some(data),
                options: TransferOptions::default(),
                from_device_identifier: String::new(),
                logging_params: LoggingParams {
                    interaction_ids: None,
                    device_identifier: None,
                    command_initiated_time: None,
                    page_instance_ids: None,
                    command_id: None,
                },
            }),
        };
        match instruction_of(&request) {
            Instruction::Transfer(plan) => plan,
            other => panic!("a transfer naming a track must decode to a plan: {other:?}"),
        }
    }

    #[tokio::test]
    async fn each_new_host_call_before_sign_in_returns_not_connected() {
        let mut live = LiveSession::new(SessionConfig::default());
        let plan = plan();

        assert!(matches!(
            SessionHost::prepare_player(&mut live),
            Err(SessionError::NotConnected)
        ));
        assert!(matches!(
            SessionHost::resolve_context(&live, "spotify:playlist:abc").await,
            Err(SessionError::NotConnected)
        ));
        assert!(matches!(
            SessionHost::load(&mut live, "spotify:track:x", 5_000, false),
            Err(SessionError::NotConnected)
        ));
        assert!(matches!(
            SessionHost::accept(&live, RequestId(1), &plan, Resolution::Resolved(Vec::new())).await,
            Err(SessionError::NotConnected)
        ));
        assert!(matches!(
            SessionHost::release_sound(&live).await,
            Err(SessionError::NotConnected)
        ));
        assert!(
            !SessionHost::holding(&live),
            "a window with no session holds nothing"
        );
    }
}
