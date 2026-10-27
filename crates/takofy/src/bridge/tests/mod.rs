use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use takofy_core::connect::{ClusterView, ConnectError, WITHDRAW_TIMEOUT};
use takofy_core::credentials::{AccessToken, CredentialStore, RefreshToken, StoreError};
use takofy_core::engine::{EngineError, RootlistRead};
use takofy_core::instruction::{Instruction, Kind, Refusal};
use takofy_core::login::{AuthError, SourceError, SourceErrorKind, TokenSet, TokenSource};
use takofy_core::playback::{PlaybackError, PlaybackEvent};
use takofy_core::{ORDER_MARGIN, Resolution, TransferPlan};
use takofy_model::{
    AlbumDetail, ArtistDetail, ArtistPortrait, CoverImage, CoverSet, Device, DeviceKind, Episode,
    FollowedArtist, FollowedShow, Landing, LyricLine, Move, Order, PlaylistAccess, PlaylistCover,
    PlaylistEntry, RemotePlayback, RemoteState, Removal, Report, RequestId, SavedAlbum, Show,
    ShowDetail, Status, TrackEntry, TrackLyrics,
};
use takofy_ui::signin::failure_message;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::UnboundedSender;

use super::playback::transport_of;
use super::signin::{authorize_update, failure_of};
use super::wake::Waker;
use super::*;
use crate::cover::spawn_cover;
use crate::playhead::Playhead;
use crate::session::{ArtistPage, Retired, SessionError, SessionHost, TrackPage};

fn runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

/// A current-thread runtime keeps spawned tasks from running unless the
/// teardown future is actually pending on them. That is what makes the
/// `is_closed` assertions below fail when `finish` does not join, instead
/// of racing a worker thread and passing by accident.
fn current_thread_runtime() -> Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn access(value: &str) -> AccessToken {
    AccessToken::new(value.to_owned()).expect("non-empty access token")
}

fn token_set(value: &str, refresh: Option<&str>) -> TokenSet {
    TokenSet {
        access: access(value),
        refresh: refresh.and_then(|secret| RefreshToken::new(secret.to_owned())),
    }
}

fn auth_source(kind: SourceErrorKind) -> AuthError {
    AuthError::Source(SourceError { kind })
}

#[derive(Default)]
struct RecordingWaker {
    wakes: AtomicUsize,
}

impl RecordingWaker {
    fn wakes(&self) -> usize {
        self.wakes.load(Ordering::SeqCst)
    }
}

impl Waker for RecordingWaker {
    fn wake(&self) {
        self.wakes.fetch_add(1, Ordering::SeqCst);
    }
}

/// A waker that peeks at the channel when it runs, so the test can see
/// whether the update was queued first.
struct QueueProbe {
    rx: Mutex<Receiver<Update>>,
    seen: Mutex<Option<Update>>,
    wakes: AtomicUsize,
}

impl QueueProbe {
    fn new(rx: Receiver<Update>) -> Self {
        Self {
            rx: Mutex::new(rx),
            seen: Mutex::new(None),
            wakes: AtomicUsize::new(0),
        }
    }

    fn seen(&self) -> Option<Update> {
        self.seen.lock().expect("lock").clone()
    }

    fn wakes(&self) -> usize {
        self.wakes.load(Ordering::SeqCst)
    }
}

impl Waker for QueueProbe {
    fn wake(&self) {
        self.wakes.fetch_add(1, Ordering::SeqCst);
        let seen = self.rx.lock().expect("lock").try_recv().ok();
        *self.seen.lock().expect("lock") = seen;
    }
}

#[derive(Default)]
struct MemoryStore {
    token: Mutex<Option<RefreshToken>>,
    unavailable: AtomicBool,
}

impl MemoryStore {
    fn seeded(value: &str) -> Self {
        Self {
            token: Mutex::new(RefreshToken::new(value.to_owned())),
            ..Self::default()
        }
    }

