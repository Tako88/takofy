//! Session facade: the calls the command task makes on a live session.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use librespot::core::authentication::Credentials;
use librespot::core::{Session, SessionConfig};

use crate::album::AlbumError;
use crate::artist::ArtistError;
use crate::connect::{ClusterView, ConnectError, Steer};
use crate::context::{ContextError, Page, PageSource, Walker, resolve_uris};
use crate::credentials::AccessToken;
use crate::followed::FollowedError;
use crate::hold;
use crate::latest::Latest;
use crate::lyrics::LyricsError;
use crate::playback::{Playback, PlaybackError, PlaybackEvent, Volume};
use crate::playlist_edit::{self, PlaylistError};
use crate::presence::Presence;
use crate::registry::{Registry, Verdict};
use crate::remote::{
    LabelCache, ROOTLIST_WIRE, RemoteContext, RemoteRootlist, label_playlists, label_tracks,
    read_album, read_artist, read_followed_artists, read_followed_shows, read_liked_tracks,
    read_lyrics, read_playlist_state, read_portraits, read_related, read_release_tracks,
    read_saved_albums, read_saved_state, read_show, write_add, write_create, write_follow,
    write_like, write_move, write_remove, write_rename, write_rootlist_add, write_rootlist_move,
    write_rootlist_remove, write_saved,
};
use crate::rootlist::{RootEntry, RootlistError, fetch_all};
use crate::search::search_uri;
use crate::show::ShowError;
use crate::station::{StationError, read_neighbours, read_station};
use crate::transfer::{Resolution, TransferPlan};
use crate::watch::{self, Gate};
use takofy_model::home::{Station, StationSeed};
use takofy_model::{
    AlbumDetail, ArtistDetail, ArtistPortrait, CoverSet, FollowedArtist, FollowedShow, Landing,
    LikedTrack, Move, PlaylistAccess, PlaylistEntry, Removal, Report, RequestId, Revision,
    SavedAlbum, ShowDetail, TrackEntry, TrackLyrics,
};

