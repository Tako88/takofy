//! Fixtures and probes shared by the bridge's tests.

use super::super::wake::Waker;
use super::super::*;
use super::announce::{
    AlbumScript, FollowedArtistsScript, FollowedScript, SavedAlbumsScript, ShowScript,
    followed_artist,
};
use super::create::{CreateScript, EntryScript, LinkScript};
use super::devices::{
    PHONE, PlayScript, RefreshScript, SteerScript, TransferScript, WithdrawScript, tablet,
};
use super::follow::{FollowScript, MoveScript};
use super::reads::{ArtistScript, LikedScript, LyricsScript, PortraitScript, SearchScript};
use super::recovery::RetireScript;
use super::transfer::{AcceptScript, PrepareScript, ResolveScript, default_landing};
use crate::session::{ArtistPage, Retired, SessionError, SessionHost, TrackPage};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use takofy_core::connect::{ClusterView, ConnectError};
use takofy_core::credentials::{AccessToken, CredentialStore, RefreshToken, StoreError};
use takofy_core::engine::{EngineError, RootlistRead};
use takofy_core::login::{SourceError, SourceErrorKind, TokenSet, TokenSource};
use takofy_core::playback::{PlaybackError, PlaybackEvent};
use takofy_core::{Resolution, TransferPlan};
use takofy_model::{
    AlbumDetail, ArtistPortrait, Device, DeviceKind, FollowedArtist, FollowedShow, Landing, Move,
    PlaylistAccess, PlaylistEntry, RemotePlayback, RemoteState, Removal, Report, RequestId,
    SavedAlbum, ShowDetail, Status, TrackEntry, TrackLyrics,
};
use tokio::runtime::Runtime;
use tokio::sync::mpsc::UnboundedSender;

pub(super) fn runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

/// A current-thread runtime keeps spawned tasks from running unless the
/// teardown future is actually pending on them. That is what makes the
/// `is_closed` assertions below fail when `finish` does not join, instead
/// of racing a worker thread and passing by accident.
pub(super) fn current_thread_runtime() -> Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

pub(super) fn access(value: &str) -> AccessToken {
    AccessToken::new(value.to_owned()).expect("non-empty access token")
}

pub(super) fn token_set(value: &str, refresh: Option<&str>) -> TokenSet {
    TokenSet {
        access: access(value),
        refresh: refresh.and_then(|secret| RefreshToken::new(secret.to_owned())),
    }
}

#[derive(Default)]
pub(super) struct RecordingWaker {
    wakes: AtomicUsize,
}

impl RecordingWaker {
    pub(super) fn wakes(&self) -> usize {
        self.wakes.load(Ordering::SeqCst)
    }
}

impl Waker for RecordingWaker {
    fn wake(&self) {
        self.wakes.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Default)]
pub(super) struct MemoryStore {
    token: Mutex<Option<RefreshToken>>,
    pub(super) unavailable: AtomicBool,
}

impl MemoryStore {
    pub(super) fn seeded(value: &str) -> Self {
        Self {
            token: Mutex::new(RefreshToken::new(value.to_owned())),
            ..Self::default()
        }
    }

    /// A store that cannot be read, as on a machine with no provider.
    pub(super) fn unavailable() -> Self {
        Self {
            unavailable: AtomicBool::new(true),
            ..Self::default()
        }
    }
}