    /// A store that cannot be read, as on a machine with no provider.
    fn unavailable() -> Self {
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
struct FixedSource {
    refresh: Mutex<Option<Result<TokenSet, SourceError>>>,
    interactive: Mutex<Option<Result<TokenSet, SourceError>>>,
}

impl FixedSource {
    fn on_refresh(result: Result<TokenSet, SourceError>) -> Self {
        Self {
            refresh: Mutex::new(Some(result)),
            ..Self::default()
        }
    }

    fn on_interactive(result: Result<TokenSet, SourceError>) -> Self {
        Self {
            interactive: Mutex::new(Some(result)),
            ..Self::default()
        }
    }

    fn with(
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
struct ScriptedSource {
    refreshes: Mutex<std::collections::VecDeque<Result<TokenSet, SourceError>>>,
    /// One sender per refresh, so a test can see each one begin.
    started: Mutex<std::collections::VecDeque<Sender<()>>>,
    /// One gate per refresh; a refresh waits for its gate before answering.
    gates: Mutex<std::collections::VecDeque<Receiver<()>>>,
    /// How many refreshes have answered, so a test can count the attempts.
    delivered: AtomicUsize,
}

impl ScriptedSource {
    fn sequencer(results: Vec<Result<TokenSet, SourceError>>) -> Self {
        Self {
            refreshes: Mutex::new(results.into()),
            ..Self::default()
        }
    }

    /// Makes the next refresh announce its start and wait for the returned
    /// sender to release it.
    fn hold_next(&self) -> (Sender<()>, Receiver<()>) {
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

fn good(value: &str) -> Result<TokenSet, SourceError> {
    Ok(token_set(value, None))
}

fn no_token() -> Result<TokenSet, SourceError> {
    Err(SourceError {
        kind: SourceErrorKind::Other,
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum ConnectScript {
    #[default]
    Ok,
    Fail,
    Pending,
}

#[derive(Clone)]
enum RootlistScript {
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
enum TracksScript {
    Ready(TrackPage),
    Fail,
    Pending,
}

impl Default for TracksScript {
    fn default() -> Self {
        Self::Ready(TrackPage::default())
    }
}

/// A scripted album answer; the default is a bare one at [`ALBUM`].
#[derive(Clone)]
enum AlbumScript {
    Ready(AlbumDetail),
    Fail,
}

impl Default for AlbumScript {
    fn default() -> Self {
        Self::Ready(album_detail(ALBUM))
    }
}

/// A scripted show answer; the default is a bare one at [`SHOW`].
///
/// `Pending` never answers, so a stop can be tested while the load is still
/// in flight (the same shape [`TracksScript`] uses).
#[derive(Clone)]
enum ShowScript {
    Ready(ShowDetail),
    Fail,
    Pending,
}

impl Default for ShowScript {
    fn default() -> Self {
        Self::Ready(show_detail(SHOW))
    }
}

/// A scripted followed-shows answer; the default is an empty list.
#[derive(Clone)]
enum FollowedScript {
    Ready(Vec<FollowedShow>),
    Fail,
}

impl Default for FollowedScript {
    fn default() -> Self {
        Self::Ready(Vec::new())
    }
}

/// A scripted followed-artists answer; the default is an empty list.
#[derive(Clone)]
enum FollowedArtistsScript {
    Ready(Vec<FollowedArtist>),
    Fail,
}

/// What a likeness read answers in a test.
#[derive(Clone, Default)]
enum RelatedScript {
    /// The cards the artists are named with.
    #[default]
    Ready,
    /// The read fails, as a disconnected session does.
    #[allow(dead_code)]
    Fail,
}

impl Default for FollowedArtistsScript {
    fn default() -> Self {
        Self::Ready(Vec::new())
    }
}

/// A scripted saved-albums answer; the default is an empty list.
#[derive(Clone)]
enum SavedAlbumsScript {
    Ready(Vec<SavedAlbum>),
    Fail,
}

impl Default for SavedAlbumsScript {
    fn default() -> Self {
        Self::Ready(Vec::new())
    }
}

/// A scripted liked-tracks answer; the default is an empty set.
#[derive(Clone)]
enum LikedScript {
    Ready(Vec<String>),
    Fail,
}

impl Default for LikedScript {
    fn default() -> Self {
        Self::Ready(Vec::new())
    }
}

/// A scripted like write.
#[derive(Clone, Copy, Default)]
enum LikeScript {
    #[default]
    Ok,
    Fail,
    Pending,
}

/// A scripted artist answer; the default is a bare one at [`ARTIST`].
#[derive(Clone)]
enum ArtistScript {
    Ready(ArtistPage),
    Fail,
}

impl Default for ArtistScript {
    fn default() -> Self {
        Self::Ready(ArtistPage {
            artist: artist_detail(ARTIST),
            followed: Some(false),
        })
    }
}

/// A scripted lyrics answer; the default is no lines.
///
/// `Pending` never answers, so a stop can be tested while the load is still
/// in flight (the same shape [`ShowScript`] uses).
#[derive(Clone)]
enum LyricsScript {
    Ready(TrackLyrics),
    Fail,
    Pending,
}

impl Default for LyricsScript {
    fn default() -> Self {
        Self::Ready(TrackLyrics::default())
    }
}

/// A scripted search answer; the default is no tracks.
///
/// `Pending` never answers, so a stop can be tested while the load is still
/// in flight (the same shape [`LyricsScript`] uses).
#[derive(Clone)]
enum SearchScript {
    Ready(Vec<TrackEntry>),
    Fail,
    Pending,
}

impl Default for SearchScript {
    fn default() -> Self {
        Self::Ready(Vec::new())
    }
}

/// A scripted portrait read; the default is no portraits.
///
/// `Pending` never answers, so a stop can be tested while the read is still
/// in flight (the same shape [`SearchScript`] uses).
#[derive(Clone)]
enum PortraitScript {
    Ready(Vec<ArtistPortrait>),
    Fail,
    Pending,
}

impl Default for PortraitScript {
    fn default() -> Self {
        Self::Ready(Vec::new())
    }
}

/// A scripted release-label answer.
#[derive(Clone, Default)]
enum ReleaseScript {
    #[default]
    Ready,
    Named(Vec<TrackEntry>),
    Fail,
}

/// A scripted follow write.
#[derive(Clone, Copy, Default)]
enum FollowScript {
    #[default]
    Ok,
    Fail,
}

/// A scripted move write; the default names a revision the window has not
/// seen, so a test that cares names its own.
#[derive(Clone)]
enum MoveScript {
    Ready(Revision),
    Fail,
    Pending,
}

impl Default for MoveScript {
    fn default() -> Self {
        Self::Ready(Revision::from_wire(&[1]).expect("a revision"))
    }
}

/// A scripted rootlist move; the default names a revision the window has
/// not seen.
#[derive(Clone)]
enum RootlistMoveScript {
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
enum AddScript {
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
enum RenameScript {
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

/// A scripted one-playlist read; the default answers nothing.
#[derive(Clone, Default)]
enum EntryScript {
    #[default]
    Missing,
    Ready(Box<PlaylistEntry>),
}

/// The playlist a scripted create makes.
const CREATED: &str = "spotify:playlist:0JQ5DAqbMKFEC4WFtoNRpw";

/// A scripted create; the default makes [`CREATED`].
#[derive(Clone)]
enum CreateScript {
    Ready(String),
    Fail,
    Pending,
}

impl Default for CreateScript {
    fn default() -> Self {
        Self::Ready(CREATED.to_owned())
    }
}

/// A scripted rootlist link; the default names a revision the window has
/// not seen.
#[derive(Clone)]
enum LinkScript {
    Ready(Revision),
    Fail,
    Pending,
}

impl Default for LinkScript {
    fn default() -> Self {
        Self::Ready(Revision::from_wire(&[4]).expect("a revision"))
    }
}

/// A scripted playlist-state read. The default answers nothing, the way a
/// list the session cannot see yet does.
#[derive(Clone, Default)]
enum StateScript {
    #[default]
    Missing,
    Ready(PlaylistAccess),
    Pending,
}

/// One add the fake host was asked for: its target and everything it holds.
type AddWrite = (String, Revision, usize, Vec<String>);

/// A scripted watch start; the default succeeds and opens the stream.
#[derive(Clone, Default)]
enum WatchScript {
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

/// A scripted device refresh; the default answers the phone that holds the
/// sound.
#[derive(Clone, Default)]
enum RefreshScript {
    #[default]
    Phone,
    TwoDevices,
    Fail,
    Pending,
}

/// A scripted steer. The default obeys.
#[derive(Clone, Copy, Default)]
enum SteerScript {
    #[default]
    Ok,
    Fail,
}

/// A scripted hand-over. The default moves the sound.
#[derive(Clone, Copy, Default)]
enum TransferScript {
    #[default]
    Ok,
    Fail,
    NoContext,
    Pending,
}

/// A scripted withdraw. The default answers.
#[derive(Clone, Copy, Default)]
enum WithdrawScript {
    #[default]
    Ok,
    Pending,
}

/// A scripted play. The default starts the player.
#[derive(Clone, Copy, Default)]
enum PlayScript {
    #[default]
    Ok,
    /// The machine has no output, so nothing can ever play here.
    NoAudioDevice,
    /// One track the player refuses; the player itself is fine.
    NotPlayable,
}

/// A scripted player start. The default starts it.
#[derive(Clone, Copy, Default)]
enum PrepareScript {
    #[default]
    Ok,
    NoAudioDevice,
}

/// A scripted context read. The default answers no rows.
#[derive(Clone)]
enum ResolveScript {
    Ok(Vec<String>),
    Fail,
    Pending,
}

impl Default for ResolveScript {
    fn default() -> Self {
        Self::Ok(Vec::new())
    }
}

/// A scripted hand-over. The default lands on the test track, playing.
#[derive(Clone, Default)]
enum AcceptScript {
    #[default]
    Ok,
    Landed(Box<Landing>),
    Fail,
    Pending,
}

/// A scripted positioned load. The default loads it.
#[derive(Clone, Copy, Default)]
enum LoadScript {
    #[default]
    Ok,
    Fail,
    /// Starts only when the test releases it, so what the loop sent before
    /// the load can be read while it is still in flight.
    Held,
}

/// A scripted session retirement. The default joins at once.
#[derive(Clone, Copy, Default)]
enum RetireScript {
    #[default]
    Ok,
    /// Never answers, so a test can stop the window mid-retirement.
    Pending,
}

/// One host call, in the order the command loop made it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Call {
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
struct FakeState {
    connect_scripts: Mutex<Vec<ConnectScript>>,
    rootlist_script: Mutex<RootlistScript>,
    entry_script: Mutex<EntryScript>,
    entry_reads: AtomicUsize,
    tracks_script: Mutex<TracksScript>,
    album_script: Mutex<AlbumScript>,
    show_script: Mutex<ShowScript>,
    followed_script: Mutex<FollowedScript>,
    followed_artists_script: Mutex<FollowedArtistsScript>,
    related_script: Mutex<RelatedScript>,
    saved_albums_script: Mutex<SavedAlbumsScript>,
    liked_script: Mutex<LikedScript>,
    like_script: Mutex<LikeScript>,
    artist_script: Mutex<ArtistScript>,
    lyrics_script: Mutex<LyricsScript>,
    search_script: Mutex<SearchScript>,
    portrait_script: Mutex<PortraitScript>,
    release_script: Mutex<ReleaseScript>,
    follow_script: Mutex<FollowScript>,
    move_script: Mutex<MoveScript>,
    remove_script: Mutex<MoveScript>,
    rootlist_move_script: Mutex<RootlistMoveScript>,
    remove_playlist_script: Mutex<RootlistMoveScript>,
    remove_saved_script: Mutex<LikeScript>,
    add_script: Mutex<AddScript>,
    rename_script: Mutex<RenameScript>,
    create_script: Mutex<CreateScript>,
    link_script: Mutex<LinkScript>,
    state_script: Mutex<StateScript>,
    watch_scripts: Mutex<Vec<WatchScript>>,
    refresh_script: Mutex<RefreshScript>,
    steer_scripts: Mutex<Vec<SteerScript>>,
    transfer_script: Mutex<TransferScript>,
    withdraw_script: Mutex<WithdrawScript>,
    play_script: Mutex<PlayScript>,
    prepare_script: Mutex<PrepareScript>,
    resolve_script: Mutex<ResolveScript>,
    accept_script: Mutex<AcceptScript>,
    load_script: Mutex<LoadScript>,
    /// The sender that says the held load started, and the receiver that
    /// lets it finish.
    load_gate: Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
    /// The gate a held watch waits behind before it answers.
    watch_gate: Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
    /// Whether giving the sound back is refused.
    sound_release_fails: AtomicBool,
    /// The answer each holding check gives; the last entry repeats.
    holding_script: Mutex<Vec<bool>>,
    /// The requests the fake registry still holds, with the deadline each
    /// was given. One leaves when it is answered.
    pending: Mutex<Vec<(RequestId, Instant)>>,
    /// What the accept was handed, so a fallback can be asserted.
    last_resolution: Mutex<Option<Resolution>>,
    /// Every call that moved the account's view of this device, in order.
    calls: Mutex<Vec<Call>>,
    /// Whether the session is known to be dead.
    down: AtomicBool,
    /// How retiring is scripted.
    retire_script: Mutex<RetireScript>,
    retire_started: Mutex<Option<Sender<()>>>,
    /// The connect count at each retirement, so the retire-before-connect
    /// order is visible.
    retires_at_connect: Mutex<Vec<usize>>,
    /// The token the last connect was handed.
    last_connect_token: Mutex<Option<String>>,
    /// Every request the loop refused, in order.
    refusals: Mutex<Vec<RequestId>>,
    connected: AtomicBool,
    connects: AtomicUsize,
    rootlists: AtomicUsize,
    tracks: AtomicUsize,
    albums: AtomicUsize,
    shows: AtomicUsize,
    followed_lists: AtomicUsize,
    followed_artist_lists: AtomicUsize,
    saved_album_lists: AtomicUsize,
    liked_lists: AtomicUsize,
    like_writes: AtomicUsize,
    artists: AtomicUsize,
    lyrics_loads: AtomicUsize,
    searches: AtomicUsize,
    /// Every query asked for, in order, so an echo can be compared.
    searched: Mutex<Vec<String>>,
    portrait_reads: AtomicUsize,
    /// Every artist list asked for, in order.
    portraited: Mutex<Vec<Vec<String>>>,
    releases: AtomicUsize,
    follow_writes: AtomicUsize,
    move_writes: AtomicUsize,
    remove_writes: AtomicUsize,
    rootlist_move_writes: AtomicUsize,
    remove_playlist_writes: AtomicUsize,
    remove_saved_writes: AtomicUsize,
    add_writes: AtomicUsize,
    rename_writes: AtomicUsize,
    create_writes: AtomicUsize,
    link_writes: AtomicUsize,
    state_reads: AtomicUsize,
    artist_started: Mutex<Option<Sender<()>>>,
    lyrics_started: Mutex<Option<Sender<()>>>,
    search_started: Mutex<Option<Sender<()>>>,
    portrait_started: Mutex<Option<Sender<()>>>,
    show_started: Mutex<Option<Sender<()>>>,

    last_follow: Mutex<Option<(String, bool)>>,
    last_move: Mutex<Option<(String, Revision, Vec<Move>)>>,
    last_remove: Mutex<Option<(String, Revision, Vec<Removal>)>>,
    last_root_move: Mutex<Option<(Revision, usize, usize)>>,
    last_playlist_remove: Mutex<Option<(Revision, usize)>>,
    last_saved_remove: Mutex<Option<(String, bool)>>,
    last_add: Mutex<Option<AddWrite>>,
    last_rename: Mutex<Option<RenameWrite>>,
    last_create: Mutex<Option<String>>,
    last_link: Mutex<Option<(Revision, String)>>,
    last_like: Mutex<Option<(Vec<String>, bool)>>,
    move_started: Mutex<Option<Sender<()>>>,
    rootlist_move_started: Mutex<Option<Sender<()>>>,
    remove_playlist_started: Mutex<Option<Sender<()>>>,
    remove_saved_started: Mutex<Option<Sender<()>>>,
    probes: AtomicUsize,
    last_probe: Mutex<Option<String>>,
    probe_answer: Mutex<Option<bool>>,
    probe_started: Mutex<Option<Sender<()>>>,
    add_started: Mutex<Option<Sender<()>>>,
    rename_started: Mutex<Option<Sender<()>>>,
    create_started: Mutex<Option<Sender<()>>>,
    link_started: Mutex<Option<Sender<()>>>,
    state_started: Mutex<Option<Sender<()>>>,
    like_started: Mutex<Option<Sender<()>>>,
    plays: AtomicUsize,
    pauses: AtomicUsize,
    resumes: AtomicUsize,
    stops: AtomicUsize,
    seeks: AtomicUsize,
    last_seek: AtomicUsize,
    volumes: AtomicUsize,
    last_volume: AtomicUsize,
    /// The level the fake player was built at, so a test can tell a level
    /// that arrived before the player existed from one that arrived after.
    level_at_player_start: AtomicUsize,
    players: AtomicUsize,
    player_started: AtomicBool,
    shutdowns: AtomicUsize,
    watches: AtomicUsize,
    refreshes: AtomicUsize,
    /// Every steer asked for, in order, so the target and the action can be
    /// compared.
    steers: Mutex<Vec<(String, CoreSteer)>>,
    /// Every hand-over asked for, in order.
    transfers: Mutex<Vec<String>>,
    withdrawals: AtomicUsize,
    /// The calls the exit tail made, in order, so a withdraw can be seen to
    /// precede the shutdown.
    exit_calls: Mutex<Vec<&'static str>>,
    device_tx: Mutex<Option<UnboundedSender<ClusterEvent>>>,
    device_rx: Mutex<Option<UnboundedReceiver<ClusterEvent>>>,
    inbound_tx: Mutex<Option<UnboundedSender<Inbound>>>,
    inbound_rx: Mutex<Option<UnboundedReceiver<Inbound>>>,
    watch_started: Mutex<Option<Sender<()>>>,
    refresh_started: Mutex<Option<Sender<()>>>,
    steer_started: Mutex<Option<Sender<()>>>,
    transfer_started: Mutex<Option<Sender<()>>>,
    withdraw_started: Mutex<Option<Sender<()>>>,
    connect_started: Mutex<Option<Sender<()>>>,
    rootlist_started: Mutex<Option<Sender<()>>>,
    tracks_started: Mutex<Option<Sender<()>>>,
    album_started: Mutex<Option<Sender<()>>>,
    play_started: Mutex<Option<Sender<()>>>,
    events: Mutex<Option<UnboundedReceiver<PlaybackEvent>>>,
    event_tx: Mutex<Option<UnboundedSender<PlaybackEvent>>>,
    /// How many times the one event stream was handed to a drain.
    event_takes: AtomicUsize,
}

/// Records every session call so an exit path can be asserted without a
/// network or an audio device.
#[derive(Clone)]
struct FakeHost {
    shared: Arc<FakeState>,
}

impl FakeHost {
    fn new(connect: ConnectScript, rootlist: RootlistScript) -> Self {
        Self::with_connect_scripts(vec![connect], rootlist)
    }

    /// One script per `connect` call; the last one repeats once the queue
    /// runs out, so a single-entry queue behaves like a fixed script.
    fn with_connect_scripts(connect: Vec<ConnectScript>, rootlist: RootlistScript) -> Self {
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

/// The album the shell tests open; one name, so a URI and its detail agree.
const ALBUM: &str = "spotify:album:a";

/// The show the shell tests open.
const SHOW: &str = "spotify:show:a";

/// The artist the shell tests open.
const ARTIST: &str = "spotify:artist:a";

/// The track the lyrics tests open.
const TRACK: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ1";

/// The playlist the landing tests hand over and play from.
const PLAYLIST: &str = "spotify:playlist:p";

/// A row the context read answers with, after [`TRACK`].
const SECOND: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ2";

/// A local file the phone can hold in its queue and this window cannot fetch.
const LOCAL: &str = "spotify:local:artist:album:title:200";

/// The landing the fake accept answers with unless a test scripts its own.
fn default_landing() -> Landing {
    Landing {
        rows: vec![TRACK.to_owned()],
        index: 0,
        position_ms: 0,
        playing: true,
        context: None,
        shuffle: Some(false),
        repeat_context: Some(false),
        repeat_track: Some(false),
        epoch: 1,
    }
}

/// Scripts the answers the landing's holding checks get; the last repeats.
fn hold_with(shared: &FakeState, script: &[bool]) {
    *shared.holding_script.lock().expect("lock") = script.to_vec();
}

/// The deadline the fake registry gives each request, standing in for the
/// answer budget the real one runs on.
const ORDER_BUDGET: Duration = Duration::from_secs(5);

/// Registers one request in the fake registry, so an order finds it pending,
/// and answers with the deadline its budget ends it at.
fn hold_request(shared: &FakeState, id: RequestId) -> Instant {
    let deadline = Instant::now() + ORDER_BUDGET;
    shared.pending.lock().expect("lock").push((id, deadline));
    deadline
}

/// Makes the next watch wait for the returned sender before it answers.
fn hold_watch(shared: &FakeState) -> tokio::sync::oneshot::Sender<()> {
    let (release, gate) = tokio::sync::oneshot::channel();
    *shared.watch_gate.lock().expect("lock") = Some(gate);
    release
}

/// Reads updates until the one naming `id`, which an order sends to the
/// window; anything else the watch sends on its own is passed over.
fn order_update(ui_end: &UiEnd, id: RequestId) -> (Order, Instant) {
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(2) {
        match ui_end.updates.recv_timeout(Duration::from_millis(100)) {
            Ok(Update::Remote(RemoteUpdate::Order {
                id: named,
                order,
                expires,
            })) if named == id => return (order, expires),
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => panic!("the worker went away"),
        }
    }
    panic!("timed out waiting for the order update");
}

/// A decoded transfer, built from the wire shape the request loop reads.
fn transfer_instruction(current: &str, context: Option<&str>, pages: &[&str]) -> Instruction {
    use librespot::core::dealer::protocol::{
        Command as WireCommand, LoggingParams, Request, TransferCommand, TransferOptions,
    };
    use librespot::protocol::context::Context;
    use librespot::protocol::context_page::ContextPage;
    use librespot::protocol::context_track::ContextTrack;
    use librespot::protocol::playback::Playback as WirePlayback;
    use librespot::protocol::session::Session as WireSession;
    use librespot::protocol::transfer_state::TransferState;
    use takofy_core::instruction::instruction_of;

    let page_of = |uris: &[&str]| ContextPage {
        tracks: uris
            .iter()
            .map(|uri| ContextTrack {
                uri: Some((*uri).to_owned()),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    let playback = WirePlayback {
        current_track: Some(ContextTrack {
            uri: Some(current.to_owned()),
            ..Default::default()
        })
        .into(),
        position_as_of_timestamp: Some(5_000),
        timestamp: Some(1_000_000),
        is_paused: Some(false),
        ..Default::default()
    };
    let session = WireSession {
        context: context
            .map(|uri| Context {
                uri: Some(uri.to_owned()),
                pages: vec![page_of(pages)],
                ..Default::default()
            })
            .into(),
        ..Default::default()
    };
    let data = TransferState {
        playback: Some(playback).into(),
        current_session: Some(session).into(),
        ..Default::default()
    };
    let request = Request {
        message_id: 1,
        sent_by_device_id: "phone".to_owned(),
        command: WireCommand::Transfer(TransferCommand {
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
    instruction_of(&request)
}

/// One phone-side play, decoded the way the receiver decodes it.
///
/// The row it starts on comes from `skip`; a play whose pages are empty is
/// read from its context instead.
fn play_instruction(
    context: &str,
    pages: &[&str],
    skip: Option<librespot::core::dealer::protocol::SkipTo>,
) -> Instruction {
    use librespot::core::dealer::protocol::{
        Command as WireCommand, LoggingParams, PlayCommand, PlayOptions, Request,
    };
    use librespot::protocol::context::Context;
    use librespot::protocol::context_page::ContextPage;
    use librespot::protocol::context_track::ContextTrack;
    use librespot::protocol::player::PlayOrigin;
    use takofy_core::instruction::instruction_of;

    let page = ContextPage {
        tracks: pages
            .iter()
            .map(|uri| ContextTrack {
                uri: Some((*uri).to_owned()),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    let request = Request {
        message_id: 1,
        sent_by_device_id: "phone".to_owned(),
        command: WireCommand::Play(Box::new(PlayCommand {
            context: Context {
                uri: Some(context.to_owned()),
                pages: if pages.is_empty() {
                    Vec::new()
                } else {
                    vec![page]
                },
                ..Default::default()
            },
            play_origin: PlayOrigin::default(),
            options: PlayOptions {
                skip_to: skip,
                player_options_override: None,
                license: None,
                seek_to: None,
                always_play_something: None,
                audio_stream: None,
                initially_paused: None,
                prefetch_level: None,
                system_initiated: None,
            },
            logging_params: LoggingParams {
                interaction_ids: None,
                device_identifier: None,
                command_initiated_time: None,
                page_instance_ids: None,
                command_id: None,
            },
        })),
    };
    instruction_of(&request)
}

/// One artist page's data, with no top tracks and no releases, so a test
/// names only what it reads.
fn artist_detail(uri: &str) -> ArtistDetail {
    ArtistDetail {
        uri: uri.to_owned(),
        name: "Vela Moreno".to_owned(),
        portrait: None,
        top_tracks: Vec::new(),
        releases: Vec::new(),
        related: Vec::new(),
    }
}

/// One track's lyrics with a single timed line, so a test names only what
/// it reads.
fn lyrics_of(words: &str) -> TrackLyrics {
    TrackLyrics {
        lines: vec![LyricLine {
            start_ms: Some(0),
            end_ms: Some(1_000),
            words: words.to_owned(),
        }],
        synced: true,
    }
}

/// One album page's data, with no tracks, so a test names only what it reads.
fn album_detail(uri: &str) -> AlbumDetail {
    AlbumDetail {
        uri: uri.to_owned(),
        name: "Record".to_owned(),
        artists: "Band".to_owned(),
        artist_uri: None,
        year: Some(1997),
        cover: None,
        tracks: Vec::new(),
    }
}

/// One show page's data, with one episode, so a test names only what it
/// reads.
fn show_detail(uri: &str) -> ShowDetail {
    ShowDetail {
        show: Show {
            uri: uri.to_owned(),
            name: "Lateral".to_owned(),
            publisher: "Lateral".to_owned(),
            description: String::new(),
            cover: None,
        },
        episodes: vec![Episode {
            uri: "spotify:episode:e".to_owned(),
            title: "An episode".to_owned(),
            show: "Lateral".to_owned(),
            show_uri: Some(uri.to_owned()),
            description: None,
            published: None,
            duration_ms: None,
            cover: None,
            named: true,
        }],
    }
}

/// One followed-show card, named after its URI so a test can tell two apart.
fn followed_show(uri: &str) -> FollowedShow {
    FollowedShow {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        publisher: None,
        cover: None,
        episodes: None,
    }
}

/// One followed-artist card, named after its URI so a test can tell two
/// apart.
fn followed_artist(uri: &str) -> FollowedArtist {
    FollowedArtist {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        portrait: None,
        releases: None,
    }
}

/// One saved-album card, named after its URI so a test can tell two apart.
fn saved_album(uri: &str) -> SavedAlbum {
    SavedAlbum {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        artists: String::new(),
        year: None,
        cover: None,
        tracks: None,
    }
}

fn playlist(uri: &str, name: &str) -> PlaylistEntry {
    PlaylistEntry {
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

fn revision(byte: u8) -> Revision {
    Revision::from_wire(&[byte]).expect("a revision")
}

fn expect_updates(ui_end: &UiEnd, count: usize) -> Vec<Update> {
    (0..count)
        .map(|_| ui_end.updates.recv().expect("update"))
        .collect()
}

/// Reads the next `count` updates, failing instead of hanging when one the
/// window owes never comes.
fn expect_updates_within(ui_end: &UiEnd, count: usize) -> Vec<Update> {
    (0..count)
        .map(|_| {
            ui_end
                .updates
                .recv_timeout(Duration::from_secs(2))
                .expect("update")
        })
        .collect()
}

fn track(uri: &str) -> TrackEntry {
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

fn report(uri: &str) -> Report {
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

/// The other device every scripted device answer names.
const PHONE: &str = "phone";

fn phone() -> Device {
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

fn tablet() -> Device {
    Device {
        id: "tablet".to_owned(),
        name: "Tablet".to_owned(),
        kind: DeviceKind::Tablet,
    }
}

fn phone_view() -> ClusterView {
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

/// Everything already queued, without waiting for more.
fn queued(ui_end: &UiEnd) -> Vec<Update> {
    let mut seen = Vec::new();
    while let Ok(update) = ui_end.updates.try_recv() {
        seen.push(update);
    }
    seen
}

/// The event sender of the current fake player.
fn events(shared: &FakeState) -> UnboundedSender<PlaybackEvent> {
    shared
        .event_tx
        .lock()
        .expect("lock")
        .clone()
        .expect("the player has an event stream")
}

/// Asserts the worker sent nothing within a wait long enough to notice.
fn expect_no_update(ui_end: &UiEnd) {
    match ui_end.updates.recv_timeout(Duration::from_millis(100)) {
        Err(RecvTimeoutError::Timeout) => {}
        Err(RecvTimeoutError::Disconnected) => panic!("the worker went away"),
        Ok(update) => panic!("expected no update, got {update:?}"),
    }
}

/// Waits until `check` holds, so no test has to guess at a delay.
fn until(what: &str, mut check: impl FnMut() -> bool) {
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
fn calls(shared: &FakeState) -> Vec<Call> {
    shared.calls.lock().expect("lock").clone()
}

/// Every request the loop refused, in order.
fn refusals(shared: &FakeState) -> Vec<RequestId> {
    shared.refusals.lock().expect("lock").clone()
}

/// How many times one inbound was answered, either way.
fn answers(shared: &FakeState, id: RequestId) -> usize {
    calls(shared)
        .iter()
        .filter(|call| {
            matches!(call, Call::Refuse(answered) if *answered == id)
                || matches!(call, Call::Answer(answered, _) if *answered == id)
        })
        .count()
}

/// The request stream a scripted watch opened, so a test can push into it.
fn inbound_sender(shared: &FakeState) -> UnboundedSender<Inbound> {
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
fn watch_now(ui_end: &UiEnd, shared: &FakeState) {
    let (tx, rx) = channel();
    *shared.watch_started.lock().expect("lock") = Some(tx);
    ui_end.commands.send(Command::WatchDevices).expect("watch");
    rx.recv().expect("watch started");
    shared.calls.lock().expect("lock").clear();
}

fn device_roster() -> Update {
    Update::Devices(DeviceUpdate::Roster {
        devices: vec![phone()],
        holder: Some(PHONE.to_owned()),
        held_here: false,
    })
}

fn device_playing() -> Update {
    Update::Devices(DeviceUpdate::Playing(Some(remote_playback())))
}

fn inbound(shared: &FakeState, id: u64, kind: Kind, instruction: Instruction) {
    inbound_sender(shared)
        .send(Inbound {
            id: RequestId(id),
            kind,
            instruction,
        })
        .expect("inbound");
}

mod announce;
mod covers;
mod create;
mod devices;
mod errors;
mod follow;
mod orders;
mod playback;
mod reads;
mod recovery;
mod rename;
mod rootlist;
mod session;
mod track_page;
mod transfer;