/// Why an [`Engine`] operation failed.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("session error: {0}")]
    Session(#[from] librespot::core::Error),
    #[error(transparent)]
    Rootlist(#[from] RootlistError),
    #[error(transparent)]
    Context(#[from] ContextError),
    #[error(transparent)]
    Album(#[from] AlbumError),
    #[error(transparent)]
    Artist(#[from] ArtistError),
    #[error(transparent)]
    Show(#[from] ShowError),
    #[error(transparent)]
    Station(#[from] StationError),
    #[error(transparent)]
    Followed(#[from] FollowedError),
    #[error(transparent)]
    Lyrics(#[from] LyricsError),
    #[error(transparent)]
    Playlist(#[from] PlaylistError),
    #[error(transparent)]
    Connect(#[from] ConnectError),
}

/// Answers one incoming order while something else holds the engine.
pub trait OrderAnswer: Send + Sync {
    /// Answers one order, saying whether the entry was still pending.
    fn answer(&self, id: RequestId, ok: bool) -> bool;
}

/// A handle to the orders in flight, so one can be answered without owning the
/// engine that holds them.
pub type Orders = Arc<dyn OrderAnswer>;

/// The orders the engine's registry holds, reached through the handle.
struct RegistryOrders(Arc<Registry>);

impl OrderAnswer for RegistryOrders {
    fn answer(&self, id: RequestId, ok: bool) -> bool {
        let verdict = if ok {
            Verdict::Success
        } else {
            Verdict::Failure
        };
        self.0.answer_order(id, verdict)
    }
}

/// A connected Spotify session.
pub struct Engine {
    session: Session,
    gate: Arc<Gate>,
    /// What this device announces about itself, held so a play can be reported
    /// before anything is watching.
    presence: Arc<Presence>,
    /// The newest report the window published, kept before a watch so the first
    /// body already describes what plays here.
    latest: Arc<Latest>,
    /// The requests in flight, held so one can be refused before anything is
    /// watching.
    registry: Arc<Registry>,
    /// The bundle a call made before a watch reads, held for the engine's life
    /// so the hold a transfer records is not dropped with the call.
    detached: Arc<watch::Shared>,
    /// What the watch announced through, once one has started.
    shared: OnceLock<Arc<watch::Shared>>,
    /// Whether the pump was started; the dealer's one start cannot be repeated.
    watching: bool,
    /// The track labels this session has already read.
    labels: LabelCache,
}

impl Engine {
    /// The stage and the registry exist before any watch, because the window can
    /// report a playable device before it asks for its own entry.
    fn new(session: Session) -> Self {
        let presence = Arc::new(Presence::default());
        let latest = Arc::new(Latest::default());
        let registry = Arc::new(Registry::new(watch::Timing::default()));
        Self {
            session,
            gate: Arc::default(),
            detached: watch::Shared::detached(
                Arc::clone(&presence),
                Arc::clone(&latest),
                Arc::clone(&registry),
            ),
            presence,
            latest,
            registry,
            shared: OnceLock::new(),
            watching: false,
            labels: LabelCache::new(),
        }
    }

    /// Connects with an already-issued access token.
    pub async fn connect(cfg: SessionConfig, token: &AccessToken) -> Result<Self, EngineError> {
        let session = Session::new(cfg, None);
        session
            .connect(Credentials::with_access_token(token.expose()), false)
            .await?;
        Ok(Self::new(session))
    }

    /// Whether the session this engine was built on is known to be dead.
    ///
    /// librespot sets this on a closed socket and on its keep-alive timeout, so
    /// it stays false while a silent drop is still undetected.
    pub fn is_down(&self) -> bool {
        self.session.is_invalid()
    }

    /// Ends the session: every request in flight fails, and the dealer's stream
    /// closes with it. The engine cannot be used afterwards.
    pub fn close(self) {
        self.session.shutdown();
    }

    /// Starts watching the account's devices, answering the streams the window
    /// reads.
    ///
    /// The answer is one watch for the whole session: a second call is refused
    /// rather than started, because the dealer consumes its builder once.
    pub async fn watch_devices(&mut self) -> Result<watch::Watching, EngineError> {
        if self.watching {
            return Err(ConnectError::AlreadyWatching.into());
        }
        let watching = watch::watch(
            &self.session,
            Arc::clone(&self.gate),
            Arc::clone(&self.presence),
            Arc::clone(&self.latest),
            Arc::clone(&self.registry),
        )
        .await?;
        self.watching = true;
        // The one watch this engine can start fills the slot; a second attempt
        // is refused above, so there is nothing to replace.
        let _ = self.shared.set(Arc::clone(&watching.shared));
        Ok(watching)
    }

    /// Says whether this device can play, which is the stage it announces.
    ///
    /// Works before a watch: the stage is held here, and the announcement made
    /// when the watch starts reads it.
    pub fn set_playable(&self, playable: bool) {
        self.presence.set_playable(playable);
    }

    /// Answers one incoming request `Failure`.
    ///
    /// Works before a watch, like the stage: the registry answers a request the
    /// request loop handed over.
    pub fn refuse(&self, id: RequestId) {
        self.registry.refuse(id);
    }

    /// Publishes what the window is playing, or that it has nothing to publish.
    ///
    /// The slot lives here rather than in the watch, so setting it before the
    /// watch starts is not lost with the call that set it.
    pub fn report(&self, report: Option<Report>) {
        self.latest.set(report);
    }

    /// Re-applies what the window knew before this session existed.
    ///
    /// A fresh engine starts with an empty stage and no report, so a rebuild
    /// would otherwise make the device unplayable and forget what plays here.
    pub fn restore(&self, playable: bool, report: Option<Report>) {
        self.presence.set_playable(playable);
        self.latest.set(report);
    }

    /// Answers one order the window carried out, and says whether the entry was
    /// still pending.
    ///
    /// The verdict is the window's own. Either way its ack is recorded, because
    /// the next state put is what tells the phone the order was heard.
    pub fn answer_order(&self, id: RequestId, ok: bool) -> bool {
        self.orders().answer(id, ok)
    }

    /// A handle that answers orders without borrowing the engine, so a caller
    /// that already owns it can still answer a phone.
    pub fn orders(&self) -> Orders {
        Arc::new(RegistryOrders(Arc::clone(&self.registry)))
    }

    /// When a pending request's budget answers it.
    pub fn expires(&self, id: RequestId) -> Option<Instant> {
        self.registry.expires(id)
    }

    /// Asks the cluster for its roster again, answering what it says.
    pub async fn refresh_devices(&self) -> Result<ClusterView, EngineError> {
        let wire = watch::SessionWire::new(&self.session);
        let timing = watch::Timing::default();
        // A refresh before any watch still announces the stage this engine
        // holds, so it reads the same bundle rather than a throwaway one.
        let shared = self.bundle();
        Ok(watch::refresh(&wire, &self.gate, &shared, timing).await?)
    }

    /// Sends one transport action to the device that holds the sound.
    pub async fn steer_device(&self, target: &str, steer: &Steer) -> Result<(), EngineError> {
        let wire = watch::SessionWire::new(&self.session);
        Ok(watch::steer(&wire, &self.gate, target, steer, watch::Timing::default()).await?)
    }

    /// Moves the account's sound to another device, taking it first when this
    /// device does not hold it.
    pub async fn hand_over(&self, to: &str) -> Result<(), EngineError> {
        let wire = watch::SessionWire::new(&self.session);
        let shared = self.bundle();
        Ok(hold::pick(&wire, &self.gate, &shared, to, watch::Timing::default()).await?)
    }

    /// Takes this client off the account's device list.
    pub async fn withdraw_devices(&self) -> Result<(), EngineError> {
        let wire = watch::SessionWire::new(&self.session);
        Ok(watch::withdraw(&wire, &self.gate, &self.presence, watch::Timing::default()).await?)
    }

    /// Every track URI a context holds, in the walker's order.
    ///
    /// Bounded by the transfer's resolve budget: a read that hangs would hold a
    /// hand-over's answer open, and the phone is waiting for one.
    pub async fn resolve_context(&self, uri: &str) -> Result<Vec<String>, EngineError> {
        let source = RemoteContext {
            session: &self.session,
        };
        let budget = watch::Timing::default().resolve;
        Ok(resolve_within(source, uri, budget).await?)
    }

    /// Takes the account's sound for one hand-over, answering the phone.
    ///
    /// The hold is recorded in the bundle the pump announces from, so the report
    /// outlives this call and every later body describes the same hold.
    pub async fn accept_transfer(
        &self,
        id: RequestId,
        plan: &TransferPlan,
        resolution: Resolution,
    ) -> Result<Landing, EngineError> {
        let wire = watch::SessionWire::new(&self.session);
        let shared = self.bundle();
        Ok(hold::accept(
            &wire,
            &self.gate,
            &shared,
            id,
            plan,
            resolution,
            watch::Timing::default(),
        )
        .await?)
    }

    /// Gives the account's sound back, announcing what the device is now.
    pub async fn release_sound(&self) -> Result<(), EngineError> {
        let wire = watch::SessionWire::new(&self.session);
        let shared = self.bundle();
        hold::release(&wire, &self.gate, &shared, watch::Timing::default()).await?;
        Ok(())
    }

    /// Whether this device holds the account's sound.
    pub fn holding(&self) -> bool {
        self.presence.active()
    }

    /// The bundle the pump announces from, or the detached one before a watch.
    ///
    /// A transfer can only be accepted once the pump has named a connection, so
    /// the detached bundle answers refusals and never keeps a hold of its own.
    fn bundle(&self) -> Arc<watch::Shared> {
        match self.shared.get() {
            Some(shared) => Arc::clone(shared),
            None => Arc::clone(&self.detached),
        }
    }

    /// The user's playlists, labelled, with what the column may write.
    pub async fn rootlist(&self) -> Result<RootlistRead, EngineError> {
        let source = RemoteRootlist {
            session: &self.session,
            wire: ROOTLIST_WIRE,
        };
        let walked = fetch_all(&source).await?;
        Ok(RootlistRead {
            entries: label_playlists(&self.session, walked.playlists, &self.labels).await,
            access: walked.access,
        })
    }

    /// One playlist as the sidebar draws it: its name, cover and counts.
    ///
    /// A created list has none of these in the write's replies, so this is the
    /// read that fills them in without waiting for the next rootlist reload.
    pub async fn playlist_entry(&self, playlist_uri: &str) -> Option<PlaylistEntry> {
        let entry = RootEntry {
            uri: playlist_uri.to_owned(),
            saved_at: None,
        };
        label_playlists(&self.session, vec![entry], &self.labels)
            .await
            .pop()
    }

    /// Starts a player over this session.
    ///
    /// Needs an active Tokio runtime: the event forwarder is spawned on it.
    pub fn playback(
        &self,
        volume: Volume,
    ) -> Result<
        (
            Playback,
            tokio::sync::mpsc::UnboundedReceiver<PlaybackEvent>,
        ),
        PlaybackError,
    > {
        Playback::start(self.session.clone(), volume)
    }

    /// A paged, labelled view of a playlist's tracks.
    pub fn tracks(&self, playlist_uri: &str) -> TrackPages<'_> {
        TrackPages {
            session: &self.session,
            labels: &self.labels,
            walker: Walker::new(
                RemoteContext {
                    session: &self.session,
                },
                playlist_uri,
            ),
        }
    }

    /// The first page of a playlist's tracks, with what the page may change.
    ///
    /// The rows and the list's own revision are read together: one walk of the
    /// context's first page, and one request for the state a write must name. A
    /// state that cannot be read leaves the page with no access to offer.
    pub async fn playlist_page(&self, playlist_uri: &str) -> Result<PlaylistPage, EngineError> {
        let mut pages = self.tracks(playlist_uri);
        let (page, state) = tokio::join!(
            pages.next(),
            read_playlist_state(&self.session, playlist_uri)
        );
        let tracks = page?.unwrap_or_default();
        let shown = tracks
            .iter()
            .map(|track| track.uri.clone())
            .collect::<Vec<_>>();
        let access = state.and_then(|state| {
            playlist_edit::access(
                Some(state.revision),
                state.can_edit,
                state.owned,
                &state.items,
                &shown,
            )
        });
        let cover = page_cover(pages.cover(), &tracks);
        Ok(PlaylistPage {
            tracks,
            cover,
            access,
        })
    }

    /// Writes one move to a playlist, answering the revision to hold next.
    pub async fn move_track(
        &self,
        playlist_uri: &str,
        revision: &Revision,
        moves: &[Move],
    ) -> Result<Revision, EngineError> {
        Ok(write_move(&self.session, playlist_uri, revision, moves).await?)
    }

    /// Removes rows from a playlist, answering the revision to hold next.
    pub async fn remove_tracks(
        &self,
        playlist_uri: &str,
        revision: &Revision,
        removals: &[Removal],
    ) -> Result<Revision, EngineError> {
        Ok(write_remove(&self.session, playlist_uri, revision, removals).await?)
    }

    /// Writes one move to the user's rootlist, answering the revision to hold
    /// next.
    pub async fn move_playlist(
        &self,
        revision: &Revision,
        from: usize,
        to: usize,
    ) -> Result<Revision, EngineError> {
        Ok(write_rootlist_move(&self.session, revision, from, to).await?)
    }

    /// Removes one playlist from the user's rootlist, answering the revision the
    /// next write must name.
    ///
    /// The rootlist unlinks only: the playlist itself survives.
    pub async fn remove_playlist(
        &self,
        revision: &Revision,
        index: usize,
    ) -> Result<Revision, EngineError> {
        Ok(write_rootlist_remove(&self.session, revision, index).await?)
    }

    /// Writes one saved album or show out of the library.
    pub async fn set_saved(&self, uri: &str, saved: bool) -> Result<(), EngineError> {
        Ok(write_saved(&self.session, uri, saved).await?)
    }

    /// Whether one album or show is in the library, or `None` when the probe did
    /// not answer.
    ///
    /// This is the answer the library's own lists cannot give: a saved album is
    /// only listed when the albums read has run, and a page needs to know about
    /// its own album whatever the sidebar is showing.
    pub async fn saved_state(&self, uri: &str) -> Option<bool> {
        read_saved_state(&self.session, uri).await
    }

    /// Creates a playlist called `name`, answering its URI.
    ///
    /// The playlist is not on the rootlist until [`Self::link_playlist`] puts it
    /// there, and nothing else can find it before that.
    pub async fn make_playlist(&self, name: &str) -> Result<String, EngineError> {
        Ok(write_create(&self.session, name).await?)
    }

    /// Puts `playlist_uri` at the front of the user's rootlist, answering the
    /// revision to hold next.
    pub async fn link_playlist(
        &self,
        revision: &Revision,
        playlist_uri: &str,
    ) -> Result<Revision, EngineError> {
        Ok(write_rootlist_add(&self.session, revision, playlist_uri).await?)
    }

    /// What a playlist read says about writing to it, when the read answers.
    ///
    /// Best effort: a list that cannot be read yet is one the sidebar may not
    /// write to, which is the fail-safe the caller draws.
    pub async fn playlist_state(&self, playlist_uri: &str) -> Option<PlaylistAccess> {
        let state = read_playlist_state(&self.session, playlist_uri).await?;
        playlist_edit::listing_access(state.revision.as_bytes(), state.can_edit)
    }

    /// Renames a playlist, answering the revision to hold next.
    pub async fn rename_playlist(
        &self,
        playlist_uri: &str,
        revision: &Revision,
        name: &str,
    ) -> Result<Revision, EngineError> {
        Ok(write_rename(&self.session, playlist_uri, revision, name).await?)
    }

    /// Appends tracks to a playlist, answering the revision to hold next.
    pub async fn add_tracks(
        &self,
        playlist_uri: &str,
        revision: &Revision,
        from_index: usize,
        track_uris: &[String],
    ) -> Result<Revision, EngineError> {
        Ok(write_add(
            &self.session,
            playlist_uri,
            revision,
            from_index,
            track_uris,
        )
        .await?)
    }

    /// The tracks one search query answers, labelled, in the wire's own order.
    ///
    /// One page only: the search route answers twenty tracks and offers no
    /// paging, so this never walks. A blank query asks for nothing and answers
    /// nothing without a request.
    pub async fn search(&self, query: &str) -> Result<Vec<TrackEntry>, EngineError> {
        let source = RemoteContext {
            session: &self.session,
        };
        let Some(page) = search_page(source, query).await? else {
            return Ok(Vec::new());
        };
        Ok(label_tracks(&self.session, page.tracks, &self.labels).await)
    }

    /// One album, its tracks labelled.
    ///
    /// Unlike a playlist this is a single request and a single labelling pass:
    /// an album's order comes from the album, so there is no context to walk.
    pub async fn album(&self, album_uri: &str) -> Result<AlbumDetail, EngineError> {
        Ok(read_album(&self.session, album_uri, &self.labels).await?)
    }

    /// One artist: its page data plus whether the user follows it.
    pub async fn artist(
        &self,
        artist_uri: &str,
    ) -> Result<(ArtistDetail, Option<bool>), EngineError> {
        Ok(read_artist(&self.session, artist_uri, &self.labels).await?)
    }

    /// The related artists an artist page shows as cards, in the order given.
    ///
    /// A URI whose artist cannot be read keeps its URI and stays in the row.
    pub async fn related(&self, uris: Vec<String>) -> Result<Vec<FollowedArtist>, EngineError> {
        Ok(read_related(&self.session, uris).await?)
    }

    /// The artists' portraits, in the order asked for, from one batched read.
    ///
    /// `None` when the request failed or was abandoned: a speculative read, so
    /// its failure is silent and drawn like a cover that has not arrived.
    pub async fn portraits(&self, artists: &[String]) -> Option<Vec<ArtistPortrait>> {
        read_portraits(&self.session, artists).await
    }

    /// One release's track names, in the release's own order.
    ///
    /// Cannot fail: a track that cannot be labelled falls back to its URI.
    pub async fn release_tracks(&self, tracks: &[String]) -> Vec<TrackEntry> {
        read_release_tracks(&self.session, tracks, &self.labels).await
    }

    /// Writes the artist into the user's followed set, or out of it.
    pub async fn set_follow(&self, artist_uri: &str, follow: bool) -> Result<(), EngineError> {
        Ok(write_follow(&self.session, artist_uri, follow).await?)
    }

    /// One show: its metadata and every episode, labelled in the show's order.
    ///
    /// A URI that does not name a show fails before any request is made.
    pub async fn show(&self, show_uri: &str) -> Result<ShowDetail, EngineError> {
        Ok(read_show(&self.session, show_uri).await?)
    }

    /// One station read from its seed, its rows labelled.
    pub async fn station(&self, seed: &StationSeed) -> Result<Station, EngineError> {
        Ok(read_station(&self.session, &self.labels, seed).await?)
    }

    /// The artists a station names beside `artist`, which is where this client
    /// learns an artist's neighbours.
    pub async fn neighbours(&self, artist: &str) -> Result<Vec<FollowedArtist>, EngineError> {
        Ok(read_neighbours(&self.session, artist).await?)
    }

    /// One track's lyrics, empty when the track has none.
    ///
    /// A track without lyrics is an empty success rather than an error: the
    /// endpoint's not-found is the answer, not a failure. A URI that does not
    /// name a track fails before any request is made.
    pub async fn lyrics(&self, track_uri: &str) -> Result<TrackLyrics, EngineError> {
        Ok(read_lyrics(&self.session, track_uri).await?)
    }

    /// The shows the user follows, newest follow first, each as its card draws it.
    pub async fn followed_shows(&self) -> Result<Vec<FollowedShow>, EngineError> {
        Ok(read_followed_shows(&self.session).await?)
    }

    /// The artists the user follows, newest follow first, each as its card draws it.
    pub async fn followed_artists(&self) -> Result<Vec<FollowedArtist>, EngineError> {
        Ok(read_followed_artists(&self.session).await?)
    }

    /// The albums the user saves, newest save first, each as its card draws it.
    pub async fn saved_albums(&self) -> Result<Vec<SavedAlbum>, EngineError> {
        Ok(read_saved_albums(&self.session).await?)
    }

    /// The tracks the user has liked, newest save first.
    pub async fn liked_tracks(&self) -> Result<Vec<LikedTrack>, EngineError> {
        Ok(read_liked_tracks(&self.session).await?)
    }

    /// Writes one pick into the liked-songs set, or out of it.
    pub async fn set_liked(&self, tracks: Vec<String>, liked: bool) -> Result<(), EngineError> {
        Ok(write_like(&self.session, tracks, liked).await?)
    }
}

/// The rootlist: its entries, and what the column may write with.
///
/// `access` is `None` when the read named no revision, and its `editable` is
/// false whenever the read dropped an item, so a row index stops being the
/// wire's index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RootlistRead {
    pub entries: Vec<PlaylistEntry>,
    pub access: Option<PlaylistAccess>,
}

/// One playlist page: its rows, the context's own image, and what it may change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlaylistPage {
    pub tracks: Vec<TrackEntry>,
    pub cover: Option<CoverSet>,
    pub access: Option<PlaylistAccess>,
}

/// The first page one search query answers, through any page source.
///
/// Split out of [`Engine::search`] so the URI the walker resolves can be
/// asserted without a session. A blank query resolves nothing, and a search
/// answers at most one page.
async fn search_page<S: PageSource>(source: S, query: &str) -> Result<Option<Page>, ContextError> {
    let Some(uri) = search_uri(query) else {
        return Ok(None);
    };
    Walker::new(source, uri).next_page().await
}

/// Walks a context inside the budget a transfer can afford.
async fn resolve_within<S: PageSource>(
    source: S,
    uri: &str,
    budget: Duration,
) -> Result<Vec<String>, ContextError> {
    match tokio::time::timeout(budget, resolve_uris(source, uri)).await {
        Ok(result) => result,
        Err(_) => Err(ContextError::TimedOut),
    }
}

/// The track pages of one playlist.
pub struct TrackPages<'a> {
    session: &'a Session,
    labels: &'a LabelCache,
    walker: Walker<RemoteContext<'a>>,
}

impl TrackPages<'_> {
    /// The next page of labelled tracks, or `None` at the end.
    pub async fn next(&mut self) -> Result<Option<Vec<TrackEntry>>, EngineError> {
        let Some(page) = self.walker.next_page().await? else {
            return Ok(None);
        };
        Ok(Some(
            label_tracks(self.session, page.tracks, self.labels).await,
        ))
    }

    /// The context's own image, once the first page has been resolved.
    pub fn cover(&self) -> Option<CoverSet> {
        self.walker.cover()
    }
}

/// The page's cover: the context image, else the first Track-kind row's own.
///
/// The single-row derivation matches the listing's, so the band and the sidebar
/// row never disagree. A first track with no cover gives none rather than
/// falling through to the next row.
fn page_cover(context: Option<CoverSet>, rows: &[TrackEntry]) -> Option<CoverSet> {
    context.or_else(|| {
        rows.iter()
            .find(|row| row.is_track())
            .and_then(|row| row.cover.clone())
    })
}

#[cfg(test)]
mod tests;