impl CredentialStore for MemoryStore {
    fn load(&self) -> Result<Option<RefreshToken>, StoreError> {
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(StoreError::Unavailable);
        }
        Ok(self.token.lock().expect("lock").clone())
    }

    fn save(&self, token: &RefreshToken) -> Result<(), StoreError> {
        *self.token.lock().expect("lock") = Some(token.clone());
        Ok(())
    }

    fn clear(&self) -> Result<(), StoreError> {
        *self.token.lock().expect("lock") = None;
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct FixedSource {
    refresh: Mutex<Option<Result<TokenSet, SourceError>>>,
    interactive: Mutex<Option<Result<TokenSet, SourceError>>>,
}

impl FixedSource {
    pub(super) fn on_refresh(result: Result<TokenSet, SourceError>) -> Self {
        Self {
            refresh: Mutex::new(Some(result)),
            ..Self::default()
        }
    }

    pub(super) fn on_interactive(result: Result<TokenSet, SourceError>) -> Self {
        Self {
            interactive: Mutex::new(Some(result)),
            ..Self::default()
        }
    }

    pub(super) fn with(
        refresh: Result<TokenSet, SourceError>,
        interactive: Result<TokenSet, SourceError>,
    ) -> Self {
        Self {
            refresh: Mutex::new(Some(refresh)),
            interactive: Mutex::new(Some(interactive)),
        }
    }
}

impl TokenSource for FixedSource {
    fn refresh(&self, _token: &RefreshToken) -> Result<TokenSet, SourceError> {
        self.refresh
            .lock()
            .expect("lock")
            .take()
            .expect("refresh was scripted")
    }

    fn interactive(&self) -> Result<TokenSet, SourceError> {
        self.interactive
            .lock()
            .expect("lock")
            .take()
            .expect("interactive was scripted")
    }
}

/// A refresh source that answers a queue, and can hold one answer so a test
/// can act while it is in flight.
#[derive(Default)]
pub(super) struct ScriptedSource {
    refreshes: Mutex<std::collections::VecDeque<Result<TokenSet, SourceError>>>,
    /// One sender per refresh, so a test can see each one begin.
    started: Mutex<std::collections::VecDeque<Sender<()>>>,
    /// One gate per refresh; a refresh waits for its gate before answering.
    gates: Mutex<std::collections::VecDeque<Receiver<()>>>,
    /// How many refreshes have answered, so a test can count the attempts.
    pub(super) delivered: AtomicUsize,
}

impl ScriptedSource {
    pub(super) fn sequencer(results: Vec<Result<TokenSet, SourceError>>) -> Self {
        Self {
            refreshes: Mutex::new(results.into()),
            ..Self::default()
        }
    }

    /// Makes the next refresh announce its start and wait for the returned
    /// sender to release it.
    pub(super) fn hold_next(&self) -> (Sender<()>, Receiver<()>) {
        let (started, start) = channel();
        let (release, gate) = channel();
        self.started.lock().expect("lock").push_back(started);
        self.gates.lock().expect("lock").push_back(gate);
        (release, start)
    }
}

impl TokenSource for ScriptedSource {
    fn refresh(&self, _token: &RefreshToken) -> Result<TokenSet, SourceError> {
        let started = self.started.lock().expect("lock").pop_front();
        if let Some(started) = started {
            let _ = started.send(());
        }
        // Popped into a binding first: a gate held in the `if let` scrutinee
        // would keep this lock while it waits.
        let gate = self.gates.lock().expect("lock").pop_front();
        if let Some(gate) = gate {
            let _ = gate.recv();
        }
        let answer = self
            .refreshes
            .lock()
            .expect("lock")
            .pop_front()
            .unwrap_or(Err(SourceError {
                kind: SourceErrorKind::Other,
            }));
        self.delivered.fetch_add(1, Ordering::SeqCst);
        answer
    }

    fn interactive(&self) -> Result<TokenSet, SourceError> {
        Err(SourceError {
            kind: SourceErrorKind::Other,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum ConnectScript {
    #[default]
    Ok,
    Fail,
    Pending,
}

#[derive(Clone)]
pub(super) enum RootlistScript {
    Ready(Vec<PlaylistEntry>),
    /// A whole read, so the access the sidebar writes with is scripted too.
    ReadyWith(RootlistRead),
    Fail,
    Pending,
}

impl Default for RootlistScript {
    fn default() -> Self {
        Self::Ready(Vec::new())
    }
}

/// `Ready(TrackPage::default())` is a playlist with no tracks and no cover.
#[derive(Clone)]
pub(super) enum TracksScript {
    Ready(TrackPage),
    Fail,
    Pending,
}

impl Default for TracksScript {
    fn default() -> Self {
        Self::Ready(TrackPage::default())
    }
}

/// What a likeness read answers in a test.
#[derive(Clone, Default)]
pub(super) enum RelatedScript {
    /// The cards the artists are named with.
    #[default]
    Ready,
    /// The read fails, as a disconnected session does.
    #[allow(dead_code)]
    Fail,
}

/// A scripted like write.
#[derive(Clone, Copy, Default)]
pub(super) enum LikeScript {
    #[default]
    Ok,
    Fail,
    Pending,
}

/// A scripted release-label answer.
#[derive(Clone, Default)]
pub(super) enum ReleaseScript {
    #[default]
    Ready,
    Named(Vec<TrackEntry>),
    Fail,
}

/// A scripted rootlist move; the default names a revision the window has
/// not seen.
#[derive(Clone)]
pub(super) enum RootlistMoveScript {
    Ready(Revision),
    Fail,
    Pending,
}

impl Default for RootlistMoveScript {
    fn default() -> Self {
        Self::Ready(Revision::from_wire(&[2]).expect("a revision"))
    }
}

/// A scripted add write; the default names a revision the window has not
/// seen.
#[derive(Clone)]
pub(super) enum AddScript {
    Ready(Revision),
    Fail,
    Pending,
}

impl Default for AddScript {
    fn default() -> Self {
        Self::Ready(Revision::from_wire(&[3]).expect("a revision"))
    }
}

/// A scripted rename; the default names a revision the window has not seen.
#[derive(Clone)]
pub(super) enum RenameScript {
    Ready(Revision),
    Fail,
    Pending,
}

impl Default for RenameScript {
    fn default() -> Self {
        Self::Ready(Revision::from_wire(&[4]).expect("a revision"))
    }
}

/// One rename the fake host was asked for: the list, its base revision and
/// the name it should carry afterwards.
type RenameWrite = (String, Revision, String);

/// A scripted playlist-state read. The default answers nothing, the way a
/// list the session cannot see yet does.
#[derive(Clone, Default)]
pub(super) enum StateScript {
    #[default]
    Missing,
    Ready(PlaylistAccess),
    Pending,
}

/// One add the fake host was asked for: its target and everything it holds.
type AddWrite = (String, Revision, usize, Vec<String>);

/// A scripted watch start; the default succeeds and opens the stream.
#[derive(Clone, Default)]
pub(super) enum WatchScript {
    #[default]
    Ready,
    /// Succeeds once the test releases it, so a start can be held open.
    Held,
    /// A start that failed before anything was subscribed.
    Fail,
    /// The start reports success with no stream to hand over, which is
    /// what the engine answers once its stream has been taken.
    Spent,
    Pending,
}

/// A scripted positioned load. The default loads it.
#[derive(Clone, Copy, Default)]
pub(super) enum LoadScript {
    #[default]
    Ok,
    Fail,
    /// Starts only when the test releases it, so what the loop sent before
    /// the load can be read while it is still in flight.
    Held,
}

/// One host call, in the order the command loop made it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Call {
    Play(String),
    Prepare,
    Volume(u16),
    Resolve(String),
    Accept(RequestId),
    Answer(RequestId, bool),
    Report(Box<Option<takofy_model::Report>>),
    Label(Vec<String>),
    Load {
        uri: String,
        position_ms: u32,
        play: bool,
    },
    Release,
    SetPlayable(bool),
    Refuse(RequestId),
    HandOver(String),
    Retire,
}

#[derive(Default)]
pub(super) struct FakeState {
    pub(super) connect_scripts: Mutex<Vec<ConnectScript>>,
    pub(super) rootlist_script: Mutex<RootlistScript>,
    pub(super) entry_script: Mutex<EntryScript>,
    pub(super) entry_reads: AtomicUsize,
    pub(super) tracks_script: Mutex<TracksScript>,
    pub(super) album_script: Mutex<AlbumScript>,
    pub(super) show_script: Mutex<ShowScript>,
    pub(super) followed_script: Mutex<FollowedScript>,
    pub(super) followed_artists_script: Mutex<FollowedArtistsScript>,
    pub(super) related_script: Mutex<RelatedScript>,
    pub(super) saved_albums_script: Mutex<SavedAlbumsScript>,
    pub(super) liked_script: Mutex<LikedScript>,
    pub(super) like_script: Mutex<LikeScript>,
    pub(super) artist_script: Mutex<ArtistScript>,
    pub(super) lyrics_script: Mutex<LyricsScript>,
    pub(super) search_script: Mutex<SearchScript>,
    pub(super) portrait_script: Mutex<PortraitScript>,
    pub(super) release_script: Mutex<ReleaseScript>,
    pub(super) follow_script: Mutex<FollowScript>,
    pub(super) move_script: Mutex<MoveScript>,
    pub(super) remove_script: Mutex<MoveScript>,
    pub(super) rootlist_move_script: Mutex<RootlistMoveScript>,
    pub(super) remove_playlist_script: Mutex<RootlistMoveScript>,
    pub(super) remove_saved_script: Mutex<LikeScript>,
    pub(super) add_script: Mutex<AddScript>,
    pub(super) rename_script: Mutex<RenameScript>,
    pub(super) create_script: Mutex<CreateScript>,
    pub(super) link_script: Mutex<LinkScript>,
    pub(super) state_script: Mutex<StateScript>,
    pub(super) watch_scripts: Mutex<Vec<WatchScript>>,
    pub(super) refresh_script: Mutex<RefreshScript>,
    pub(super) steer_scripts: Mutex<Vec<SteerScript>>,
    pub(super) transfer_script: Mutex<TransferScript>,
    pub(super) withdraw_script: Mutex<WithdrawScript>,
    pub(super) play_script: Mutex<PlayScript>,
    pub(super) prepare_script: Mutex<PrepareScript>,
    pub(super) resolve_script: Mutex<ResolveScript>,
    pub(super) accept_script: Mutex<AcceptScript>,
    pub(super) load_script: Mutex<LoadScript>,
    /// The sender that says the held load started, and the receiver that
    /// lets it finish.
    pub(super) load_gate:
        Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
    /// The gate a held watch waits behind before it answers.
    pub(super) watch_gate: Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
    /// Whether giving the sound back is refused.
    pub(super) sound_release_fails: AtomicBool,
    /// The answer each holding check gives; the last entry repeats.
    pub(super) holding_script: Mutex<Vec<bool>>,
    /// The requests the fake registry still holds, with the deadline each
    /// was given. One leaves when it is answered.
    pub(super) pending: Mutex<Vec<(RequestId, Instant)>>,
    /// What the accept was handed, so a fallback can be asserted.
    pub(super) last_resolution: Mutex<Option<Resolution>>,
    /// Every call that moved the account's view of this device, in order.
    pub(super) calls: Mutex<Vec<Call>>,
    /// Whether the session is known to be dead.
    pub(super) down: AtomicBool,
    /// How retiring is scripted.
    pub(super) retire_script: Mutex<RetireScript>,
    pub(super) retire_started: Mutex<Option<Sender<()>>>,
    /// The connect count at each retirement, so the retire-before-connect
    /// order is visible.
    pub(super) retires_at_connect: Mutex<Vec<usize>>,
    /// The token the last connect was handed.
    pub(super) last_connect_token: Mutex<Option<String>>,
    /// Every request the loop refused, in order.
    pub(super) refusals: Mutex<Vec<RequestId>>,
    pub(super) connected: AtomicBool,
    pub(super) connects: AtomicUsize,
    pub(super) rootlists: AtomicUsize,
    pub(super) tracks: AtomicUsize,
    pub(super) albums: AtomicUsize,
    pub(super) shows: AtomicUsize,
    pub(super) followed_lists: AtomicUsize,
    pub(super) followed_artist_lists: AtomicUsize,
    pub(super) saved_album_lists: AtomicUsize,
    pub(super) liked_lists: AtomicUsize,
    pub(super) like_writes: AtomicUsize,
    pub(super) artists: AtomicUsize,
    pub(super) lyrics_loads: AtomicUsize,
    pub(super) searches: AtomicUsize,
    /// Every query asked for, in order, so an echo can be compared.
    pub(super) searched: Mutex<Vec<String>>,
    pub(super) portrait_reads: AtomicUsize,
    /// Every artist list asked for, in order.
    pub(super) portraited: Mutex<Vec<Vec<String>>>,
    pub(super) releases: AtomicUsize,
    pub(super) follow_writes: AtomicUsize,
    pub(super) move_writes: AtomicUsize,
    pub(super) remove_writes: AtomicUsize,
    pub(super) rootlist_move_writes: AtomicUsize,
    pub(super) remove_playlist_writes: AtomicUsize,
    pub(super) remove_saved_writes: AtomicUsize,
    pub(super) add_writes: AtomicUsize,
    pub(super) rename_writes: AtomicUsize,
    pub(super) create_writes: AtomicUsize,
    pub(super) link_writes: AtomicUsize,
    pub(super) state_reads: AtomicUsize,
    pub(super) artist_started: Mutex<Option<Sender<()>>>,
    pub(super) lyrics_started: Mutex<Option<Sender<()>>>,
    pub(super) search_started: Mutex<Option<Sender<()>>>,
    pub(super) portrait_started: Mutex<Option<Sender<()>>>,
    pub(super) show_started: Mutex<Option<Sender<()>>>,

    pub(super) last_follow: Mutex<Option<(String, bool)>>,
    pub(super) last_move: Mutex<Option<(String, Revision, Vec<Move>)>>,
    pub(super) last_remove: Mutex<Option<(String, Revision, Vec<Removal>)>>,
    pub(super) last_root_move: Mutex<Option<(Revision, usize, usize)>>,
    pub(super) last_playlist_remove: Mutex<Option<(Revision, usize)>>,
    pub(super) last_saved_remove: Mutex<Option<(String, bool)>>,
    pub(super) last_add: Mutex<Option<AddWrite>>,
    pub(super) last_rename: Mutex<Option<RenameWrite>>,
    pub(super) last_create: Mutex<Option<String>>,
    pub(super) last_link: Mutex<Option<(Revision, String)>>,
    pub(super) last_like: Mutex<Option<(Vec<String>, bool)>>,
    pub(super) move_started: Mutex<Option<Sender<()>>>,
    pub(super) rootlist_move_started: Mutex<Option<Sender<()>>>,
    pub(super) remove_playlist_started: Mutex<Option<Sender<()>>>,
    pub(super) remove_saved_started: Mutex<Option<Sender<()>>>,
    pub(super) probes: AtomicUsize,
    pub(super) last_probe: Mutex<Option<String>>,
    pub(super) probe_answer: Mutex<Option<bool>>,
    pub(super) probe_started: Mutex<Option<Sender<()>>>,
    pub(super) add_started: Mutex<Option<Sender<()>>>,
    pub(super) rename_started: Mutex<Option<Sender<()>>>,
    pub(super) create_started: Mutex<Option<Sender<()>>>,
    pub(super) link_started: Mutex<Option<Sender<()>>>,
    pub(super) state_started: Mutex<Option<Sender<()>>>,
    pub(super) like_started: Mutex<Option<Sender<()>>>,
    pub(super) plays: AtomicUsize,
    pub(super) pauses: AtomicUsize,
    pub(super) resumes: AtomicUsize,
    pub(super) stops: AtomicUsize,
    pub(super) seeks: AtomicUsize,
    pub(super) last_seek: AtomicUsize,
    pub(super) volumes: AtomicUsize,
    pub(super) last_volume: AtomicUsize,
    /// The level the fake player was built at, so a test can tell a level
    /// that arrived before the player existed from one that arrived after.
    pub(super) level_at_player_start: AtomicUsize,
    pub(super) players: AtomicUsize,
    pub(super) player_started: AtomicBool,
    pub(super) shutdowns: AtomicUsize,
    pub(super) watches: AtomicUsize,
    pub(super) refreshes: AtomicUsize,
    /// Every steer asked for, in order, so the target and the action can be
    /// compared.
    pub(super) steers: Mutex<Vec<(String, CoreSteer)>>,
    /// Every hand-over asked for, in order.
    pub(super) transfers: Mutex<Vec<String>>,
    pub(super) withdrawals: AtomicUsize,
    /// The calls the exit tail made, in order, so a withdraw can be seen to
    /// precede the shutdown.
    pub(super) exit_calls: Mutex<Vec<&'static str>>,
    pub(super) device_tx: Mutex<Option<UnboundedSender<ClusterEvent>>>,
    pub(super) device_rx: Mutex<Option<UnboundedReceiver<ClusterEvent>>>,
    pub(super) inbound_tx: Mutex<Option<UnboundedSender<Inbound>>>,
    pub(super) inbound_rx: Mutex<Option<UnboundedReceiver<Inbound>>>,
    pub(super) watch_started: Mutex<Option<Sender<()>>>,
    pub(super) refresh_started: Mutex<Option<Sender<()>>>,
    pub(super) steer_started: Mutex<Option<Sender<()>>>,
    pub(super) transfer_started: Mutex<Option<Sender<()>>>,
    pub(super) withdraw_started: Mutex<Option<Sender<()>>>,
    pub(super) connect_started: Mutex<Option<Sender<()>>>,
    pub(super) rootlist_started: Mutex<Option<Sender<()>>>,
    pub(super) tracks_started: Mutex<Option<Sender<()>>>,
    pub(super) album_started: Mutex<Option<Sender<()>>>,
    pub(super) play_started: Mutex<Option<Sender<()>>>,
    pub(super) events: Mutex<Option<UnboundedReceiver<PlaybackEvent>>>,
    pub(super) event_tx: Mutex<Option<UnboundedSender<PlaybackEvent>>>,
    /// How many times the one event stream was handed to a drain.
    pub(super) event_takes: AtomicUsize,
}

/// Records every session call so an exit path can be asserted without a
/// network or an audio device.
#[derive(Clone)]
pub(super) struct FakeHost {
    pub(super) shared: Arc<FakeState>,
}

impl FakeHost {
    pub(super) fn new(connect: ConnectScript, rootlist: RootlistScript) -> Self {
        Self::with_connect_scripts(vec![connect], rootlist)
    }

    /// One script per `connect` call; the last one repeats once the queue
    /// runs out, so a single-entry queue behaves like a fixed script.
    pub(super) fn with_connect_scripts(
        connect: Vec<ConnectScript>,
        rootlist: RootlistScript,
    ) -> Self {
        Self {
            shared: Arc::new(FakeState {
                connect_scripts: Mutex::new(connect),
                rootlist_script: Mutex::new(rootlist),
                // A fresh app starts at full, as `Volume::default()` does.
                last_volume: AtomicUsize::new(100),
                ..FakeState::default()
            }),
        }
    }

    /// Builds the fake player once, holding the event stream a drain takes.
    fn start_player(&self) {
        if self.shared.player_started.swap(true, Ordering::SeqCst) {
            return;
        }
        self.shared.players.fetch_add(1, Ordering::SeqCst);
        // The player is built with a handle to the level cell, so it starts
        // at whatever the level held when it was built.
        self.shared.level_at_player_start.store(
            self.shared.last_volume.load(Ordering::SeqCst),
            Ordering::SeqCst,
        );
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        *self.shared.event_tx.lock().expect("lock") = Some(tx);
        *self.shared.events.lock().expect("lock") = Some(rx);
    }
}

/// The fake registry, reached without the host that owns it.
struct FakeOrders(Arc<FakeState>);

impl takofy_core::OrderAnswer for FakeOrders {
    fn answer(&self, id: RequestId, ok: bool) -> bool {
        answer_order(&self.0, id, ok)
    }
}

/// Answers one order in the fake registry, saying whether it was pending.
fn answer_order(shared: &FakeState, id: RequestId, ok: bool) -> bool {
    shared
        .calls
        .lock()
        .expect("lock")
        .push(Call::Answer(id, ok));
    let mut pending = shared.pending.lock().expect("lock");
    let held = pending.len();
    pending.retain(|(waiting, _)| *waiting != id);
    pending.len() != held
}

impl SessionHost for FakeHost {
    fn connect(
        &mut self,
        token: &AccessToken,
    ) -> impl Future<Output = Result<(), SessionError>> + Send {
        self.shared.connects.fetch_add(1, Ordering::SeqCst);
        *self.shared.last_connect_token.lock().expect("lock") = Some(token.expose().to_owned());
        if let Some(tx) = self.shared.connect_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let script = {
            let mut scripts = self.shared.connect_scripts.lock().expect("lock");
            if scripts.len() > 1 {
                scripts.remove(0)
            } else {
                scripts.first().copied().unwrap_or_default()
            }
        };
        let shared = Arc::clone(&self.shared);
        async move {
            match script {
                ConnectScript::Ok => {
                    shared.connected.store(true, Ordering::SeqCst);
                    Ok(())
                }
                ConnectScript::Fail => Err(SessionError::NotConnected),
                ConnectScript::Pending => std::future::pending().await,
            }
        }
    }

    fn rootlist(&self) -> impl Future<Output = Result<RootlistRead, SessionError>> + Send {
        self.shared.rootlists.fetch_add(1, Ordering::SeqCst);
        if let Some(tx) = self.shared.rootlist_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let script = self.shared.rootlist_script.lock().expect("lock").clone();
        async move {
            match script {
                RootlistScript::Ready(list) => Ok(RootlistRead {
                    entries: list,
                    access: None,
                }),
                RootlistScript::ReadyWith(read) => Ok(read),
                RootlistScript::Fail => Err(SessionError::NotConnected),
                RootlistScript::Pending => std::future::pending().await,
            }
        }
    }

    fn playlist_entry(
        &self,
        _playlist: &str,
    ) -> impl Future<Output = Option<PlaylistEntry>> + Send {
        self.shared.entry_reads.fetch_add(1, Ordering::SeqCst);
        let script = self.shared.entry_script.lock().expect("lock").clone();
        async move {
            match script {
                EntryScript::Missing => None,
                EntryScript::Ready(entry) => Some(*entry),
            }
        }
    }

    fn tracks(
        &mut self,
        _playlist: &str,
    ) -> impl Future<Output = Result<TrackPage, SessionError>> + Send {
        self.shared.tracks.fetch_add(1, Ordering::SeqCst);
        if let Some(tx) = self.shared.tracks_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self.shared.tracks_script.lock().expect("lock").clone();
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                TracksScript::Ready(page) => Ok(page),
                TracksScript::Fail => Err(SessionError::NotConnected),
                TracksScript::Pending => std::future::pending().await,
            }
        }
    }

    fn album(
        &self,
        _album: &str,
    ) -> impl Future<Output = Result<AlbumDetail, SessionError>> + Send {
        self.shared.albums.fetch_add(1, Ordering::SeqCst);
        if let Some(tx) = self.shared.album_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self.shared.album_script.lock().expect("lock").clone();
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                AlbumScript::Ready(detail) => Ok(detail),
                AlbumScript::Fail => Err(SessionError::NotConnected),
            }
        }
    }

    fn show(&self, _show: &str) -> impl Future<Output = Result<ShowDetail, SessionError>> + Send {
        self.shared.shows.fetch_add(1, Ordering::SeqCst);
        if let Some(tx) = self.shared.show_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self.shared.show_script.lock().expect("lock").clone();
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                ShowScript::Ready(detail) => Ok(detail),
                ShowScript::Fail => Err(SessionError::NotConnected),
                ShowScript::Pending => std::future::pending().await,
            }
        }
    }

    fn followed_shows(
        &self,
    ) -> impl Future<Output = Result<Vec<FollowedShow>, SessionError>> + Send {
        self.shared.followed_lists.fetch_add(1, Ordering::SeqCst);
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self.shared.followed_script.lock().expect("lock").clone();
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                FollowedScript::Ready(shows) => Ok(shows),
                FollowedScript::Fail => Err(SessionError::NotConnected),
            }
        }
    }

    fn followed_artists(
        &self,
    ) -> impl Future<Output = Result<Vec<FollowedArtist>, SessionError>> + Send {
        self.shared
            .followed_artist_lists
            .fetch_add(1, Ordering::SeqCst);
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self
            .shared
            .followed_artists_script
            .lock()
            .expect("lock")
            .clone();
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                FollowedArtistsScript::Ready(artists) => Ok(artists),
                FollowedArtistsScript::Fail => Err(SessionError::NotConnected),
            }
        }
    }

    fn related(
        &self,
        uris: Vec<String>,
    ) -> impl Future<Output = Result<Vec<FollowedArtist>, SessionError>> + Send {
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self.shared.related_script.lock().expect("lock").clone();
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                RelatedScript::Ready => Ok(uris.iter().map(|uri| followed_artist(uri)).collect()),
                RelatedScript::Fail => Err(SessionError::NotConnected),
            }
        }
    }

    fn saved_albums(&self) -> impl Future<Output = Result<Vec<SavedAlbum>, SessionError>> + Send {
        self.shared.saved_album_lists.fetch_add(1, Ordering::SeqCst);
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self
            .shared
            .saved_albums_script
            .lock()
            .expect("lock")
            .clone();
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                SavedAlbumsScript::Ready(albums) => Ok(albums),
                SavedAlbumsScript::Fail => Err(SessionError::NotConnected),
            }
        }
    }

    fn liked_tracks(&self) -> impl Future<Output = Result<Vec<String>, SessionError>> + Send {
        self.shared.liked_lists.fetch_add(1, Ordering::SeqCst);
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self.shared.liked_script.lock().expect("lock").clone();
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                LikedScript::Ready(tracks) => Ok(tracks),
                LikedScript::Fail => Err(SessionError::NotConnected),
            }
        }
    }

    fn set_liked(
        &self,
        tracks: Vec<String>,
        liked: bool,
    ) -> impl Future<Output = Result<(), SessionError>> + Send {
        self.shared.like_writes.fetch_add(1, Ordering::SeqCst);
        *self.shared.last_like.lock().expect("lock") = Some((tracks, liked));
        if let Some(tx) = self.shared.like_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = *self.shared.like_script.lock().expect("lock");
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                LikeScript::Ok => Ok(()),
                LikeScript::Fail => Err(SessionError::NotConnected),
                LikeScript::Pending => std::future::pending().await,
            }
        }
    }

    fn artist(
        &self,
        _artist: &str,
    ) -> impl Future<Output = Result<ArtistPage, SessionError>> + Send {
        self.shared.artists.fetch_add(1, Ordering::SeqCst);
        if let Some(tx) = self.shared.artist_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self.shared.artist_script.lock().expect("lock").clone();
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                ArtistScript::Ready(page) => Ok(page),
                ArtistScript::Fail => Err(SessionError::NotConnected),
            }
        }
    }

    fn lyrics(
        &self,
        _track: &str,
    ) -> impl Future<Output = Result<TrackLyrics, SessionError>> + Send {
        self.shared.lyrics_loads.fetch_add(1, Ordering::SeqCst);
        if let Some(tx) = self.shared.lyrics_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self.shared.lyrics_script.lock().expect("lock").clone();
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                LyricsScript::Ready(lyrics) => Ok(lyrics),
                LyricsScript::Fail => Err(SessionError::NotConnected),
                LyricsScript::Pending => std::future::pending().await,
            }
        }
    }

    fn search(
        &self,
        query: &str,
    ) -> impl Future<Output = Result<Vec<TrackEntry>, SessionError>> + Send {
        self.shared.searches.fetch_add(1, Ordering::SeqCst);
        self.shared
            .searched
            .lock()
            .expect("lock")
            .push(query.to_owned());
        if let Some(tx) = self.shared.search_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self.shared.search_script.lock().expect("lock").clone();
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                SearchScript::Ready(tracks) => Ok(tracks),
                SearchScript::Fail => Err(SessionError::NotConnected),
                SearchScript::Pending => std::future::pending().await,
            }
        }
    }

    fn portraits(
        &self,
        artists: Vec<String>,
    ) -> impl Future<Output = Option<Vec<ArtistPortrait>>> + Send {
        self.shared.portrait_reads.fetch_add(1, Ordering::SeqCst);
        self.shared.portraited.lock().expect("lock").push(artists);
        if let Some(tx) = self.shared.portrait_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self.shared.portrait_script.lock().expect("lock").clone();
        async move {
            if !connected {
                return None;
            }
            match script {
                PortraitScript::Ready(portraits) => Some(portraits),
                PortraitScript::Fail => None,
                PortraitScript::Pending => std::future::pending().await,
            }
        }
    }

    fn release_tracks(
        &self,
        tracks: &[String],
    ) -> impl Future<Output = Result<Vec<TrackEntry>, SessionError>> + Send {
        self.shared.releases.fetch_add(1, Ordering::SeqCst);
        self.shared
            .calls
            .lock()
            .expect("lock")
            .push(Call::Label(tracks.to_vec()));
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self.shared.release_script.lock().expect("lock").clone();
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                ReleaseScript::Ready => Ok(Vec::new()),
                ReleaseScript::Named(tracks) => Ok(tracks),
                ReleaseScript::Fail => Err(SessionError::NotConnected),
            }
        }
    }

    fn set_follow(
        &self,
        artist: &str,
        follow: bool,
    ) -> impl Future<Output = Result<(), SessionError>> + Send {
        self.shared.follow_writes.fetch_add(1, Ordering::SeqCst);
        *self.shared.last_follow.lock().expect("lock") = Some((artist.to_owned(), follow));
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = *self.shared.follow_script.lock().expect("lock");
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                FollowScript::Ok => Ok(()),
                FollowScript::Fail => Err(SessionError::NotConnected),
            }
        }
    }

    fn move_track(
        &self,
        playlist: &str,
        revision: &Revision,
        moves: &[Move],
    ) -> impl Future<Output = Result<Revision, SessionError>> + Send {
        self.shared.move_writes.fetch_add(1, Ordering::SeqCst);
        *self.shared.last_move.lock().expect("lock") =
            Some((playlist.to_owned(), revision.clone(), moves.to_vec()));
        if let Some(tx) = self.shared.move_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self.shared.move_script.lock().expect("lock").clone();
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                MoveScript::Ready(revision) => Ok(revision),
                MoveScript::Fail => Err(SessionError::NotConnected),
                MoveScript::Pending => std::future::pending().await,
            }
        }
    }

    fn remove_tracks(
        &self,
        playlist: &str,
        revision: &Revision,
        removals: &[Removal],
    ) -> impl Future<Output = Result<Revision, SessionError>> + Send {
        self.shared.remove_writes.fetch_add(1, Ordering::SeqCst);
        *self.shared.last_remove.lock().expect("lock") =
            Some((playlist.to_owned(), revision.clone(), removals.to_vec()));
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self.shared.remove_script.lock().expect("lock").clone();
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                MoveScript::Ready(revision) => Ok(revision),
                MoveScript::Fail => Err(SessionError::NotConnected),
                MoveScript::Pending => std::future::pending().await,
            }
        }
    }

    fn move_playlist(
        &self,
        revision: &Revision,
        from: usize,
        to: usize,
    ) -> impl Future<Output = Result<Revision, SessionError>> + Send {
        self.shared
            .rootlist_move_writes
            .fetch_add(1, Ordering::SeqCst);
        *self.shared.last_root_move.lock().expect("lock") = Some((revision.clone(), from, to));
        if let Some(tx) = self
            .shared
            .rootlist_move_started
            .lock()
            .expect("lock")
            .take()
        {
            let _ = tx.send(());
        }
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self
            .shared
            .rootlist_move_script
            .lock()
            .expect("lock")
            .clone();
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                RootlistMoveScript::Ready(revision) => Ok(revision),
                RootlistMoveScript::Fail => Err(SessionError::NotConnected),
                RootlistMoveScript::Pending => std::future::pending().await,
            }
        }
    }

    fn remove_playlist(
        &self,
        revision: &Revision,
        index: usize,
    ) -> impl Future<Output = Result<Revision, SessionError>> + Send {
        self.shared
            .remove_playlist_writes
            .fetch_add(1, Ordering::SeqCst);
        *self.shared.last_playlist_remove.lock().expect("lock") = Some((revision.clone(), index));
        if let Some(tx) = self
            .shared
            .remove_playlist_started
            .lock()
            .expect("lock")
            .take()
        {
            let _ = tx.send(());
        }
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self
            .shared
            .remove_playlist_script
            .lock()
            .expect("lock")
            .clone();
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                RootlistMoveScript::Ready(revision) => Ok(revision),
                RootlistMoveScript::Fail => Err(SessionError::NotConnected),
                RootlistMoveScript::Pending => std::future::pending().await,
            }
        }
    }

    fn saved_state(&self, uri: &str) -> impl Future<Output = Option<bool>> + Send {
        self.shared.probes.fetch_add(1, Ordering::SeqCst);
        *self.shared.last_probe.lock().expect("lock") = Some(uri.to_owned());
        let answer = *self.shared.probe_answer.lock().expect("lock");
        if let Some(tx) = self.shared.probe_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        async move { answer }
    }

    fn set_saved(
        &self,
        uri: &str,
        saved: bool,
    ) -> impl Future<Output = Result<(), SessionError>> + Send {
        self.shared
            .remove_saved_writes
            .fetch_add(1, Ordering::SeqCst);
        *self.shared.last_saved_remove.lock().expect("lock") = Some((uri.to_owned(), saved));
        if let Some(tx) = self
            .shared
            .remove_saved_started
            .lock()
            .expect("lock")
            .take()
        {
            let _ = tx.send(());
        }
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = *self.shared.remove_saved_script.lock().expect("lock");
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                LikeScript::Ok => Ok(()),
                LikeScript::Fail => Err(SessionError::NotConnected),
                LikeScript::Pending => std::future::pending().await,
            }
        }
    }

    fn add_tracks(
        &self,
        playlist: &str,
        revision: &Revision,
        from_index: usize,
        tracks: &[String],
    ) -> impl Future<Output = Result<Revision, SessionError>> + Send {
        self.shared.add_writes.fetch_add(1, Ordering::SeqCst);
        *self.shared.last_add.lock().expect("lock") = Some((
            playlist.to_owned(),
            revision.clone(),
            from_index,
            tracks.to_vec(),
        ));
        if let Some(tx) = self.shared.add_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self.shared.add_script.lock().expect("lock").clone();
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                AddScript::Ready(revision) => Ok(revision),
                AddScript::Fail => Err(SessionError::NotConnected),
                AddScript::Pending => std::future::pending().await,
            }
        }
    }

    fn rename_playlist(
        &self,
        playlist: &str,
        revision: &Revision,
        name: &str,
    ) -> impl Future<Output = Result<Revision, SessionError>> + Send {
        self.shared.rename_writes.fetch_add(1, Ordering::SeqCst);
        *self.shared.last_rename.lock().expect("lock") =
            Some((playlist.to_owned(), revision.clone(), name.to_owned()));
        if let Some(tx) = self.shared.rename_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self.shared.rename_script.lock().expect("lock").clone();
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                RenameScript::Ready(revision) => Ok(revision),
                RenameScript::Fail => Err(SessionError::NotConnected),
                RenameScript::Pending => std::future::pending().await,
            }
        }
    }

    fn make_playlist(
        &self,
        name: &str,
    ) -> impl Future<Output = Result<String, SessionError>> + Send {
        self.shared.create_writes.fetch_add(1, Ordering::SeqCst);
        *self.shared.last_create.lock().expect("lock") = Some(name.to_owned());
        if let Some(tx) = self.shared.create_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self.shared.create_script.lock().expect("lock").clone();
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                CreateScript::Ready(uri) => Ok(uri),
                CreateScript::Fail => Err(SessionError::NotConnected),
                CreateScript::Pending => std::future::pending().await,
            }
        }
    }

    fn link_playlist(
        &self,
        revision: &Revision,
        playlist: &str,
    ) -> impl Future<Output = Result<Revision, SessionError>> + Send {
        self.shared.link_writes.fetch_add(1, Ordering::SeqCst);
        *self.shared.last_link.lock().expect("lock") =
            Some((revision.clone(), playlist.to_owned()));
        if let Some(tx) = self.shared.link_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self.shared.link_script.lock().expect("lock").clone();
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                LinkScript::Ready(revision) => Ok(revision),
                LinkScript::Fail => Err(SessionError::NotConnected),
                LinkScript::Pending => std::future::pending().await,
            }
        }
    }

    fn playlist_state(
        &self,
        _playlist: &str,
    ) -> impl Future<Output = Option<PlaylistAccess>> + Send {
        self.shared.state_reads.fetch_add(1, Ordering::SeqCst);
        if let Some(tx) = self.shared.state_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self.shared.state_script.lock().expect("lock").clone();
        async move {
            if !connected {
                return None;
            }
            match script {
                StateScript::Missing => None,
                StateScript::Ready(access) => Some(access),
                StateScript::Pending => std::future::pending().await,
            }
        }
    }

    fn watch_devices(&mut self) -> impl Future<Output = Result<(), SessionError>> + Send {
        self.shared.watches.fetch_add(1, Ordering::SeqCst);
        let script = {
            let mut scripts = self.shared.watch_scripts.lock().expect("lock");
            if scripts.len() > 1 {
                scripts.remove(0)
            } else {
                scripts.first().cloned().unwrap_or_default()
            }
        };
        if matches!(script, WatchScript::Ready | WatchScript::Held) {
            // The streams exist before the signal fires, so a test that
            // waits for the start can use them straight away.
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            *self.shared.device_tx.lock().expect("lock") = Some(tx);
            *self.shared.device_rx.lock().expect("lock") = Some(rx);
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            *self.shared.inbound_tx.lock().expect("lock") = Some(tx);
            *self.shared.inbound_rx.lock().expect("lock") = Some(rx);
        }
        if let Some(tx) = self.shared.watch_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let gate = self.shared.watch_gate.lock().expect("lock").take();
        async move {
            match script {
                WatchScript::Ready => Ok(()),
                WatchScript::Held => {
                    if let Some(gate) = gate {
                        let _ = gate.await;
                    }
                    Ok(())
                }
                WatchScript::Spent => Err(SessionError::Engine(EngineError::Connect(
                    ConnectError::DealerSpent,
                ))),
                WatchScript::Fail => Err(SessionError::NotConnected),
                WatchScript::Pending => std::future::pending().await,
            }
        }
    }

    fn take_devices(&mut self) -> Option<UnboundedReceiver<ClusterEvent>> {
        self.shared.device_rx.lock().expect("lock").take()
    }

    fn take_inbound(&mut self) -> Option<UnboundedReceiver<Inbound>> {
        self.shared.inbound_rx.lock().expect("lock").take()
    }

    fn set_playable(&self, playable: bool) {
        self.shared
            .calls
            .lock()
            .expect("lock")
            .push(Call::SetPlayable(playable));
    }

    fn report(&self, report: Option<takofy_model::Report>) {
        self.shared
            .calls
            .lock()
            .expect("lock")
            .push(Call::Report(Box::new(report)));
    }

    fn refuse(&self, id: RequestId) {
        self.shared
            .calls
            .lock()
            .expect("lock")
            .push(Call::Refuse(id));
        self.shared.refusals.lock().expect("lock").push(id);
    }

    fn refresh_devices(&self) -> impl Future<Output = Result<ClusterView, SessionError>> + Send {
        self.shared.refreshes.fetch_add(1, Ordering::SeqCst);
        if let Some(tx) = self.shared.refresh_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let script = self.shared.refresh_script.lock().expect("lock").clone();
        async move {
            match script {
                RefreshScript::Phone => Ok(phone_view()),
                RefreshScript::TwoDevices => Ok(two_device_view()),
                RefreshScript::Fail => Err(SessionError::NotConnected),
                RefreshScript::Pending => std::future::pending().await,
            }
        }
    }

    fn steer(
        &self,
        device: &str,
        steer: CoreSteer,
    ) -> impl Future<Output = Result<(), SessionError>> + Send {
        self.shared
            .steers
            .lock()
            .expect("lock")
            .push((device.to_owned(), steer));
        if let Some(tx) = self.shared.steer_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let script = {
            let mut scripts = self.shared.steer_scripts.lock().expect("lock");
            if scripts.len() > 1 {
                scripts.remove(0)
            } else {
                scripts.first().copied().unwrap_or_default()
            }
        };
        async move {
            match script {
                SteerScript::Ok => Ok(()),
                SteerScript::Fail => Err(SessionError::NotConnected),
            }
        }
    }

    fn hand_over(&self, device: &str) -> impl Future<Output = Result<(), SessionError>> + Send {
        self.shared
            .transfers
            .lock()
            .expect("lock")
            .push(device.to_owned());
        self.shared
            .calls
            .lock()
            .expect("lock")
            .push(Call::HandOver(device.to_owned()));
        if let Some(tx) = self.shared.transfer_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let script = *self.shared.transfer_script.lock().expect("lock");
        async move {
            match script {
                TransferScript::Ok => Ok(()),
                TransferScript::Fail => Err(SessionError::NotConnected),
                TransferScript::NoContext => Err(SessionError::Engine(EngineError::Connect(
                    ConnectError::NoContext,
                ))),
                TransferScript::Pending => std::future::pending().await,
            }
        }
    }

    fn withdraw_devices(&self) -> impl Future<Output = Result<(), SessionError>> + Send {
        self.shared.withdrawals.fetch_add(1, Ordering::SeqCst);
        self.shared
            .exit_calls
            .lock()
            .expect("lock")
            .push("withdraw");
        if let Some(tx) = self.shared.withdraw_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let script = *self.shared.withdraw_script.lock().expect("lock");
        async move {
            match script {
                WithdrawScript::Ok => Ok(()),
                WithdrawScript::Pending => std::future::pending().await,
            }
        }
    }

    fn play(&mut self, uri: &str) -> Result<(), SessionError> {
        self.shared.plays.fetch_add(1, Ordering::SeqCst);
        self.shared
            .calls
            .lock()
            .expect("lock")
            .push(Call::Play(uri.to_owned()));
        if !self.shared.connected.load(Ordering::SeqCst) {
            return Err(SessionError::NotConnected);
        }
        let script = *self.shared.play_script.lock().expect("lock");
        match script {
            PlayScript::Ok => {}
            PlayScript::NoAudioDevice => {
                return Err(SessionError::Playback(PlaybackError::NoAudioDevice));
            }
            PlayScript::NotPlayable => {
                return Err(SessionError::Playback(PlaybackError::NotPlayable(
                    uri.to_owned(),
                )));
            }
        }
        self.start_player();
        if let Some(tx) = self.shared.play_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        Ok(())
    }

    fn take_events(&mut self) -> Option<UnboundedReceiver<PlaybackEvent>> {
        self.shared.event_takes.fetch_add(1, Ordering::SeqCst);
        self.shared.events.lock().expect("lock").take()
    }

    fn prepare_player(&mut self) -> Result<(), SessionError> {
        self.shared.calls.lock().expect("lock").push(Call::Prepare);
        if !self.shared.connected.load(Ordering::SeqCst) {
            return Err(SessionError::NotConnected);
        }
        match *self.shared.prepare_script.lock().expect("lock") {
            PrepareScript::Ok => {
                self.start_player();
                Ok(())
            }
            PrepareScript::NoAudioDevice => {
                Err(SessionError::Playback(PlaybackError::NoAudioDevice))
            }
        }
    }

    fn resolve_context(
        &self,
        uri: &str,
    ) -> impl Future<Output = Result<Vec<String>, SessionError>> + Send {
        self.shared
            .calls
            .lock()
            .expect("lock")
            .push(Call::Resolve(uri.to_owned()));
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let script = self.shared.resolve_script.lock().expect("lock").clone();
        async move {
            if !connected {
                return Err(SessionError::NotConnected);
            }
            match script {
                ResolveScript::Ok(uris) => Ok(uris),
                ResolveScript::Fail => Err(SessionError::NotConnected),
                ResolveScript::Pending => std::future::pending().await,
            }
        }
    }

    async fn accept(
        &self,
        id: RequestId,
        _plan: &TransferPlan,
        resolution: Resolution,
    ) -> Result<Landing, SessionError> {
        self.shared
            .calls
            .lock()
            .expect("lock")
            .push(Call::Accept(id));
        *self.shared.last_resolution.lock().expect("lock") = Some(resolution);
        let script = self.shared.accept_script.lock().expect("lock").clone();
        match script {
            // The real accept answers the phone either way, so the fake does
            // too and the bridge must not answer a second time.
            AcceptScript::Ok => {
                self.shared
                    .calls
                    .lock()
                    .expect("lock")
                    .push(Call::Answer(id, true));
                Ok(default_landing())
            }
            AcceptScript::Landed(landing) => {
                self.shared
                    .calls
                    .lock()
                    .expect("lock")
                    .push(Call::Answer(id, true));
                Ok(*landing)
            }
            AcceptScript::Fail => {
                self.shared
                    .calls
                    .lock()
                    .expect("lock")
                    .push(Call::Answer(id, false));
                Err(SessionError::NotConnected)
            }
            AcceptScript::Pending => std::future::pending().await,
        }
    }

    fn load(&mut self, uri: &str, position_ms: u32, play: bool) -> Result<(), SessionError> {
        self.shared.calls.lock().expect("lock").push(Call::Load {
            uri: uri.to_owned(),
            position_ms,
            play,
        });
        if !self.shared.connected.load(Ordering::SeqCst) {
            return Err(SessionError::NotConnected);
        }
        match *self.shared.load_script.lock().expect("lock") {
            LoadScript::Ok => {
                self.start_player();
                Ok(())
            }
            LoadScript::Fail => Err(SessionError::Playback(PlaybackError::NotPlayable(
                uri.to_owned(),
            ))),
            LoadScript::Held => {
                let gate = self.shared.load_gate.lock().expect("lock").take();
                if let Some((started, release)) = gate {
                    let _ = started.send(());
                    // A test that fails before releasing still lets the
                    // window finish, so a failure is not a hang.
                    let _ = release.recv_timeout(Duration::from_secs(2));
                }
                self.start_player();
                Ok(())
            }
        }
    }

    fn release_sound(&self) -> impl Future<Output = Result<(), SessionError>> + Send {
        self.shared.calls.lock().expect("lock").push(Call::Release);
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let refused = self.shared.sound_release_fails.load(Ordering::SeqCst);
        async move {
            if connected && !refused {
                Ok(())
            } else {
                Err(SessionError::NotConnected)
            }
        }
    }

    fn holding(&self) -> bool {
        let mut script = self.shared.holding_script.lock().expect("lock");
        if script.len() > 1 {
            script.remove(0)
        } else {
            script.first().copied().unwrap_or(false)
        }
    }

    fn answer_order(&self, id: RequestId, ok: bool) -> bool {
        answer_order(&self.shared, id, ok)
    }

    fn orders(&self) -> Option<takofy_core::Orders> {
        Some(Arc::new(FakeOrders(Arc::clone(&self.shared))))
    }

    fn expires(&self, id: RequestId) -> Option<Instant> {
        self.shared
            .pending
            .lock()
            .expect("lock")
            .iter()
            .find(|(waiting, _)| *waiting == id)
            .map(|(_, expires)| *expires)
    }

    fn pause(&self) {
        self.shared.pauses.fetch_add(1, Ordering::SeqCst);
    }

    fn resume(&self) {
        self.shared.resumes.fetch_add(1, Ordering::SeqCst);
    }

    fn stop(&self) {
        self.shared.stops.fetch_add(1, Ordering::SeqCst);
    }

    fn seek(&self, position_ms: u32) {
        self.shared.seeks.fetch_add(1, Ordering::SeqCst);
        self.shared
            .last_seek
            .store(position_ms as usize, Ordering::SeqCst);
    }

    fn set_volume(&self, percent: u16) {
        self.shared
            .calls
            .lock()
            .expect("lock")
            .push(Call::Volume(percent));
        self.shared.volumes.fetch_add(1, Ordering::SeqCst);
        self.shared
            .last_volume
            .store(usize::from(percent), Ordering::SeqCst);
    }

    fn is_down(&self) -> bool {
        self.shared.down.load(Ordering::SeqCst)
    }

    fn shutdown(self) -> impl Future<Output = Result<(), SessionError>> + Send {
        self.shared.shutdowns.fetch_add(1, Ordering::SeqCst);
        self.shared
            .exit_calls
            .lock()
            .expect("lock")
            .push("shutdown");
        async move { Ok(()) }
    }

    fn retire(&mut self) -> impl Future<Output = Retired> + Send {
        self.shared.calls.lock().expect("lock").push(Call::Retire);
        self.shared
            .retires_at_connect
            .lock()
            .expect("lock")
            .push(self.shared.connects.load(Ordering::SeqCst));
        if let Some(tx) = self.shared.retire_started.lock().expect("lock").take() {
            let _ = tx.send(());
        }
        let script = *self.shared.retire_script.lock().expect("lock");
        // The session is gone with it, so the next connect and the next
        // player are built from scratch, and the streams the old drains
        // read end here.
        self.shared.player_started.store(false, Ordering::SeqCst);
        self.shared.connected.store(false, Ordering::SeqCst);
        self.shared.down.store(false, Ordering::SeqCst);
        *self.shared.device_tx.lock().expect("lock") = None;
        *self.shared.inbound_tx.lock().expect("lock") = None;
        *self.shared.event_tx.lock().expect("lock") = None;
        async move {
            match script {
                RetireScript::Ok => Retired::Joined,
                RetireScript::Pending => std::future::pending().await,
            }
        }
    }
}

/// The artist the shell tests open.
pub(super) const ARTIST: &str = "spotify:artist:a";

/// The track the lyrics tests open.
pub(super) const TRACK: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ1";

/// A row the context read answers with, after [`TRACK`].
pub(super) const SECOND: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ2";

/// The deadline the fake registry gives each request, standing in for the
/// answer budget the real one runs on.
const ORDER_BUDGET: Duration = Duration::from_secs(5);

/// Registers one request in the fake registry, so an order finds it pending,
/// and answers with the deadline its budget ends it at.
pub(super) fn hold_request(shared: &FakeState, id: RequestId) -> Instant {
    let deadline = Instant::now() + ORDER_BUDGET;
    shared.pending.lock().expect("lock").push((id, deadline));
    deadline
}

pub(super) fn playlist(uri: &str, name: &str) -> PlaylistEntry {
    PlaylistEntry {
        saved_at: None,
        composed: Vec::new(),
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        cover: None,
        description: None,
        owner: None,
        tracks: None,
        access: None,
    }
}

pub(super) fn revision(byte: u8) -> Revision {
    Revision::from_wire(&[byte]).expect("a revision")
}

pub(super) fn expect_updates(ui_end: &UiEnd, count: usize) -> Vec<Update> {
    (0..count)
        .map(|_| ui_end.updates.recv().expect("update"))
        .collect()
}

pub(super) fn track(uri: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: uri.to_owned(),
        artists: String::new(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

pub(super) fn report(uri: &str) -> Report {
    Report {
        status: Status::Playing,
        track: Some(track(uri)),
        prev: Vec::new(),
        next: Vec::new(),
        context: None,
        queue_revision: String::new(),
        options: None,
        position_ms: 0,
        duration_ms: None,
    }
}

pub(super) fn phone() -> Device {
    Device {
        id: PHONE.to_owned(),
        name: "Phone".to_owned(),
        kind: DeviceKind::Phone,
    }
}

fn remote_playback() -> RemotePlayback {
    RemotePlayback {
        track: track("spotify:track:remote"),
        state: RemoteState::Playing,
        position_ms: 1_000,
        duration_ms: Some(200_000),
        steerable: true,
    }
}

pub(super) fn phone_view() -> ClusterView {
    ClusterView {
        devices: Some(vec![phone()]),
        holder: Some(PHONE.to_owned()),
        held_here: false,
        changed_ms: 0,
        playback: Some(remote_playback()),
    }
}

fn two_device_view() -> ClusterView {
    ClusterView {
        devices: Some(vec![phone(), tablet()]),
        holder: Some(PHONE.to_owned()),
        held_here: false,
        changed_ms: 0,
        playback: Some(remote_playback()),
    }
}

/// Asserts the worker sent nothing within a wait long enough to notice.
pub(super) fn expect_no_update(ui_end: &UiEnd) {
    match ui_end.updates.recv_timeout(Duration::from_millis(100)) {
        Err(RecvTimeoutError::Timeout) => {}
        Err(RecvTimeoutError::Disconnected) => panic!("the worker went away"),
        Ok(update) => panic!("expected no update, got {update:?}"),
    }
}

/// Waits until `check` holds, so no test has to guess at a delay.
pub(super) fn until(what: &str, mut check: impl FnMut() -> bool) {
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(2) {
        if check() {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("timed out waiting for {what}");
}

/// Every call the loop made that moved the account's view of this device.
pub(super) fn calls(shared: &FakeState) -> Vec<Call> {
    shared.calls.lock().expect("lock").clone()
}

/// Every request the loop refused, in order.
pub(super) fn refusals(shared: &FakeState) -> Vec<RequestId> {
    shared.refusals.lock().expect("lock").clone()
}

/// How many times one inbound was answered, either way.
pub(super) fn answers(shared: &FakeState, id: RequestId) -> usize {
    calls(shared)
        .iter()
        .filter(|call| {
            matches!(call, Call::Refuse(answered) if *answered == id)
                || matches!(call, Call::Answer(answered, _) if *answered == id)
        })
        .count()
}

/// The request stream a scripted watch opened, so a test can push into it.
pub(super) fn inbound_sender(shared: &FakeState) -> UnboundedSender<Inbound> {
    shared
        .inbound_tx
        .lock()
        .expect("lock")
        .clone()
        .expect("a watch opened the request stream")
}

/// Watches once, answering the signal the fake raises when it is called.
/// Starts the device watch and waits for it, then forgets the calls it made.
///
/// The watch readies the player so the device can be announced as able to
/// play, which is setup rather than the scenario a test is about.
pub(super) fn watch_now(ui_end: &UiEnd, shared: &FakeState) {
    let (tx, rx) = channel();
    *shared.watch_started.lock().expect("lock") = Some(tx);
    ui_end.commands.send(Command::WatchDevices).expect("watch");
    rx.recv().expect("watch started");
    shared.calls.lock().expect("lock").clear();
}

pub(super) fn device_roster() -> Update {
    Update::Devices(DeviceUpdate::Roster {
        devices: vec![phone()],
        holder: Some(PHONE.to_owned()),
        held_here: false,
    })
}

pub(super) fn device_playing() -> Update {
    Update::Devices(DeviceUpdate::Playing(Some(remote_playback())))
}
