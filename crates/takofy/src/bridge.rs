//! The rendezvous between the window and the sign-in worker.
//!
//! The window owns only what it draws. The worker owns the blocking sign-in
//! calls, and teardown finishes it before the runtime is dropped. Sign-in work
//! runs on plain threads rather than the runtime: a blocking call parked on a
//! browser callback must not hold up runtime shutdown, and a detached thread
//! dies with the process.

use std::collections::HashSet;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use takofy_core::connect::{ClusterEvent, Steer as CoreSteer, WITHDRAW_TIMEOUT};
use takofy_core::credentials::{AccessToken, CredentialStore};
use takofy_core::instruction::{Instruction, Kind};
use takofy_core::login::{AuthError, SourceErrorKind, TokenSource, authorize, resume};
use takofy_core::playback::{PlaybackError, PlaybackEvent};
use takofy_core::receive::Inbound;
use takofy_core::{ORDER_MARGIN, Resolution};
use takofy_model::{CoverUrl, Move, Order, Owner, PlaylistEntry, Removal, RequestId, Revision};
use takofy_ui::message::{
    Command, DeviceUpdate, Failure, RemoteUpdate, Steer, TransportUpdate, Update,
};
use tokio::runtime::{Handle, Runtime};
use tokio::sync::Semaphore;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;
use tokio::sync::oneshot::Receiver as StopReceiver;
use tokio::task::JoinSet;

use crate::cover::{FETCH_TIMEOUT, MAX_CONCURRENT_COVERS, fetch_cover, spawn_cover};
use crate::devices::SentRoster;
use crate::session::{SessionError, SessionHost, stop_or};

/// A handle the worker can wake to ask for the next frame.
pub(crate) trait Waker: Send + Sync + 'static {
    fn wake(&self);
}

impl Waker for egui::Context {
    fn wake(&self) {
        self.request_repaint();
    }
}

/// Holds the waker that exists only once the window does.
///
/// A resume result can arrive before the window is created. It waits in the
/// update channel, and the first frame drains it.
#[derive(Clone, Default)]
pub(crate) struct WakeSlot(Arc<OnceLock<Arc<dyn Waker>>>);

impl WakeSlot {
    /// Installs the waker. The first install wins; a later one is ignored.
    pub(crate) fn install(&self, waker: Arc<dyn Waker>) {
        let _ = self.0.set(waker);
    }

    fn wake(&self) {
        if let Some(waker) = self.0.get() {
            waker.wake();
        }
    }
}

/// Queues an update, then wakes the window.
#[derive(Clone)]
pub(crate) struct Notifier {
    tx: Sender<Update>,
    slot: WakeSlot,
}

impl Notifier {
    /// Queues an update, then wakes the window.
    ///
    /// `false` means the window is gone and the update reached nobody, which a
    /// caller that owes the phone an answer has to know.
    pub(crate) fn send(&self, update: Update) -> bool {
        // The update must be readable before the waker runs: a wake that beats
        // the queue into the channel would be spent on a frame with nothing to
        // drain.
        if self.tx.send(update).is_err() {
            return false;
        }
        self.slot.wake();
        true
    }
}

/// The window's end: updates in, commands out.
pub(crate) struct UiEnd {
    pub updates: Receiver<Update>,
    pub commands: UnboundedSender<Command>,
}

/// The worker's end: a stop signal and the task that owns the command loop.
pub(crate) struct Worker {
    stop: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

impl Worker {
    /// Signals the command loop and waits for it to end.
    pub(crate) async fn finish(self) {
        let _ = self.stop.send(());
        if let Err(error) = self.task.await {
            // A panic message can be third-party text. The default hook has
            // already printed it, so only the kind is worth logging.
            let reason = if error.is_panic() {
                "panicked"
            } else if error.is_cancelled() {
                "cancelled"
            } else {
                "ended abnormally"
            };
            log::error!("session task {reason}");
        }
    }
}

/// What the task received from the select, once the borrow on `stop` is gone.
enum Event {
    /// An access token, or `None` once every sender has gone.
    Token(Option<AccessToken>),
    /// A command, or `None` once the window has gone.
    Command(Option<Command>),
    /// A request from the account, or `None` once that stream has ended.
    Inbound(Option<Inbound>),
}

/// Whether a host call finished or was abandoned because the window closed.
enum Step {
    Done,
    Stopped,
}

impl Step {
    fn stopped(&self) -> bool {
        matches!(self, Self::Stopped)
    }
}

/// State the device watch keeps across commands: whether it was started, the
/// request stream it handed over, the roster the window was told, and whether
/// the pump has ended.
struct Devices {
    watching: bool,
    inbound: Option<UnboundedReceiver<Inbound>>,
    sent: SentRoster,
    dead: Arc<AtomicBool>,
}

impl Default for Devices {
    fn default() -> Self {
        Self {
            watching: false,
            inbound: None,
            sent: SentRoster::default(),
            dead: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Waits for the account's next request, forgetting a stream that has ended.
///
/// A stream that closed leaves no receiver behind, so this arm never resolves
/// again; leaving it armed on a dead channel would spin the loop. Cancelling the
/// await loses nothing, because the receiver is polled in place rather than
/// rebuilt.
async fn next_inbound(slot: &mut Option<UnboundedReceiver<Inbound>>) -> Option<Inbound> {
    let Some(receiver) = slot else {
        return std::future::pending().await;
    };
    let inbound = receiver.recv().await;
    if inbound.is_none() {
        *slot = None;
    }
    inbound
}

/// Removes finished fetch tasks so their cells do not accumulate over a
/// session, and logs a panic that would otherwise go unobserved.
fn reap_finished_covers(tasks: &mut JoinSet<()>) {
    while let Some(joined) = tasks.try_join_next() {
        if let Err(error) = joined {
            log::warn!("cover task ended: {error}");
        }
    }
}

/// Answers one cover request. Every request ends in exactly one update: the
/// fetch is spawned, or a failure is sent straight back, so the window's
/// in-flight entry always resolves.
fn handle_fetch(
    connected: bool,
    client: Option<&reqwest::Client>,
    tasks: &mut JoinSet<()>,
    permits: Arc<Semaphore>,
    in_flight: Arc<Mutex<HashSet<CoverUrl>>>,
    url: CoverUrl,
    notifier: Notifier,
) {
    let (true, Some(client)) = (connected, client) else {
        // A signed-out window has no cover to want, and a client that failed
        // to build can never serve one; both resolve the request now.
        notifier.send(Update::CoverFailed { url });
        return;
    };
    let client = client.clone();
    let requested = url.clone();
    let fetch = async move { fetch_cover(&client, &requested).await };
    spawn_cover(
        tasks,
        permits,
        in_flight,
        url,
        fetch,
        |bytes| crate::decode::decode(&bytes),
        notifier,
    );
}

/// Starts the sign-in worker and kicks off the resume attempt immediately, so
/// it overlaps window creation.
pub(crate) fn start<H: SessionHost, S: TokenSource + Send + Sync + 'static>(
    handle: &Handle,
    host: H,
    store: Arc<dyn CredentialStore>,
    source: Arc<S>,
) -> (UiEnd, WakeSlot, Worker) {
    start_with(handle, host, store, source, WITHDRAW_TIMEOUT)
}

/// Starts the worker with an explicit outer withdraw bound.
fn start_with<H: SessionHost, S: TokenSource + Send + Sync + 'static>(
    handle: &Handle,
    host: H,
    store: Arc<dyn CredentialStore>,
    source: Arc<S>,
    withdraw_timeout: Duration,
) -> (UiEnd, WakeSlot, Worker) {
    let (updates_tx, updates_rx) = channel();
    let (commands_tx, mut commands_rx) = tokio::sync::mpsc::unbounded_channel();
    let (tokens_tx, mut tokens_rx) = tokio::sync::mpsc::unbounded_channel();
    let (stop_tx, mut stop_rx) = oneshot::channel();
    let slot = WakeSlot::default();
    let notifier = Notifier {
        tx: updates_tx,
        slot: slot.clone(),
    };

    {
        let notifier = notifier.clone();
        let tokens = tokens_tx.clone();
        let store = Arc::clone(&store);
        let source = Arc::clone(&source);
        std::thread::spawn(move || {
            let result = resume(store.as_ref(), source.as_ref());
            if let Ok(Some(token)) = &result
                && tokens.send(token.clone()).is_err()
            {
                return;
            }
            send_update(&notifier, resume_update(result));
        });
    }

    let task_notifier = notifier.clone();
    let task = handle.spawn(async move {
        let mut host = host;
        let mut connected = false;
        let mut devices = Devices::default();
        let mut token: Option<AccessToken> = None;
        // Built once: the timeout, the redirect ban, and the bounded pool live
        // on the client, and covers are the only thing it is ever used for.
        // Redirects are refused so an allowlisted host cannot hand the fetch to
        // a host the allowlist never approved; a 3xx becomes a fetch error.
        let cover_client = match reqwest::Client::builder()
            .timeout(FETCH_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .pool_max_idle_per_host(2)
            .build()
        {
            Ok(client) => Some(client),
            Err(error) => {
                // Without a client no fetch can be served; the request handler
                // answers each one instead of leaving it hanging.
                log::warn!("cover client could not be built: {error}");
                None
            }
        };
        let cover_permits = Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS));
        let cover_in_flight: Arc<Mutex<HashSet<CoverUrl>>> = Arc::new(Mutex::new(HashSet::new()));
        let mut cover_tasks = JoinSet::new();
        loop {
            reap_finished_covers(&mut cover_tasks);
            let event = tokio::select! {
                biased;
                _ = &mut stop_rx => break,
                received = tokens_rx.recv() => Event::Token(received),
                command = commands_rx.recv() => Event::Command(command),
                request = next_inbound(&mut devices.inbound) => Event::Inbound(request),
            };
            match event {
                Event::Token(Some(received)) => {
                    if connected {
                        // The first token opened the session; a second cannot
                        // improve it.
                        continue;
                    }
                    token = Some(received);
                    let step = load_library(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        token.as_ref(),
                        &mut connected,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Token(None) => {}
                Event::Command(Some(Command::Authorize)) => {
                    spawn_authorize(
                        task_notifier.clone(),
                        tokens_tx.clone(),
                        Arc::clone(&store),
                        Arc::clone(&source),
                    );
                }
                Event::Command(Some(Command::ReloadLibrary)) => {
                    let step = load_library(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        token.as_ref(),
                        &mut connected,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadTracks { playlist })) => {
                    let step =
                        load_tracks(&mut host, &mut stop_rx, &task_notifier, &playlist).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadAlbum { album })) => {
                    let step = load_album(&mut host, &mut stop_rx, &task_notifier, &album).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadArtist { artist })) => {
                    let step = load_artist(&mut host, &mut stop_rx, &task_notifier, &artist).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadRelated { artist, uris })) => {
                    let step =
                        load_related(&mut host, &mut stop_rx, &task_notifier, &artist, uris).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadLyrics { track })) => {
                    let step = load_lyrics(&mut host, &mut stop_rx, &task_notifier, &track).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadShow { show })) => {
                    let step = load_show(&mut host, &mut stop_rx, &task_notifier, &show).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadFollowedShows)) => {
                    let step = load_followed_shows(&mut host, &mut stop_rx, &task_notifier).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadFollowedArtists)) => {
                    let step = load_followed_artists(&mut host, &mut stop_rx, &task_notifier).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadSavedAlbums)) => {
                    let step = load_saved_albums(&mut host, &mut stop_rx, &task_notifier).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadLikedTracks)) => {
                    let step = load_liked_tracks(&mut host, &mut stop_rx, &task_notifier).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::SetLiked { tracks, liked })) => {
                    let step =
                        set_liked(&mut host, &mut stop_rx, &task_notifier, &tracks, liked).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::Search { query })) => {
                    let step = load_search(&mut host, &mut stop_rx, &task_notifier, &query).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadPortraits { query, artists })) => {
                    let step =
                        load_portraits(&mut host, &mut stop_rx, &task_notifier, &query, &artists)
                            .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadReleaseTracks { release, tracks })) => {
                    let step = load_release_tracks(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &release,
                        &tracks,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::SetFollow { artist, followed })) => {
                    let step =
                        set_follow(&mut host, &mut stop_rx, &task_notifier, &artist, followed)
                            .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::MoveTrack {
                    playlist,
                    revision,
                    moves,
                })) => {
                    let step = move_track(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &playlist,
                        &revision,
                        &moves,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::RemoveTracks {
                    playlist,
                    revision,
                    removals,
                })) => {
                    let step = remove_tracks(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &playlist,
                        &revision,
                        &removals,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::MovePlaylist { revision, from, to })) => {
                    let step =
                        move_playlist(&mut host, &mut stop_rx, &task_notifier, &revision, from, to)
                            .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::RemovePlaylist {
                    playlist,
                    revision,
                    index,
                })) => {
                    let step = remove_playlist(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &playlist,
                        &revision,
                        index,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LinkPlaylist { playlist, revision })) => {
                    let step = link_playlist(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &playlist,
                        &revision,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadSaved { uri })) => {
                    let step = load_saved(&mut host, &mut stop_rx, &task_notifier, &uri).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::SetSaved { uri, saved })) => {
                    let step =
                        set_saved(&mut host, &mut stop_rx, &task_notifier, &uri, saved).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::AddTracks {
                    playlist,
                    revision,
                    from_index,
                    tracks,
                })) => {
                    let step = add_tracks(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &playlist,
                        &revision,
                        from_index,
                        &tracks,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::RenamePlaylist { playlist, name })) => {
                    let step =
                        rename_playlist(&mut host, &mut stop_rx, &task_notifier, &playlist, &name)
                            .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::CreatePlaylist {
                    name,
                    revision,
                    tracks,
                })) => {
                    let step = create_playlist(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &name,
                        &revision,
                        &tracks,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::WatchDevices)) => {
                    let step = watch_devices(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &mut devices,
                        connected,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::RefreshDevices)) => {
                    let step = refresh_devices(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &mut devices,
                        connected,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::Steer { device, steer })) => {
                    let step = steer_device(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &device,
                        core_steer(steer),
                        connected,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::Transfer { to })) => {
                    let step =
                        transfer_device(&mut host, &mut stop_rx, &task_notifier, &to, connected)
                            .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LabelRemote { track })) => {
                    let step = label_remote(&mut host, &mut stop_rx, &task_notifier, &track).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LabelRows { uris })) => {
                    let step = label_rows(&mut host, &mut stop_rx, &task_notifier, &uris).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::Play { uri })) => {
                    play(&mut host, &task_notifier, &uri);
                }
                // The window applied an order, or could not. An entry the budget
                // already answered is gone: the phone has been told `Failure`,
                // and the next report put corrects what it shows.
                Event::Command(Some(Command::Answer { id, ok })) => {
                    if host.answer_order(id, ok) {
                        log::info!(target: LANDING_LOG_TARGET, "answer delivered");
                    } else {
                        log::info!(
                            target: LANDING_LOG_TARGET,
                            "answer found the entry gone"
                        );
                    }
                }
                // A report only has a wire to reach while the device is on the
                // account; before that the engine's own slot is not announced.
                Event::Command(Some(Command::Report(report))) => {
                    if devices.watching {
                        host.report(report);
                    }
                }
                Event::Command(Some(Command::Pause)) => host.pause(),
                Event::Command(Some(Command::Resume)) => host.resume(),
                Event::Command(Some(Command::Stop)) => host.stop(),
                Event::Command(Some(Command::Seek { position_ms })) => host.seek(position_ms),
                Event::Command(Some(Command::Volume { percent })) => host.set_volume(percent),
                // The arm only spawns: the fetch must not park the command loop
                // that also awaits host calls. A signed-out window has no cover
                // to want, and every request is answered either way.
                Event::Command(Some(Command::Fetch { url })) => {
                    handle_fetch(
                        connected,
                        cover_client.as_ref(),
                        &mut cover_tasks,
                        Arc::clone(&cover_permits),
                        Arc::clone(&cover_in_flight),
                        url,
                        task_notifier.clone(),
                    );
                }
                Event::Command(None) => break,
                // Every request is answered. A hand-over is served in full, a
                // transport order is handed to the window that owns the sound,
                // and the rest are refused at once, so the phone is never left
                // waiting out its budget.
                Event::Inbound(Some(request)) => {
                    log::info!("connect inbound: {}", request.kind.word());
                    let step = match request.instruction {
                        Instruction::Order(order) => {
                            serve_order(&host, &task_notifier, request.id, request.kind, order)
                        }
                        instruction => {
                            serve_landing(
                                &mut host,
                                &mut stop_rx,
                                &task_notifier,
                                request.id,
                                instruction,
                            )
                            .await
                        }
                    };
                    if step.stopped() {
                        break;
                    }
                }
                Event::Inbound(None) => {}
            }
        }
        // Cancels every in-flight fetch. A detached decode thread is left to
        // finish and its result is dropped with the oneshot receiver.
        cover_tasks.abort_all();
        // The device goes before the player does, so a forced quit never leaves
        // the account with an entry this client will not answer on.
        if devices.watching {
            withdraw(&mut host, withdraw_timeout).await;
        }
        let _ = host.shutdown().await;
    });

    (
        UiEnd {
            updates: updates_rx,
            commands: commands_tx,
        },
        slot,
        Worker {
            stop: stop_tx,
            task,
        },
    )
}

/// Runs the authorize flow off the runtime and hands the task its token.
fn spawn_authorize<S: TokenSource + Send + Sync + 'static>(
    notifier: Notifier,
    tokens: UnboundedSender<AccessToken>,
    store: Arc<dyn CredentialStore>,
    source: Arc<S>,
) {
    std::thread::spawn(move || {
        let result = authorize(store.as_ref(), source.as_ref());
        if let Ok(token) = &result
            && tokens.send(token.clone()).is_err()
        {
            return;
        }
        send_update(&notifier, authorize_update(result));
    });
}

/// Connects if needed and lists the rootlist, announcing the outcome.
///
/// `Stopped` means the window closed while the call was in flight, so the
/// caller must leave the loop and run the shutdown.
async fn load_library<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    token: Option<&AccessToken>,
    connected: &mut bool,
) -> Step {
    if !*connected {
        let Some(token) = token else {
            // A reload with nothing to connect with; a retry can only work once
            // sign-in has delivered a token.
            notifier.send(Update::PlaylistFailed(library_failure(
                &SessionError::NotConnected,
            )));
            return Step::Done;
        };
        match stop_or(stop, host.connect(token)).await {
            None => return Step::Stopped,
            Some(Ok(())) => *connected = true,
            Some(Err(error)) => {
                notifier.send(Update::PlaylistFailed(library_failure(&error)));
                return Step::Done;
            }
        }
    }

    match stop_or(stop, host.rootlist()).await {
        None => Step::Stopped,
        Some(Ok(read)) => {
            notifier.send(Update::Playlists(read.entries));
            // Sent after the list, and only when the read named a revision: the
            // list is not reorderable until its own access arrives.
            if let Some(access) = read.access {
                notifier.send(Update::RootlistAccess(access));
            }
            Step::Done
        }
        Some(Err(error)) => {
            notifier.send(Update::PlaylistFailed(library_failure(&error)));
            Step::Done
        }
    }
}

/// Sends the ready update, or the failure update with its reason collapsed for
/// the window.
pub(crate) fn announce<T>(
    notifier: &Notifier,
    result: Result<T, SessionError>,
    ready: impl FnOnce(T) -> Update,
    failed: impl FnOnce(Failure) -> Update,
) {
    match result {
        Ok(value) => notifier.send(ready(value)),
        Err(error) => notifier.send(failed(library_failure(&error))),
    };
}

/// Loads the first page of a playlist's tracks and announces it.
async fn load_tracks<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    playlist: &str,
) -> Step {
    match stop_or(stop, host.tracks(playlist)).await {
        None => Step::Stopped,
        Some(result) => {
            announce(
                notifier,
                result,
                |page| Update::Tracks {
                    playlist: playlist.to_owned(),
                    tracks: page.tracks,
                    cover: page.cover,
                    access: page.access,
                },
                |failure| Update::TracksFailed {
                    playlist: playlist.to_owned(),
                    failure,
                },
            );
            Step::Done
        }
    }
}

/// Loads one album with its labelled tracks and announces it.
async fn load_album<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    album: &str,
) -> Step {
    match stop_or(stop, host.album(album)).await {
        None => Step::Stopped,
        Some(result) => {
            announce(notifier, result, Update::Album, |failure| {
                Update::AlbumFailed {
                    album: album.to_owned(),
                    failure,
                }
            });
            Step::Done
        }
    }
}

/// Loads one show with its episodes and announces it.
async fn load_show<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    show: &str,
) -> Step {
    match stop_or(stop, host.show(show)).await {
        None => Step::Stopped,
        Some(result) => {
            announce(notifier, result, Update::Show, |failure| {
                Update::ShowFailed {
                    show: show.to_owned(),
                    failure,
                }
            });
            Step::Done
        }
    }
}

/// Loads the shows the user follows and announces the whole list.
///
/// It has its own failure, because the playlists beside it draw fine and
/// "Could not load the library" would be false there.
async fn load_followed_shows<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
) -> Step {
    match stop_or(stop, host.followed_shows()).await {
        None => Step::Stopped,
        Some(Ok(shows)) => {
            notifier.send(Update::FollowedShows(shows));
            Step::Done
        }
        Some(Err(error)) => {
            notifier.send(Update::FollowedShowsFailed(followed_failure(&error)));
            Step::Done
        }
    }
}

/// Loads the artists the user follows and announces the whole list.
///
/// It has its own failure, because the playlists beside it draw fine and
/// "Could not load the library" would be false there.
async fn load_followed_artists<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
) -> Step {
    match stop_or(stop, host.followed_artists()).await {
        None => Step::Stopped,
        Some(Ok(artists)) => {
            notifier.send(Update::FollowedArtists(artists));
            Step::Done
        }
        Some(Err(error)) => {
            notifier.send(Update::FollowedArtistsFailed(artists_failure(&error)));
            Step::Done
        }
    }
}

/// Loads the albums the user saved and announces the whole list.
///
/// It has its own failure, because the playlists beside it draw fine and
/// "Could not load the library" would be false there.
async fn load_saved_albums<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
) -> Step {
    match stop_or(stop, host.saved_albums()).await {
        None => Step::Stopped,
        Some(Ok(albums)) => {
            notifier.send(Update::SavedAlbums(albums));
            Step::Done
        }
        Some(Err(error)) => {
            notifier.send(Update::SavedAlbumsFailed(albums_failure(&error)));
            Step::Done
        }
    }
}

/// Loads the tracks the user has liked and announces the whole set.
///
async fn load_liked_tracks<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
) -> Step {
    match stop_or(stop, host.liked_tracks()).await {
        None => Step::Stopped,
        Some(Ok(tracks)) => {
            notifier.send(Update::LikedTracks(tracks));
            Step::Done
        }
        Some(Err(error)) => {
            notifier.send(Update::LikedTracksFailed(liked_failure(&error)));
            Step::Done
        }
    }
}

/// Writes the liked set and announces what was requested.
///
async fn set_liked<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    tracks: &[String],
    liked: bool,
) -> Step {
    match stop_or(stop, host.set_liked(tracks.to_vec(), liked)).await {
        None => Step::Stopped,
        Some(Ok(())) => {
            notifier.send(Update::Liked {
                tracks: tracks.to_vec(),
                liked,
            });
            Step::Done
        }
        Some(Err(error)) => {
            log::warn!("like write failed: {}", error.kind());
            notifier.send(Update::LikeFailed {
                tracks: tracks.to_vec(),
                failure: Failure::Save,
            });
            Step::Done
        }
    }
}

/// Loads one artist and announces the page, then the followed state when the
/// probe answered.
///
/// The two updates are separate because they are separate facts: the page is
/// drawn from the first, and the Follow label moves only on the second. A probe
/// that did not answer sends nothing, so the label stays where it was.
/// Names the artists one artist is heard with, and announces the cards they make.
async fn load_related<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    artist: &str,
    uris: Vec<String>,
) -> Step {
    match stop_or(stop, host.related(uris)).await {
        None => Step::Stopped,
        Some(Ok(cards)) => {
            notifier.send(Update::Related {
                artist: artist.to_owned(),
                cards,
            });
            Step::Done
        }
        Some(Err(error)) => {
            notifier.send(Update::RelatedFailed {
                artist: artist.to_owned(),
                failure: related_failure(&error),
            });
            Step::Done
        }
    }
}

/// The failure the window shows for a likeness read: its kind reaches the log,
/// nothing else does.
fn related_failure(error: &SessionError) -> Failure {
    log::warn!("the likeness read failed: {}", error.kind());
    Failure::Related
}

async fn load_artist<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    artist: &str,
) -> Step {
    match stop_or(stop, host.artist(artist)).await {
        None => Step::Stopped,
        Some(Ok(page)) => {
            notifier.send(Update::Artist(page.artist));
            if let Some(followed) = page.followed {
                notifier.send(Update::Followed {
                    artist: artist.to_owned(),
                    followed,
                });
            }
            Step::Done
        }
        Some(Err(error)) => {
            notifier.send(Update::ArtistFailed {
                artist: artist.to_owned(),
                failure: library_failure(&error),
            });
            Step::Done
        }
    }
}

/// Loads one track's lyrics and announces them.
///
/// Its failure is `Lyrics`, not `Load`: the page around the pane may draw fine,
/// and the pane's own message names only the lyrics.
async fn load_lyrics<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    track: &str,
) -> Step {
    match stop_or(stop, host.lyrics(track)).await {
        None => Step::Stopped,
        Some(Ok(lyrics)) => {
            notifier.send(Update::Lyrics {
                track: track.to_owned(),
                lyrics,
            });
            Step::Done
        }
        Some(Err(error)) => {
            notifier.send(Update::LyricsFailed {
                track: track.to_owned(),
                failure: lyrics_failure(&error),
            });
            Step::Done
        }
    }
}

/// Loads one search query's tracks and announces them.
///
/// The query is echoed verbatim. The window clears its in-flight marker on an
/// echo that matches the key it sent, so a reworded query here would strand
/// the read.
async fn load_search<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    query: &str,
) -> Step {
    match stop_or(stop, host.search(query)).await {
        None => Step::Stopped,
        Some(Ok(tracks)) => {
            notifier.send(Update::Search {
                query: query.to_owned(),
                tracks,
            });
            Step::Done
        }
        Some(Err(error)) => {
            notifier.send(Update::SearchFailed {
                query: query.to_owned(),
                failure: search_failure(&error),
            });
            Step::Done
        }
    }
}

/// Reads one answer's artist portraits and announces them.
///
/// The query is echoed verbatim, like a search's, because the window drops a
/// reply for a query it has typed past. A read that does not answer is silent:
/// the row keeps the look of a cover that has not arrived.
async fn load_portraits<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    query: &str,
    artists: &[String],
) -> Step {
    match stop_or(stop, host.portraits(artists.to_vec())).await {
        None => Step::Stopped,
        Some(Some(portraits)) => {
            notifier.send(Update::Portraits {
                query: query.to_owned(),
                portraits,
            });
            Step::Done
        }
        Some(None) => {
            notifier.send(Update::PortraitsFailed {
                query: query.to_owned(),
            });
            Step::Done
        }
    }
}

/// Labels one release's tracks and announces them.
async fn load_release_tracks<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    release: &str,
    tracks: &[String],
) -> Step {
    match stop_or(stop, host.release_tracks(tracks)).await {
        None => Step::Stopped,
        Some(Ok(tracks)) => {
            notifier.send(Update::ReleaseTracks {
                release: release.to_owned(),
                tracks,
            });
            Step::Done
        }
        Some(Err(error)) => {
            notifier.send(Update::ReleaseTracksFailed {
                release: release.to_owned(),
                failure: library_failure(&error),
            });
            Step::Done
        }
    }
}

/// Writes the followed state and announces what the server accepted.
///
/// The announced value is the one that was requested, because the write either
/// landed or it did not: there is no partial follow. A failure is `Save`, not
/// `Load`, because the user asked for this and it did not stick.
async fn set_follow<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    artist: &str,
    followed: bool,
) -> Step {
    match stop_or(stop, host.set_follow(artist, followed)).await {
        None => Step::Stopped,
        Some(Ok(())) => {
            notifier.send(Update::Followed {
                artist: artist.to_owned(),
                followed,
            });
            Step::Done
        }
        Some(Err(error)) => {
            log::warn!("follow write failed: {}", error.kind());
            notifier.send(Update::FollowFailed {
                artist: artist.to_owned(),
                failure: Failure::Save,
            });
            Step::Done
        }
    }
}

/// Writes one move and announces the revision its reply names.
///
/// The announced revision is the one the next move must be based on. A failure
/// is `Save`, not `Load`, because the user asked for this and it did not stick.
async fn move_track<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    playlist: &str,
    revision: &Revision,
    moves: &[Move],
) -> Step {
    match stop_or(stop, host.move_track(playlist, revision, moves)).await {
        None => Step::Stopped,
        Some(Ok(revision)) => {
            notifier.send(Update::TrackMoved {
                playlist: playlist.to_owned(),
                revision,
            });
            Step::Done
        }
        Some(Err(error)) => {
            log::warn!("move write failed: {}", error.kind());
            notifier.send(Update::MoveFailed {
                playlist: playlist.to_owned(),
                failure: Failure::Save,
            });
            Step::Done
        }
    }
}

/// Removes a playlist's rows and announces the revision its reply names.
///
/// A failure is `Save`, not `Load`, because the user asked for this and it did
/// not stick.
async fn remove_tracks<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    playlist: &str,
    revision: &Revision,
    removals: &[Removal],
) -> Step {
    match stop_or(stop, host.remove_tracks(playlist, revision, removals)).await {
        None => Step::Stopped,
        Some(Ok(revision)) => {
            notifier.send(Update::TracksRemoved {
                playlist: playlist.to_owned(),
                revision,
            });
            Step::Done
        }
        Some(Err(error)) => {
            log::warn!("remove write failed: {}", error.kind());
            notifier.send(Update::RemoveFailed {
                playlist: playlist.to_owned(),
                failure: Failure::Save,
            });
            Step::Done
        }
    }
}

/// Writes one move to the sidebar's playlists and announces the reply revision.
async fn move_playlist<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    revision: &Revision,
    from: usize,
    to: usize,
) -> Step {
    match stop_or(stop, host.move_playlist(revision, from, to)).await {
        None => Step::Stopped,
        Some(Ok(revision)) => {
            notifier.send(Update::PlaylistMoved { revision });
            Step::Done
        }
        Some(Err(error)) => {
            log::warn!("playlist move failed: {}", error.kind());
            notifier.send(Update::PlaylistMoveFailed);
            Step::Done
        }
    }
}

/// Unlinks one rootlist row and announces the revision its reply names.
async fn remove_playlist<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    playlist: &str,
    revision: &Revision,
    index: usize,
) -> Step {
    match stop_or(stop, host.remove_playlist(revision, index)).await {
        None => Step::Stopped,
        Some(Ok(revision)) => {
            notifier.send(Update::PlaylistRemoved {
                playlist: playlist.to_owned(),
                revision,
            });
            Step::Done
        }
        Some(Err(error)) => {
            log::warn!("playlist removal failed: {}", error.kind());
            notifier.send(Update::PlaylistRemoveFailed {
                playlist: playlist.to_owned(),
            });
            Step::Done
        }
    }
}

/// Puts one playlist at the front of the rootlist and announces the reply.
///
/// The link answers the revision everything after it is based on, so the window
/// keeps that rather than the one it sent.
async fn link_playlist<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    playlist: &str,
    revision: &Revision,
) -> Step {
    match stop_or(stop, host.link_playlist(revision, playlist)).await {
        None => Step::Stopped,
        Some(Ok(revision)) => {
            notifier.send(Update::PlaylistLinked {
                playlist: playlist.to_owned(),
                revision,
            });
            Step::Done
        }
        Some(Err(error)) => {
            log::warn!("playlist link failed: {}", error.kind());
            notifier.send(Update::PlaylistLinkFailed {
                playlist: playlist.to_owned(),
            });
            Step::Done
        }
    }
}

/// Asks whether one album or show is in the library, and announces the answer.
///
/// A probe that does not answer is announced as unknown rather than as "not
/// saved": the heart then stays inert instead of claiming the item is not there.
async fn load_saved<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    uri: &str,
) -> Step {
    let Some(answer) = stop_or(stop, host.saved_state(uri)).await else {
        return Step::Stopped;
    };
    notifier.send(Update::SavedState {
        uri: uri.to_owned(),
        saved: answer,
    });
    Step::Done
}

/// Writes one album or show in or out, and echoes the URI it was asked for.
///
/// The reply carries no body, so the echo is the whole confirmation the window
/// gets until its next read.
async fn set_saved<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    uri: &str,
    saved: bool,
) -> Step {
    match stop_or(stop, host.set_saved(uri, saved)).await {
        None => Step::Stopped,
        Some(Ok(())) => {
            notifier.send(Update::Saved {
                uri: uri.to_owned(),
                saved,
            });
            Step::Done
        }
        Some(Err(error)) => {
            log::warn!("saved write failed: {}", error.kind());
            notifier.send(Update::SavedFailed {
                uri: uri.to_owned(),
                saved,
            });
            Step::Done
        }
    }
}

/// Appends tracks to a playlist and announces the reply revision.
async fn add_tracks<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    playlist: &str,
    revision: &Revision,
    from_index: usize,
    tracks: &[String],
) -> Step {
    match stop_or(
        stop,
        host.add_tracks(playlist, revision, from_index, tracks),
    )
    .await
    {
        None => Step::Stopped,
        Some(Ok(revision)) => {
            notifier.send(Update::TrackAdded {
                playlist: playlist.to_owned(),
                revision,
            });
            Step::Done
        }
        Some(Err(error)) => {
            log::warn!("add write failed: {}", error.kind());
            notifier.send(Update::AddFailed {
                playlist: playlist.to_owned(),
            });
            Step::Done
        }
    }
}

/// Renames one playlist, reading its own state for the base revision.
///
/// The UI may hold several stale copies of a list, so the revision is never
/// taken from it: a read that names the list and says it is editable is the
/// only basis the write may name.
async fn rename_playlist<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    playlist: &str,
    name: &str,
) -> Step {
    let Some(state) = stop_or(stop, host.playlist_state(playlist)).await else {
        return Step::Stopped;
    };
    let Some(state) = state.filter(|state| state.editable) else {
        notifier.send(Update::PlaylistRenameFailed {
            playlist: playlist.to_owned(),
        });
        return Step::Done;
    };
    match stop_or(stop, host.rename_playlist(playlist, &state.revision, name)).await {
        None => Step::Stopped,
        Some(Ok(revision)) => {
            notifier.send(Update::PlaylistRenamed {
                playlist: playlist.to_owned(),
                name: name.to_owned(),
                revision,
            });
            Step::Done
        }
        Some(Err(error)) => {
            log::warn!("rename write failed: {}", error.kind());
            notifier.send(Update::PlaylistRenameFailed {
                playlist: playlist.to_owned(),
            });
            Step::Done
        }
    }
}

/// Creates a playlist, links it, and lands the pick it was made for.
///
/// One command, up to four host calls, and the first failure ends it: a create
/// that failed created nothing, so there is nothing to link, and a link that
/// failed leaves a real playlist the account holds and this app cannot see.
/// Neither is retried here, because no playlist write is idempotent.
///
/// The add is the one failure that is not fatal: the list exists either way, so
/// it is announced as a list with nothing in it plus a notice of its own, never
/// as an add failure, which the sidebar refuses while the create is pending.
async fn create_playlist<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    name: &str,
    revision: &Revision,
    tracks: &[String],
) -> Step {
    let Some(created) = stop_or(stop, host.make_playlist(name)).await else {
        return Step::Stopped;
    };
    let uri = match created {
        Ok(uri) => uri,
        Err(error) => {
            log::warn!("playlist create failed: {}", error.kind());
            notifier.send(Update::PlaylistCreateFailed);
            return Step::Done;
        }
    };
    let Some(linked) = stop_or(stop, host.link_playlist(revision, &uri)).await else {
        return Step::Stopped;
    };
    let revision = match linked {
        Ok(revision) => revision,
        Err(error) => {
            log::warn!("playlist link failed: {}", error.kind());
            notifier.send(Update::PlaylistUnlinked);
            return Step::Done;
        }
    };
    let Some(state) = stop_or(stop, host.playlist_state(&uri)).await else {
        return Step::Stopped;
    };
    let mut access = state;
    // The name is the one thing a create does not carry: the body's name field
    // is ignored, so a fresh list is nameless until it is renamed. The rename
    // answers the revision everything after it is based on.
    let mut named = false;
    if let Some(revision) = access.as_ref().map(|access| access.revision.clone()) {
        match stop_or(stop, host.rename_playlist(&uri, &revision, name)).await {
            None => return Step::Stopped,
            Some(Ok(reply)) => {
                named = true;
                if let Some(access) = &mut access {
                    access.revision = reply;
                }
            }
            Some(Err(error)) => log::warn!("naming a new playlist failed: {}", error.kind()),
        }
    }
    // The list's length is the index its next append names, so a count that did
    // not land would make the next add prepend.
    let mut landed = 0;
    if let Some(revision) = access.as_ref().map(|access| access.revision.clone())
        && !tracks.is_empty()
    {
        match stop_or(stop, host.add_tracks(&uri, &revision, 0, tracks)).await {
            None => return Step::Stopped,
            Some(Ok(reply)) => {
                landed = tracks.len();
                if let Some(access) = &mut access {
                    access.revision = reply;
                }
            }
            Some(Err(error)) => log::warn!("add to a new playlist failed: {}", error.kind()),
        }
    }
    // No reply on this chain carries a cover, so a new row would draw an empty
    // slot until the next rootlist reload. One read fills it, and a read that
    // does not answer costs the row its cover and nothing else.
    let (cover, description) = match stop_or(stop, host.playlist_entry(&uri)).await {
        None => return Step::Stopped,
        Some(Some(entry)) => (entry.cover, entry.description),
        Some(None) => (None, None),
    };
    notifier.send(Update::PlaylistCreated {
        entry: PlaylistEntry {
            uri: uri.clone(),
            name: name.to_owned(),
            // A list the rename did not reach keeps the typed name for this
            // session only, and the notice below says so.
            named,
            cover,
            description,
            tracks: Some(landed),
            owner: Some(Owner::You),
            access,
        },
        revision,
    });
    if !named {
        notifier.send(Update::PlaylistUnnamed {
            playlist: uri.clone(),
        });
    }
    if !tracks.is_empty() && landed == 0 {
        notifier.send(Update::PlaylistUnfilled { playlist: uri });
    }
    Step::Done
}

/// Whether this failure means the device can no longer play at all.
///
/// One track that will not play is not that: the player is up and the next one
/// may play, so the announced stage must stay where it was.
fn ends_playability(error: &SessionError) -> bool {
    matches!(
        error,
        SessionError::NotConnected | SessionError::Playback(PlaybackError::NoAudioDevice)
    )
}

/// Starts the player if needed and hands the one drain thread its events.
fn play<H: SessionHost>(host: &mut H, notifier: &Notifier, uri: &str) {
    match host.play(uri) {
        Ok(()) => {
            // The device can be picked from now on, so the account is told so
            // before the player's first event arrives.
            host.set_playable(true);
            ensure_drain(host, notifier);
        }
        Err(error) => {
            if ends_playability(&error) {
                host.set_playable(false);
            }
            notifier.send(Update::Transport(TransportUpdate::Failed(
                playback_failure(&error),
            )));
        }
    }
}

/// Hands the player's events to the one drain thread, if a player was started
/// since the last call.
///
/// A transfer normally follows a local play, so this is usually a no-op. A
/// stale picker row can still send one to a window that never played, and then
/// preparing the player would leave its events with nobody to read them.
fn ensure_drain<H: SessionHost>(host: &mut H, notifier: &Notifier) {
    if let Some(events) = host.take_events() {
        spawn_drain(notifier.clone(), events);
    }
}

/// Hands one transport order to the window, which applies it and answers.
///
/// The entry stays pending here: the window's answer is the phone's, and until
/// it arrives the registry's own budget is what ends the order. Nothing is
/// applied without an answer, so a phone that hears nothing is never obeyed in
/// secret.
fn serve_order<H: SessionHost>(
    host: &H,
    notifier: &Notifier,
    id: RequestId,
    kind: Kind,
    order: Order,
) -> Step {
    if !host.holding() {
        log::info!(
            target: LANDING_LOG_TARGET,
            "order refused, nothing is held: {}",
            kind.word()
        );
        host.refuse(id);
        return Step::Done;
    }
    // A deadline is what a pending entry has: one the budget already answered is
    // gone, and the phone has been told it failed.
    let Some(deadline) = host.expires(id) else {
        log::info!(
            target: LANDING_LOG_TARGET,
            "order refused, entry gone: {}",
            kind.word()
        );
        host.refuse(id);
        return Step::Done;
    };
    // The window drops an order it receives within the margin of the budget:
    // the effect could not be answered before the phone is told it failed.
    let expires = deadline.checked_sub(ORDER_MARGIN).unwrap_or(deadline);
    if !notifier.send(Update::Remote(RemoteUpdate::Order { id, order, expires })) {
        // A window that has closed cannot carry the order out, and leaving the
        // phone to wait out its budget costs it the whole of it.
        log::info!(
            target: LANDING_LOG_TARGET,
            "order refused, no window: {}",
            kind.word()
        );
        host.refuse(id);
        return Step::Done;
    }
    log::info!(
        target: LANDING_LOG_TARGET,
        "order forwarded: {}",
        kind.word()
    );
    Step::Done
}

/// Serves one hand-over or one phone-side play: prepare, read what is missing,
/// take the sound, name the track and load it.
///
/// Every refusal is answered before anything is put, so a transfer or a play
/// this window cannot carry out leaves the phone's own playback untouched.
/// The hand-over path's own log target. It carries decisions, never a URI: which
/// arm refused, whether the player was ready, and what the put answered.
pub(crate) const LANDING_LOG_TARGET: &str = "takofy::bridge";

async fn serve_landing<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    id: RequestId,
    instruction: Instruction,
) -> Step {
    let plan = match instruction {
        // A phone-side play is the same load as a hand-over: one path, so the
        // two cannot prepare, put or land differently.
        Instruction::Transfer(plan) | Instruction::Play(plan) => plan,
        Instruction::Refused(refusal) => {
            log::info!(target: LANDING_LOG_TARGET, "landing refused: {}", refusal.word());
            host.refuse(id);
            return Step::Done;
        }
        // The order path dispatches before this call, so this arm only keeps a
        // misplaced order from being dropped unanswered.
        Instruction::Order(_) => {
            log::info!(target: LANDING_LOG_TARGET, "landing other: order");
            host.refuse(id);
            return Step::Done;
        }
        // A context the phone asked us to adopt says where its own playback
        // lives, not what to change here: the rows this window holds are the
        // queue. `Failure` would only make the phone retry, and the ack still
        // rides the next put like any other command's.
        Instruction::Ignored => {
            log::info!(target: LANDING_LOG_TARGET, "landing ignored: answered");
            host.answer_order(id, true);
            return Step::Done;
        }
        Instruction::Other(kind) => {
            log::info!(target: LANDING_LOG_TARGET, "landing other: {}", kind.word());
            host.refuse(id);
            return Step::Done;
        }
    };

    if let Err(error) = host.prepare_player() {
        if ends_playability(&error) {
            host.set_playable(false);
        }
        log::warn!("landing prepare failed: {}", error.kind());
        host.refuse(id);
        return Step::Done;
    }
    host.set_playable(true);
    ensure_drain(host, notifier);
    log::info!(target: LANDING_LOG_TARGET, "landing player ready");

    // The phone's own pages cost no read; a context is read only when they are
    // absent, and a failed read falls back to the one track rather than
    // refusing the hand-over.
    let resolution = match plan.resolve_uri() {
        None => Resolution::Failed,
        Some(uri) => match stop_or(stop, host.resolve_context(uri)).await {
            None => return Step::Stopped,
            Some(Ok(uris)) => Resolution::Resolved(uris),
            Some(Err(error)) => {
                log::warn!("context resolve failed: {}", error.kind());
                Resolution::Failed
            }
        },
    };

    // A play can name its row by position alone, so the row may only be known
    // once the read has answered. A landing on no row would take the sound with
    // nothing to play, so it is refused before the put.
    if plan.anchor(&resolution).is_none() {
        log::info!(target: LANDING_LOG_TARGET, "landing refused: no row to anchor on");
        host.refuse(id);
        return Step::Done;
    }

    // Accept answers the phone itself, either way, so a failure here is the
    // whole answer and nothing is loaded.
    let landing = match stop_or(stop, host.accept(id, &plan, resolution)).await {
        None => return Step::Stopped,
        Some(Ok(landing)) => landing,
        Some(Err(error)) => {
            log::info!(
                target: LANDING_LOG_TARGET,
                "landing accept failed: {}",
                error.kind()
            );
            return Step::Done;
        }
    };

    // The row to load is the landing's, not the plan's: a play may have named
    // its row by position, so only the landing knows which uri that position
    // found. `accept` refuses a landing that names no track, so this only keeps
    // a row the landing left empty from being loaded.
    let Some(current) = landing
        .rows
        .get(landing.index)
        .filter(|row| !row.is_empty())
        .cloned()
    else {
        log::info!(target: LANDING_LOG_TARGET, "landing names no row to load");
        return Step::Done;
    };

    // The loss rule may have ended this activation while the accept ran, so the
    // landing is not announced and the player is left alone.
    if !host.holding() {
        log::info!(
            target: LANDING_LOG_TARGET,
            "landing lost the sound before it was announced"
        );
        return Step::Done;
    }

    // Before the load, so no playback update can overtake the rows it belongs
    // to.
    notifier.send(Update::Remote(RemoteUpdate::Landed(landing.clone())));

    // A loss between the announcement and the load leaves the player paused,
    // which is where the transport settles through its own event.
    if let Err(error) = host.load(
        &current,
        landing.position_ms,
        landing.playing && host.holding(),
    ) {
        log::warn!("landing load failed: {}", error.kind());
        notifier.send(Update::Transport(TransportUpdate::Failed(
            playback_failure(&error),
        )));
        // The sound was already taken, so a load that cannot happen gives it
        // back instead of leaving the account naming a silent device.
        if let Err(error) = host.release_sound().await {
            log::warn!("landing give-back failed: {}", error.kind());
            notifier.send(Update::Transport(TransportUpdate::Failed(
                playback_failure(&error),
            )));
        }
        // Nothing plays here, so the transport the landing left waiting stops
        // through the loss rule rather than sitting in `Loading`.
        notifier.send(Update::Remote(RemoteUpdate::Lost {
            epoch: landing.epoch,
        }));
        return Step::Done;
    }

    log::info!(
        target: LANDING_LOG_TARGET,
        "landing loaded at {} ms, playing={}",
        landing.position_ms,
        landing.playing
    );
    // The phone was answered before either, and a row that lands as a bare URI
    // is renamed when its names arrive.
    let labelled = match stop_or(stop, host.release_tracks(std::slice::from_ref(&current))).await {
        None => return Step::Stopped,
        Some(Ok(named)) => named,
        Some(Err(error)) => {
            log_device_error(&error);
            Vec::new()
        }
    };
    if !labelled.is_empty() {
        notifier.send(Update::Remote(RemoteUpdate::Labelled(labelled)));
    }
    Step::Done
}

/// Starts the device watch, once per session.
///
/// `devices.watching` is the only guard that keeps a duplicate or a late request
/// off the dealer's one start: the builder is consumed by the first, so a second
/// cannot be attempted and a failure here is not retryable by asking again.
async fn watch_devices<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    devices: &mut Devices,
    connected: bool,
) -> Step {
    if !connected {
        notifier.send(device_failure(Failure::Devices));
        return Step::Done;
    }
    if devices.watching {
        return Step::Done;
    }
    // The audio path is readied before the device is announced, so the account
    // is told this window can play before anything here has played. Without it
    // the phone can only pick this device after a local play, which makes a
    // freshly opened window the one device it cannot choose. A machine with no
    // audio device is the honest exception: it announces a controller, which the
    // phone lists and greys out.
    match host.prepare_player() {
        Ok(()) => host.set_playable(true),
        Err(error) => {
            if ends_playability(&error) {
                host.set_playable(false);
            }
            log::warn!("device prepare failed: {}", error.kind());
        }
    }
    // Set before the await: a request that arrives while the start is in flight
    // must not reach the builder a second time.
    devices.watching = true;
    devices.dead.store(false, Ordering::SeqCst);
    match stop_or(stop, host.watch_devices()).await {
        None => Step::Stopped,
        Some(Err(error)) => {
            log_device_error(&error);
            devices.watching = false;
            let failure = if error.devices_terminal() {
                Failure::DevicesOff
            } else {
                Failure::Devices
            };
            notifier.send(device_failure(failure));
            Step::Done
        }
        Some(Ok(())) => {
            if let Some(events) = host.take_devices() {
                spawn_device_drain(
                    notifier.clone(),
                    events,
                    devices.sent.clone(),
                    Arc::clone(&devices.dead),
                );
            } else {
                // No stream to read, but the device was announced: keep
                // `watching` so the exit still withdraws it.
                notifier.send(device_failure(Failure::DevicesOff));
            }
            // Taken beside the events: the loop answers requests through it,
            // and a stream with nobody to read it would refuse them untold.
            devices.inbound = host.take_inbound();
            Step::Done
        }
    }
}

/// Asks the cluster for its roster, which is the only way to learn about a
/// device that appeared after launch.
async fn refresh_devices<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    devices: &mut Devices,
    connected: bool,
) -> Step {
    if !devices.watching {
        return watch_devices(host, stop, notifier, devices, connected).await;
    }
    if devices.dead.load(Ordering::SeqCst) {
        notifier.send(device_failure(Failure::DevicesOff));
        return Step::Done;
    }
    match stop_or(stop, host.refresh_devices()).await {
        None => Step::Stopped,
        Some(Ok(view)) => {
            // Compared against the one roster both paths share, so a later push
            // cannot restore a roster this ask has replaced.
            for update in devices.sent.updates(&view) {
                notifier.send(update);
            }
            Step::Done
        }
        Some(Err(error)) => {
            log_device_error(&error);
            notifier.send(device_failure(Failure::Devices));
            Step::Done
        }
    }
}

/// Sends one transport action to the device holding the sound.
///
/// A refused steer is reported once and never retried: the device ignores a
/// command aimed at a cluster that has moved on.
async fn steer_device<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    device: &str,
    steer: CoreSteer,
    connected: bool,
) -> Step {
    if !connected {
        notifier.send(Update::Devices(DeviceUpdate::SteerFailed));
        return Step::Done;
    }
    match stop_or(stop, host.steer(device, steer)).await {
        None => Step::Stopped,
        Some(Ok(())) => Step::Done,
        Some(Err(error)) => {
            log_device_error(&error);
            notifier.send(Update::Devices(DeviceUpdate::SteerFailed));
            Step::Done
        }
    }
}

/// Moves the account's sound to another device.
///
/// A hand-over that is acked reports nothing: the cluster push naming the new
/// holder is what tells the window, so a guess here would race the truth.
async fn transfer_device<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    to: &str,
    connected: bool,
) -> Step {
    if !connected {
        notifier.send(Update::Devices(DeviceUpdate::TransferFailed(
            Failure::Transfer,
        )));
        return Step::Done;
    }
    match stop_or(stop, host.hand_over(to)).await {
        None => Step::Stopped,
        Some(Ok(())) => Step::Done,
        Some(Err(error)) => {
            log_device_error(&error);
            // A window playing from no list has nothing to hand over, and no
            // retry will change that until it plays one.
            let failure = if error.no_context() {
                Failure::NoContext
            } else {
                Failure::Transfer
            };
            notifier.send(Update::Devices(DeviceUpdate::TransferFailed(failure)));
            Step::Done
        }
    }
}

/// Names the track another device is playing, when the label read answers.
///
/// A miss is silent: the title stays `None` and the caption still draws.
async fn label_remote<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    track: &str,
) -> Step {
    let tracks = [track.to_owned()];
    match stop_or(stop, host.release_tracks(&tracks)).await {
        None => Step::Stopped,
        Some(Ok(named)) => {
            if let Some(entry) = named.into_iter().next() {
                notifier.send(Update::Devices(DeviceUpdate::Labelled(entry)));
            }
            Step::Done
        }
        Some(Err(error)) => {
            log_device_error(&error);
            Step::Done
        }
    }
}

/// Names the rows a hand-over adopted, in one read.
///
/// A miss is silent: the rows keep their URIs and the bar still draws.
async fn label_rows<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    uris: &[String],
) -> Step {
    match stop_or(stop, host.release_tracks(uris)).await {
        None => Step::Stopped,
        Some(Ok(named)) => {
            if !named.is_empty() {
                notifier.send(Update::Remote(RemoteUpdate::Labelled(named)));
            }
            Step::Done
        }
        Some(Err(error)) => {
            log_device_error(&error);
            Step::Done
        }
    }
}

/// Takes the device off the account before the player goes, within a bound.
async fn withdraw<H: SessionHost>(host: &mut H, timeout: Duration) {
    if tokio::time::timeout(timeout, host.withdraw_devices())
        .await
        .is_err()
    {
        log::warn!("device withdraw did not finish before shutdown");
    }
}

/// One device-side update carrying a failure with no detail; the reason is for
/// the log.
fn device_failure(failure: Failure) -> Update {
    Update::Devices(DeviceUpdate::Failed(failure))
}

/// Logs the reason, which never reaches the window: it can embed a body.
fn log_device_error(error: &SessionError) {
    log::warn!("device call failed: {}", error.kind());
}

/// The window's steer in the words the wire uses. Total, so a new action is a
/// build failure here.
pub(crate) fn core_steer(steer: Steer) -> CoreSteer {
    match steer {
        Steer::Pause => CoreSteer::Pause,
        Steer::Resume => CoreSteer::Resume,
        Steer::SkipNext => CoreSteer::SkipNext,
        Steer::SkipPrev => CoreSteer::SkipPrev,
        Steer::SeekTo(position_ms) => CoreSteer::SeekTo(position_ms),
    }
}

/// Drains cluster views onto the update channel for as long as the pump lives.
///
/// A plain thread, like the playback drain: no runtime, so `blocking_recv`
/// cannot delay the runtime's drop.
fn spawn_device_drain(
    notifier: Notifier,
    mut events: UnboundedReceiver<ClusterEvent>,
    sent: SentRoster,
    dead: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        while let Some(event) = events.blocking_recv() {
            match event {
                ClusterEvent::View(view) => {
                    for update in sent.updates(&view) {
                        notifier.send(update);
                    }
                }
                ClusterEvent::Failed => {
                    notifier.send(device_failure(Failure::Devices));
                }
                // A loss is not a fault: its epoch is what orders it against a
                // landing still in flight, so it reaches the window as itself.
                ClusterEvent::Lost { epoch } => {
                    log::debug!("connect lost: epoch {epoch}");
                    notifier.send(Update::Remote(RemoteUpdate::Lost { epoch }));
                }
            }
        }
        // The pump is gone while the window is not: a stale remote would
        // otherwise keep advancing with nobody left to report it. The flag
        // keeps a refresh from announcing on a stream that will never push.
        dead.store(true, Ordering::SeqCst);
        notifier.send(device_failure(Failure::DevicesOff));
    });
}

/// Drains playback events onto the update channel for as long as the player
/// lives. A plain thread needs no runtime, so `blocking_recv` cannot delay the
/// runtime's drop.
fn spawn_drain(notifier: Notifier, mut events: UnboundedReceiver<PlaybackEvent>) {
    std::thread::spawn(move || {
        while let Some(event) = events.blocking_recv() {
            notifier.send(transport_of(&event));
        }
    });
}

/// Turns a playback event into the window's vocabulary. Total over the event
/// set, so a new event is a build failure here.
pub(crate) fn transport_of(event: &PlaybackEvent) -> Update {
    Update::Transport(match event {
        PlaybackEvent::Loading => TransportUpdate::Loading,
        PlaybackEvent::Playing { position_ms } => TransportUpdate::Position {
            position_ms: *position_ms,
            paused: false,
        },
        PlaybackEvent::Paused { position_ms } => TransportUpdate::Position {
            position_ms: *position_ms,
            paused: true,
        },
        PlaybackEvent::Ended => TransportUpdate::Ended,
        PlaybackEvent::Unavailable => TransportUpdate::Unavailable,
        PlaybackEvent::Stopped => TransportUpdate::Stopped,
        PlaybackEvent::Duration { duration_ms } => TransportUpdate::Duration {
            duration_ms: *duration_ms,
        },
        PlaybackEvent::Seeked { position_ms } => TransportUpdate::Seeked {
            position_ms: *position_ms,
        },
    })
}

/// A playback failure the bar can name, or `Other` when it cannot. The reason
/// is for the log, never the window.
pub(crate) fn playback_failure(error: &SessionError) -> Failure {
    log::warn!("playback failure: {}", error.kind());
    match error {
        SessionError::Playback(PlaybackError::NoAudioDevice) => Failure::NoAudioDevice,
        _ => Failure::Other,
    }
}

/// Every session-side failure reads the same to the window; the reason is for
/// the log.
pub(crate) fn library_failure(error: &SessionError) -> Failure {
    log::warn!("session failure: {}", error.kind());
    Failure::Load
}

/// The followed-shows list has its own failure: the playlists beside it draw
/// fine, so the library did not fail. The reason is for the log, never the
/// window.
pub(crate) fn followed_failure(error: &SessionError) -> Failure {
    log::warn!("session failure: {}", error.kind());
    Failure::Followed
}

/// The followed-artists list has its own failure: the playlists beside it draw
/// fine, so the library did not fail. The reason is for the log, never the
/// window.
pub(crate) fn artists_failure(error: &SessionError) -> Failure {
    log::warn!("session failure: {}", error.kind());
    Failure::FollowedArtists
}

/// The saved-albums list has its own failure: the playlists beside it draw
/// fine, so the library did not fail. The reason is for the log, never the
/// window.
pub(crate) fn albums_failure(error: &SessionError) -> Failure {
    log::warn!("session failure: {}", error.kind());
    Failure::SavedAlbums
}

/// A liked-songs read has no surface of its own; its failure is the library's
/// load. The reason is for the log, never the window.
pub(crate) fn liked_failure(error: &SessionError) -> Failure {
    log::warn!("session failure: {}", error.kind());
    Failure::Load
}

/// The lyrics pane has its own failure: the page around it may draw fine, and
/// its message names only the lyrics. The reason is for the log, never the
/// window.
pub(crate) fn lyrics_failure(error: &SessionError) -> Failure {
    log::warn!("session failure: {}", error.kind());
    Failure::Lyrics
}

/// A search has its own failure: the menu and the page name only the search,
/// and the field stays editable so typing retries. The reason is for the log,
/// never the window.
pub(crate) fn search_failure(error: &SessionError) -> Failure {
    log::warn!("search failure: {}", error.kind());
    Failure::Search
}

/// Logs the failure variant, then hands the update to the window. The variant
/// is a fixed name; the underlying error can embed a response body and is
/// never formatted.
fn send_update(notifier: &Notifier, update: Update) {
    if let Update::SignInFailed(failure) = update {
        log::warn!("sign-in failed: {failure:?}");
    }
    notifier.send(update);
}

/// Runs the window with the runtime entered, then teardown, then drops the
/// runtime.
///
/// `window` must return. eframe's `run_and_return` option is what makes it do
/// so, and the runtime is dropped only after teardown has awaited the worker.
pub(crate) fn run_ordered<R, F: Future<Output = ()>>(
    runtime: Runtime,
    window: impl FnOnce() -> R,
    teardown: impl FnOnce() -> F,
) -> R {
    let entered = runtime.enter();
    let result = window();
    drop(entered);
    runtime.block_on(teardown());
    drop(runtime);
    result
}

/// Builds the bridge, runs the window, and finishes the worker before the
/// runtime is dropped.
pub(crate) fn run_window<H: SessionHost, S, R>(
    runtime: Runtime,
    host: H,
    store: Arc<dyn CredentialStore>,
    source: Arc<S>,
    window: impl FnOnce(UiEnd, WakeSlot) -> R,
) -> R
where
    S: TokenSource + Send + Sync + 'static,
{
    let (ui_end, slot, worker) = start(runtime.handle(), host, store, source);
    run_ordered(
        runtime,
        move || window(ui_end, slot),
        move || worker.finish(),
    )
}

/// Collapses an auth failure to a variant that carries no detail, because some
/// source errors embed a response body.
pub(crate) fn failure_of(error: &AuthError) -> Failure {
    match error {
        AuthError::Store(_) => Failure::Keychain,
        AuthError::MissingRefreshToken => Failure::NoRefreshToken,
        AuthError::Source(source) if source.kind == SourceErrorKind::PortBusy => Failure::PortBusy,
        AuthError::Source(_) => Failure::Other,
    }
}

/// Turns a resume result into an update. The token is consumed, never shown.
pub(crate) fn resume_update(result: Result<Option<AccessToken>, AuthError>) -> Update {
    match result {
        Ok(Some(_)) => Update::SignedIn,
        Ok(None) => Update::NeedsAuthorization,
        Err(error) => Update::SignInFailed(failure_of(&error)),
    }
}

/// Turns an authorize result into an update. The token is consumed, never shown.
pub(crate) fn authorize_update(result: Result<AccessToken, AuthError>) -> Update {
    match result {
        Ok(_) => Update::SignedIn,
        Err(error) => Update::SignInFailed(failure_of(&error)),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use takofy_core::connect::{ClusterView, ConnectError};
    use takofy_core::credentials::{AccessToken, CredentialStore, RefreshToken, StoreError};
    use takofy_core::engine::{EngineError, RootlistRead};
    use takofy_core::instruction::{Instruction, Kind, Refusal};
    use takofy_core::login::{SourceError, SourceErrorKind, TokenSet, TokenSource};
    use takofy_core::{Resolution, TransferPlan};
    use takofy_model::{
        AlbumDetail, ArtistDetail, ArtistPortrait, CoverImage, CoverUrl, Device, DeviceKind,
        Episode, FollowedArtist, FollowedShow, Landing, LyricLine, Move, PlaylistAccess,
        PlaylistCover, PlaylistEntry, RemotePlayback, RemoteState, Removal, Report, RequestId,
        SavedAlbum, Show, ShowDetail, Status, TrackEntry, TrackLyrics,
    };
    use takofy_ui::signin::failure_message;
    use tokio::runtime::Runtime;

    use super::*;
    use crate::session::{ArtistPage, SessionError, SessionHost, TrackPage};

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

    /// One host call, in the order the command loop made it.
    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Call {
        Play(String),
        Prepare,
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

    impl SessionHost for FakeHost {
        fn connect(
            &mut self,
            _token: &AccessToken,
        ) -> impl Future<Output = Result<(), SessionError>> + Send {
            self.shared.connects.fetch_add(1, Ordering::SeqCst);
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

        fn show(
            &self,
            _show: &str,
        ) -> impl Future<Output = Result<ShowDetail, SessionError>> + Send {
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
                    RelatedScript::Ready => {
                        Ok(uris.iter().map(|uri| followed_artist(uri)).collect())
                    }
                    RelatedScript::Fail => Err(SessionError::NotConnected),
                }
            }
        }

        fn saved_albums(
            &self,
        ) -> impl Future<Output = Result<Vec<SavedAlbum>, SessionError>> + Send {
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
            *self.shared.last_playlist_remove.lock().expect("lock") =
                Some((revision.clone(), index));
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
            if matches!(script, WatchScript::Ready) {
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
            async move {
                match script {
                    WatchScript::Ready => Ok(()),
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

        fn refresh_devices(
            &self,
        ) -> impl Future<Output = Result<ClusterView, SessionError>> + Send {
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
            self.shared
                .calls
                .lock()
                .expect("lock")
                .push(Call::Answer(id, ok));
            let mut pending = self.shared.pending.lock().expect("lock");
            let held = pending.len();
            pending.retain(|(waiting, _)| *waiting != id);
            pending.len() != held
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
            self.shared.volumes.fetch_add(1, Ordering::SeqCst);
            self.shared
                .last_volume
                .store(usize::from(percent), Ordering::SeqCst);
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

    #[test]
    fn failure_of_maps_each_error() {
        assert_eq!(
            failure_of(&AuthError::Store(StoreError::Unavailable)),
            Failure::Keychain
        );
        assert_eq!(
            failure_of(&AuthError::MissingRefreshToken),
            Failure::NoRefreshToken
        );
        assert_eq!(
            failure_of(&auth_source(SourceErrorKind::PortBusy)),
            Failure::PortBusy
        );
        assert_eq!(
            failure_of(&auth_source(SourceErrorKind::Other)),
            Failure::Other
        );
    }

    #[test]
    fn resume_update_maps_the_outcomes() {
        assert_eq!(resume_update(Ok(Some(access("a")))), Update::SignedIn);
        assert_eq!(resume_update(Ok(None)), Update::NeedsAuthorization);
        assert_eq!(
            resume_update(Err(AuthError::Store(StoreError::Unavailable))),
            Update::SignInFailed(Failure::Keychain)
        );
        assert_eq!(
            resume_update(Err(auth_source(SourceErrorKind::PortBusy))),
            Update::SignInFailed(Failure::PortBusy)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_store_that_cannot_be_read_reaches_the_window_as_actionable_text() {
        let store = MemoryStore::unavailable();
        let source = FixedSource::default();

        let error = resume(&store, &source).expect_err("the store load fails");

        assert_eq!(failure_of(&error), Failure::Keychain);
        let message = failure_message(Failure::Keychain);
        assert!(
            message.contains("org.freedesktop.secrets"),
            "the window text must name the D-Bus service: {message}"
        );
        for desktop in ["GNOME", "Plasma", "Hyprland", "i3", "sway"] {
            assert!(
                message.contains(desktop),
                "{desktop} must be distinguished: {message}"
            );
        }
    }

    #[test]
    fn a_corrupt_entry_reaches_the_window_as_actionable_text() {
        let failure = failure_of(&AuthError::Store(StoreError::Corrupt));

        assert_eq!(failure, Failure::Keychain);
        assert!(
            failure_message(failure).contains("signing in again"),
            "a corrupt entry must offer a way out"
        );
    }

    #[test]
    fn authorize_update_maps_the_outcomes() {
        assert_eq!(authorize_update(Ok(access("a"))), Update::SignedIn);
        assert_eq!(
            authorize_update(Err(AuthError::MissingRefreshToken)),
            Update::SignInFailed(Failure::NoRefreshToken)
        );
        assert_eq!(
            authorize_update(Err(auth_source(SourceErrorKind::Other))),
            Update::SignInFailed(Failure::Other)
        );
    }

    #[test]
    fn every_update_wakes_once_after_it_is_queued() {
        let (tx, rx) = channel();
        let slot = WakeSlot::default();
        let probe = Arc::new(QueueProbe::new(rx));
        slot.install(probe.clone());
        let notifier = Notifier { tx, slot };

        notifier.send(Update::SignInFailed(Failure::PortBusy));

        assert_eq!(probe.wakes(), 1);
        assert_eq!(probe.seen(), Some(Update::SignInFailed(Failure::PortBusy)));
    }

    #[test]
    fn an_update_before_the_slot_is_installed_is_queued_without_waking() {
        let (tx, rx) = channel();
        let slot = WakeSlot::default();
        let notifier = Notifier {
            tx,
            slot: slot.clone(),
        };

        notifier.send(Update::NeedsAuthorization);

        let waker = Arc::new(RecordingWaker::default());
        slot.install(waker.clone());
        assert_eq!(waker.wakes(), 0);
        assert_eq!(rx.try_recv().expect("queued"), Update::NeedsAuthorization);
    }

    #[test]
    fn a_closed_receiver_is_not_a_panic() {
        let (tx, rx) = channel();
        let slot = WakeSlot::default();
        let waker = Arc::new(RecordingWaker::default());
        slot.install(waker.clone());
        let notifier = Notifier { tx, slot };

        drop(rx);
        notifier.send(Update::SignedIn);

        assert_eq!(waker.wakes(), 0);
    }

    #[test]
    fn resume_thread_reports_signed_in() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Pending);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            ui_end.updates.recv().expect("resume update")
        });

        assert_eq!(update, Update::SignedIn);
    }

    #[test]
    fn authorize_command_reports_the_outcome() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::on_interactive(Ok(token_set(
            "access",
            Some("refresh"),
        ))));

        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Pending);
        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let first = ui_end.updates.recv().expect("resume update");
            ui_end.commands.send(Command::Authorize).expect("send");
            let second = ui_end.updates.recv().expect("authorize update");
            (first, second)
        });

        assert_eq!(updates, (Update::NeedsAuthorization, Update::SignedIn));
    }

    #[test]
    fn finish_stops_the_command_task_while_the_ui_end_is_alive() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::default());
        let runtime = current_thread_runtime();
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Pending);
        let shared = Arc::clone(&host.shared);
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        // The assertion must run between `finish` and `drop(runtime)`, while
        // the runtime still owns the command task. After the runtime drops, the
        // channel would read as closed even if `finish` never joined.
        let teardown = move || async move {
            worker.finish().await;
            assert!(
                ui_end.commands.is_closed(),
                "closing the command channel is the proof that the task was joined"
            );
            assert_eq!(
                shared.shutdowns.load(Ordering::SeqCst),
                1,
                "the task must shut the session down on its way out"
            );
        };

        run_ordered(runtime, || {}, teardown);
    }

    #[test]
    fn teardown_runs_when_the_window_closure_returns_an_error() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::default());
        let runtime = current_thread_runtime();
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Pending);
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        let ran = Arc::new(AtomicBool::new(false));
        let teardown = {
            let ran = Arc::clone(&ran);
            move || async move {
                // Set before `finish`, so the flag proves teardown ran at all.
                ran.store(true, Ordering::SeqCst);
                worker.finish().await;
                assert!(
                    ui_end.commands.is_closed(),
                    "closing the command channel is the proof that the task was joined"
                );
            }
        };

        let result: Result<(), &str> = run_ordered(runtime, || Err("window failed"), teardown);

        assert!(result.is_err());
        assert!(
            ran.load(Ordering::SeqCst),
            "teardown must run on the Err path"
        );
    }

    #[test]
    fn window_runs_with_runtime_entered() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::default());

        let entered = run_window(
            runtime(),
            FakeHost::new(ConnectScript::Ok, RootlistScript::Pending),
            store,
            source,
            |_ui_end, _slot| {
                assert!(tokio::runtime::Handle::try_current().is_ok());
                tokio::spawn(async {}).abort();
                true
            },
        );

        assert!(entered);
    }

    struct DropGuard {
        log: Arc<Mutex<Vec<&'static str>>>,
    }

    impl Drop for DropGuard {
        fn drop(&mut self) {
            self.log.lock().expect("lock").push("task dropped");
        }
    }

    #[test]
    fn a_pending_task_is_dropped_after_teardown_and_before_runtime_returns() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::default());
        let runtime = runtime();
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Pending);
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

        let window = {
            let log = Arc::clone(&log);
            move || {
                let (started_tx, started_rx) = channel();
                let guard = DropGuard {
                    log: Arc::clone(&log),
                };
                tokio::spawn(async move {
                    let _guard = guard;
                    let _ = started_tx.send(());
                    let (_tx, rx) = tokio::sync::oneshot::channel::<()>();
                    let _ = rx.await;
                });
                started_rx.recv().expect("probe task started");
            }
        };

        let teardown = {
            let log = Arc::clone(&log);
            move || async move {
                assert!(
                    log.lock().expect("lock").is_empty(),
                    "the probe was already dropped when teardown began, so the order proves nothing"
                );
                log.lock().expect("lock").push("teardown");
                worker.finish().await;
            }
        };

        run_ordered(runtime, window, teardown);

        assert_eq!(&*log.lock().expect("lock"), &["teardown", "task dropped"]);
        drop(ui_end);
    }

    #[test]
    fn teardown_can_await_spawn_blocking() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::default());
        let runtime = runtime();
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Pending);
        let (_ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        let ran = Arc::new(AtomicBool::new(false));
        let teardown = {
            let ran = Arc::clone(&ran);
            move || async move {
                let inner = Arc::clone(&ran);
                tokio::task::spawn_blocking(move || inner.store(true, Ordering::SeqCst))
                    .await
                    .expect("blocking task");
                worker.finish().await;
            }
        };

        run_ordered(runtime, || {}, teardown);

        assert!(ran.load(Ordering::SeqCst));
    }

    #[test]
    fn a_token_connects_and_lists_the_rootlist() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:one", "One")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list.clone()));
        let shared = Arc::clone(&host.shared);

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            expect_updates(&ui_end, 2)
        });

        assert!(updates.contains(&Update::SignedIn));
        assert!(updates.contains(&Update::Playlists(list)));
        assert_eq!(shared.connects.load(Ordering::SeqCst), 1);
        assert_eq!(shared.rootlists.load(Ordering::SeqCst), 1);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_rootlist_read_that_named_a_revision_announces_its_access_right_after_the_list() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:one", "One")];
        let access = PlaylistAccess {
            revision: revision(9),
            editable: true,
        };
        let host = FakeHost::new(
            ConnectScript::Ok,
            RootlistScript::ReadyWith(RootlistRead {
                entries: list.clone(),
                access: Some(access.clone()),
            }),
        );

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            expect_updates(&ui_end, 3)
        });

        assert_eq!(
            updates,
            vec![
                Update::SignedIn,
                Update::Playlists(list),
                Update::RootlistAccess(access),
            ],
            "the access follows the list it belongs to"
        );
    }

    #[test]
    fn a_rootlist_read_with_no_revision_announces_no_access_update() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:one", "One")];
        let host = FakeHost::new(
            ConnectScript::Ok,
            RootlistScript::ReadyWith(RootlistRead {
                entries: list.clone(),
                access: None,
            }),
        );

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            expect_updates(&ui_end, 2)
        });

        assert_eq!(updates, vec![Update::SignedIn, Update::Playlists(list)]);
    }

    #[test]
    fn a_playlist_move_announces_the_reply_revision_and_sends_what_was_asked() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        let base = revision(1);
        let reply = revision(2);
        *shared.rootlist_move_script.lock().expect("lock") =
            RootlistMoveScript::Ready(reply.clone());

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::MovePlaylist {
                    revision: base.clone(),
                    from: 1,
                    to: 4,
                })
                .expect("move");
            ui_end.updates.recv().expect("moved")
        });

        assert_eq!(update, Update::PlaylistMoved { revision: reply });
        assert_eq!(shared.rootlist_move_writes.load(Ordering::SeqCst), 1);
        assert_eq!(
            shared.last_root_move.lock().expect("lock").clone(),
            Some((base, 1, 4)),
            "the write names the revision and the gap that were asked for"
        );
    }

    #[test]
    fn a_failed_playlist_move_is_announced() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.rootlist_move_script.lock().expect("lock") = RootlistMoveScript::Fail;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::MovePlaylist {
                    revision: revision(1),
                    from: 0,
                    to: 1,
                })
                .expect("move");
            ui_end.updates.recv().expect("move failed")
        });

        assert_eq!(update, Update::PlaylistMoveFailed);
        assert_eq!(shared.rootlist_move_writes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_playlist_move_before_connect_is_announced_as_a_failure() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::default());
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
        let shared = Arc::clone(&host.shared);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            assert_eq!(
                ui_end.updates.recv().expect("resume update"),
                Update::NeedsAuthorization
            );
            ui_end
                .commands
                .send(Command::MovePlaylist {
                    revision: revision(1),
                    from: 0,
                    to: 1,
                })
                .expect("move");
            ui_end.updates.recv().expect("move failed")
        });

        assert_eq!(update, Update::PlaylistMoveFailed);
        assert_eq!(shared.rootlist_move_writes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_stop_with_the_playlist_move_pending_shuts_down() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.rootlist_move_script.lock().expect("lock") = RootlistMoveScript::Pending;
        let (started_tx, started_rx) = channel();
        *shared.rootlist_move_started.lock().expect("lock") = Some(started_tx);

        let runtime = runtime();
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        let window = move || {
            loop {
                if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                    break;
                }
            }
            ui_end
                .commands
                .send(Command::MovePlaylist {
                    revision: revision(1),
                    from: 0,
                    to: 1,
                })
                .expect("move");
            started_rx.recv().expect("move started");
        };
        let teardown = move || async move {
            tokio::time::timeout(Duration::from_secs(1), worker.finish())
                .await
                .expect("finish must resolve when the stop interrupts the move");
            assert_eq!(
                shared.shutdowns.load(Ordering::SeqCst),
                1,
                "the shutdown must run when the stop interrupts the move"
            );
        };

        run_ordered(runtime, window, teardown);
    }

    #[test]
    fn a_playlist_removal_announces_the_reply_revision_and_sends_what_was_asked() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        let base = revision(1);
        let reply = revision(2);
        *shared.remove_playlist_script.lock().expect("lock") =
            RootlistMoveScript::Ready(reply.clone());

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::RemovePlaylist {
                    playlist: "spotify:playlist:a".to_owned(),
                    revision: base.clone(),
                    index: 3,
                })
                .expect("remove");
            ui_end.updates.recv().expect("removed")
        });

        assert_eq!(
            update,
            Update::PlaylistRemoved {
                playlist: "spotify:playlist:a".to_owned(),
                revision: reply,
            }
        );
        assert_eq!(shared.remove_playlist_writes.load(Ordering::SeqCst), 1);
        assert_eq!(
            shared.last_playlist_remove.lock().expect("lock").clone(),
            Some((base, 3)),
            "the unlink names the revision and the row that were asked for"
        );
    }

    #[test]
    fn a_failed_playlist_removal_is_announced() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.remove_playlist_script.lock().expect("lock") = RootlistMoveScript::Fail;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::RemovePlaylist {
                    playlist: "spotify:playlist:a".to_owned(),
                    revision: revision(1),
                    index: 0,
                })
                .expect("remove");
            ui_end.updates.recv().expect("remove failed")
        });

        assert_eq!(
            update,
            Update::PlaylistRemoveFailed {
                playlist: "spotify:playlist:a".to_owned(),
            }
        );
        assert_eq!(shared.remove_playlist_writes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_stop_with_the_playlist_removal_pending_shuts_down() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.remove_playlist_script.lock().expect("lock") = RootlistMoveScript::Pending;
        let (started_tx, started_rx) = channel();
        *shared.remove_playlist_started.lock().expect("lock") = Some(started_tx);

        let runtime = runtime();
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        let window = move || {
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::RemovePlaylist {
                    playlist: "spotify:playlist:a".to_owned(),
                    revision: revision(1),
                    index: 0,
                })
                .expect("remove");
            started_rx.recv().expect("remove started");
        };
        let teardown = move || async move {
            tokio::time::timeout(Duration::from_secs(1), worker.finish())
                .await
                .expect("finish must resolve when the stop interrupts the removal");
            assert_eq!(
                shared.shutdowns.load(Ordering::SeqCst),
                1,
                "the shutdown must run when the stop interrupts the removal"
            );
        };

        run_ordered(runtime, window, teardown);
    }

    #[test]
    fn a_saved_probe_reaches_the_host_and_its_answer_comes_back() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.probe_answer.lock().expect("lock") = Some(true);
        let album = "spotify:album:4uLU6hMCjMI75M1A2tKUQC";

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadSaved {
                    uri: album.to_owned(),
                })
                .expect("probe");
            ui_end.updates.recv().expect("answered")
        });

        assert_eq!(
            update,
            Update::SavedState {
                uri: album.to_owned(),
                saved: Some(true),
            }
        );
        assert_eq!(shared.probes.load(Ordering::SeqCst), 1);
        assert_eq!(
            shared.last_probe.lock().expect("lock").clone(),
            Some(album.to_owned())
        );
    }

    #[test]
    fn a_link_puts_the_playlist_on_the_rootlist_and_announces_the_new_revision() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        let gone = "spotify:playlist:gone";

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LinkPlaylist {
                    playlist: gone.to_owned(),
                    revision: revision(0x01),
                })
                .expect("link");
            ui_end.updates.recv().expect("linked")
        });

        assert!(matches!(
            update,
            Update::PlaylistLinked { playlist, .. } if playlist == gone
        ));
        assert_eq!(shared.link_writes.load(Ordering::SeqCst), 1);
        assert_eq!(
            shared
                .last_link
                .lock()
                .expect("lock")
                .clone()
                .map(|(_, playlist)| playlist),
            Some(gone.to_owned())
        );
    }

    #[test]
    fn a_saved_write_announces_the_uri_and_the_direction_it_was_asked_for() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        let album = "spotify:album:4uLU6hMCjMI75M1A2tKUQC";

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::SetSaved {
                    uri: album.to_owned(),
                    saved: true,
                })
                .expect("save");
            ui_end.updates.recv().expect("saved")
        });

        assert_eq!(
            update,
            Update::Saved {
                uri: album.to_owned(),
                saved: true,
            },
            "the reply is empty, so the window hears what it asked for"
        );
        assert_eq!(shared.remove_saved_writes.load(Ordering::SeqCst), 1);
        assert_eq!(
            shared.last_saved_remove.lock().expect("lock").clone(),
            Some((album.to_owned(), true))
        );
    }

    #[test]
    fn a_failed_saved_write_is_announced() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.remove_saved_script.lock().expect("lock") = LikeScript::Fail;
        let show = "spotify:show:4uLU6hMCjMI75M1A2tKUQC";

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::SetSaved {
                    uri: show.to_owned(),
                    saved: false,
                })
                .expect("remove");
            ui_end.updates.recv().expect("remove failed")
        });

        assert_eq!(
            update,
            Update::SavedFailed {
                uri: show.to_owned(),
                saved: false,
            }
        );
        assert_eq!(shared.remove_saved_writes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_stop_during_a_saved_removal_ends_the_task() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.remove_saved_script.lock().expect("lock") = LikeScript::Pending;
        let (started_tx, started_rx) = channel();
        *shared.remove_saved_started.lock().expect("lock") = Some(started_tx);

        let runtime = runtime();
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        let window = move || {
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::SetSaved {
                    uri: "spotify:album:4uLU6hMCjMI75M1A2tKUQC".to_owned(),
                    saved: false,
                })
                .expect("remove");
            started_rx.recv().expect("remove started");
        };
        let teardown = move || async move {
            tokio::time::timeout(Duration::from_secs(1), worker.finish())
                .await
                .expect("finish must resolve when the stop interrupts the removal");
            assert_eq!(
                shared.shutdowns.load(Ordering::SeqCst),
                1,
                "the shutdown must run when the stop interrupts the removal"
            );
        };

        run_ordered(runtime, window, teardown);
    }

    #[test]
    fn an_add_announces_the_reply_revision_and_sends_what_was_asked() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        let base = revision(1);
        let reply = revision(2);
        *shared.add_script.lock().expect("lock") = AddScript::Ready(reply.clone());
        let tracks = vec![
            "spotify:track:4uLU6hMCjMI75M1A2tKUQC".to_owned(),
            "spotify:track:4uLU6hMCjMI75M1A2tKUQD".to_owned(),
            "spotify:track:4uLU6hMCjMI75M1A2tKUQE".to_owned(),
        ];

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::AddTracks {
                    playlist: "spotify:playlist:a".to_owned(),
                    revision: base.clone(),
                    from_index: 7,
                    tracks: tracks.clone(),
                })
                .expect("add");
            ui_end.updates.recv().expect("added")
        });

        assert_eq!(
            update,
            Update::TrackAdded {
                playlist: "spotify:playlist:a".to_owned(),
                revision: reply,
            }
        );
        assert_eq!(shared.add_writes.load(Ordering::SeqCst), 1, "one write");
        assert_eq!(
            shared.last_add.lock().expect("lock").clone(),
            Some(("spotify:playlist:a".to_owned(), base, 7, tracks)),
            "the write names the playlist, the revision, the index and the whole pick"
        );
    }

    #[test]
    fn a_failed_add_is_announced() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.add_script.lock().expect("lock") = AddScript::Fail;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::AddTracks {
                    playlist: "spotify:playlist:a".to_owned(),
                    revision: revision(1),
                    from_index: 0,
                    tracks: vec!["spotify:track:x".to_owned()],
                })
                .expect("add");
            ui_end.updates.recv().expect("add failed")
        });

        assert_eq!(
            update,
            Update::AddFailed {
                playlist: "spotify:playlist:a".to_owned(),
            }
        );
        assert_eq!(shared.add_writes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn an_add_before_connect_is_announced_as_a_failure() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::default());
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
        let shared = Arc::clone(&host.shared);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            assert_eq!(
                ui_end.updates.recv().expect("resume update"),
                Update::NeedsAuthorization
            );
            ui_end
                .commands
                .send(Command::AddTracks {
                    playlist: "spotify:playlist:a".to_owned(),
                    revision: revision(1),
                    from_index: 0,
                    tracks: vec!["spotify:track:x".to_owned()],
                })
                .expect("add");
            ui_end.updates.recv().expect("add failed")
        });

        assert_eq!(
            update,
            Update::AddFailed {
                playlist: "spotify:playlist:a".to_owned(),
            }
        );
        assert_eq!(shared.add_writes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_stop_with_the_add_pending_shuts_down() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.add_script.lock().expect("lock") = AddScript::Pending;
        let (started_tx, started_rx) = channel();
        *shared.add_started.lock().expect("lock") = Some(started_tx);

        let runtime = runtime();
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        let window = move || {
            loop {
                if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                    break;
                }
            }
            ui_end
                .commands
                .send(Command::AddTracks {
                    playlist: "spotify:playlist:a".to_owned(),
                    revision: revision(1),
                    from_index: 0,
                    tracks: vec!["spotify:track:x".to_owned()],
                })
                .expect("add");
            started_rx.recv().expect("add started");
        };
        let teardown = move || async move {
            tokio::time::timeout(Duration::from_secs(1), worker.finish())
                .await
                .expect("finish must resolve when the stop interrupts the add");
            assert_eq!(
                shared.shutdowns.load(Ordering::SeqCst),
                1,
                "the shutdown must run when the stop interrupts the add"
            );
        };

        run_ordered(runtime, window, teardown);
    }

    #[test]
    fn a_stop_with_the_rename_pending_shuts_down() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.state_script.lock().expect("lock") = StateScript::Ready(PlaylistAccess {
            revision: revision(9),
            editable: true,
        });
        *shared.rename_script.lock().expect("lock") = RenameScript::Pending;
        let (started_tx, started_rx) = channel();
        *shared.rename_started.lock().expect("lock") = Some(started_tx);

        let runtime = runtime();
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        let window = move || {
            loop {
                if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                    break;
                }
            }
            ui_end
                .commands
                .send(Command::CreatePlaylist {
                    name: "Night Drive".to_owned(),
                    revision: revision(1),
                    tracks: Vec::new(),
                })
                .expect("create");
            started_rx.recv().expect("rename started");
        };
        let teardown = move || async move {
            tokio::time::timeout(Duration::from_secs(1), worker.finish())
                .await
                .expect("finish must resolve when the stop interrupts the rename");
            assert_eq!(
                shared.shutdowns.load(Ordering::SeqCst),
                1,
                "the shutdown must run when the stop interrupts the rename"
            );
        };

        run_ordered(runtime, window, teardown);
    }

    #[test]
    fn a_create_makes_links_reads_state_and_announces_the_entry() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        let access = PlaylistAccess {
            revision: revision(9),
            editable: true,
        };
        *shared.state_script.lock().expect("lock") = StateScript::Ready(access.clone());

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::CreatePlaylist {
                    name: "Night Drive".to_owned(),
                    revision: revision(1),
                    tracks: Vec::new(),
                })
                .expect("create");
            ui_end.updates.recv().expect("created")
        });

        assert_eq!(
            update,
            Update::PlaylistCreated {
                entry: PlaylistEntry {
                    uri: CREATED.to_owned(),
                    name: "Night Drive".to_owned(),
                    named: true,
                    cover: None,
                    description: None,
                    tracks: Some(0),
                    owner: Some(Owner::You),
                    access: Some(PlaylistAccess {
                        revision: revision(4),
                        editable: true,
                    }),
                },
                revision: revision(4),
            },
            "the row holds the rename's reply, which is what the next write names"
        );
        assert_eq!(shared.create_writes.load(Ordering::SeqCst), 1);
        assert_eq!(shared.link_writes.load(Ordering::SeqCst), 1);
        assert_eq!(shared.state_reads.load(Ordering::SeqCst), 1);
        assert_eq!(
            shared.rename_writes.load(Ordering::SeqCst),
            1,
            "it was named"
        );
        assert_eq!(
            shared.last_rename.lock().expect("lock").clone(),
            Some((CREATED.to_owned(), revision(9), "Night Drive".to_owned())),
            "the rename names the list the create made, on the read's revision"
        );
    }

    #[test]
    fn a_created_row_carries_the_cover_the_one_playlist_read_gives() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        let cover = CoverUrl::from_wire("https://i.scdn.co/image/abcdef").expect("a cover");
        let read = PlaylistEntry {
            cover: Some(PlaylistCover::derived(cover.clone())),
            description: Some("Made here".to_owned()),
            ..PlaylistEntry {
                uri: CREATED.to_owned(),
                name: "Night Drive".to_owned(),
                named: true,
                cover: None,
                description: None,
                tracks: Some(0),
                owner: Some(Owner::You),
                access: None,
            }
        };
        *shared.entry_script.lock().expect("lock") = EntryScript::Ready(Box::new(read));

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::CreatePlaylist {
                    name: "Night Drive".to_owned(),
                    revision: revision(1),
                    tracks: Vec::new(),
                })
                .expect("create");
            ui_end.updates.recv().expect("created")
        });

        let Update::PlaylistCreated { entry, .. } = update else {
            panic!("a create announces the row");
        };
        assert_eq!(
            entry.cover,
            Some(PlaylistCover::derived(cover)),
            "the row draws the cover the read gave, not an empty slot"
        );
        assert_eq!(entry.description.as_deref(), Some("Made here"));
        assert_eq!(shared.entry_reads.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_rename_answers_what_the_add_is_based_on() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.state_script.lock().expect("lock") = StateScript::Ready(PlaylistAccess {
            revision: revision(9),
            editable: true,
        });
        *shared.rename_script.lock().expect("lock") = RenameScript::Ready(revision(11));
        *shared.add_script.lock().expect("lock") = AddScript::Ready(revision(12));

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::CreatePlaylist {
                    name: "Night Drive".to_owned(),
                    revision: revision(1),
                    tracks: vec!["spotify:track:a".to_owned()],
                })
                .expect("create");
            ui_end.updates.recv().expect("created")
        });

        let Update::PlaylistCreated { entry, .. } = update else {
            panic!("a create announces the row");
        };
        assert_eq!(
            entry.access.map(|access| access.revision),
            Some(revision(12)),
            "the row holds the add's reply, not the read's revision"
        );
        assert_eq!(
            shared
                .last_add
                .lock()
                .expect("lock")
                .clone()
                .map(|add| add.1),
            Some(revision(11)),
            "the add is based on the rename's reply: a stale base would undo it"
        );
    }

    #[test]
    fn a_create_whose_rename_fails_says_so_and_still_takes_the_pick() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.state_script.lock().expect("lock") = StateScript::Ready(PlaylistAccess {
            revision: revision(9),
            editable: true,
        });
        *shared.rename_script.lock().expect("lock") = RenameScript::Fail;

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::CreatePlaylist {
                    name: "Night Drive".to_owned(),
                    revision: revision(1),
                    tracks: vec!["spotify:track:a".to_owned()],
                })
                .expect("create");
            expect_updates(&ui_end, 2)
        });

        let (first, second) = (updates[0].clone(), updates[1].clone());
        let Update::PlaylistCreated { entry, .. } = first else {
            panic!("a create announces the row");
        };
        assert!(!entry.named, "a row the server did not name claims a name");
        assert_eq!(entry.tracks, Some(1), "the pick still lands");
        assert_eq!(
            second,
            Update::PlaylistUnnamed {
                playlist: CREATED.to_owned(),
            },
            "the failure is said after the row, because the row clears the notice"
        );
        assert_eq!(shared.add_writes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_link_uses_the_revision_the_command_named_and_the_uri_step_one_returned() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        let base = revision(7);
        *shared.create_script.lock().expect("lock") =
            CreateScript::Ready("spotify:playlist:another".to_owned());

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::CreatePlaylist {
                    name: "Night Drive".to_owned(),
                    revision: base.clone(),
                    tracks: Vec::new(),
                })
                .expect("create");
            ui_end.updates.recv().expect("created")
        });

        assert_eq!(
            shared.last_create.lock().expect("lock").clone(),
            Some("Night Drive".to_owned())
        );
        assert_eq!(
            shared.last_link.lock().expect("lock").clone(),
            Some((base, "spotify:playlist:another".to_owned())),
            "the link names the rootlist revision and the created URI"
        );
        assert!(matches!(
            update,
            Update::PlaylistCreated { entry, .. } if entry.uri == "spotify:playlist:another"
        ));
    }

    #[test]
    fn a_make_failure_announces_create_failed_and_never_links() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.create_script.lock().expect("lock") = CreateScript::Fail;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::CreatePlaylist {
                    name: "Night Drive".to_owned(),
                    revision: revision(1),
                    tracks: Vec::new(),
                })
                .expect("create");
            ui_end.updates.recv().expect("create failed")
        });

        assert_eq!(update, Update::PlaylistCreateFailed);
        assert_eq!(shared.create_writes.load(Ordering::SeqCst), 1);
        assert_eq!(
            shared.link_writes.load(Ordering::SeqCst),
            0,
            "a create that made nothing linked nothing"
        );
        assert_eq!(shared.state_reads.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_link_failure_announces_unlinked_and_never_reads_state() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.link_script.lock().expect("lock") = LinkScript::Fail;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::CreatePlaylist {
                    name: "Night Drive".to_owned(),
                    revision: revision(1),
                    tracks: Vec::new(),
                })
                .expect("create");
            ui_end.updates.recv().expect("unlinked")
        });

        assert_eq!(update, Update::PlaylistUnlinked);
        assert_eq!(shared.link_writes.load(Ordering::SeqCst), 1);
        assert_eq!(shared.state_reads.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_create_lands_the_pick_in_the_new_list_as_one_add() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        let state = PlaylistAccess {
            revision: revision(9),
            editable: true,
        };
        *shared.state_script.lock().expect("lock") = StateScript::Ready(state.clone());
        *shared.add_script.lock().expect("lock") = AddScript::Ready(revision(10));
        let tracks = vec![
            "spotify:track:a".to_owned(),
            "spotify:track:b".to_owned(),
            "spotify:track:c".to_owned(),
        ];

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::CreatePlaylist {
                    name: "Night Drive".to_owned(),
                    revision: revision(1),
                    tracks: tracks.clone(),
                })
                .expect("create");
            ui_end.updates.recv().expect("created")
        });

        assert_eq!(
            update,
            Update::PlaylistCreated {
                entry: PlaylistEntry {
                    uri: CREATED.to_owned(),
                    name: "Night Drive".to_owned(),
                    named: true,
                    cover: None,
                    description: None,
                    tracks: Some(3),
                    owner: Some(Owner::You),
                    access: Some(PlaylistAccess {
                        revision: revision(10),
                        editable: true,
                    }),
                },
                revision: revision(4),
            },
            "the landed count is in the row, so the next append names index 3"
        );
        assert_eq!(shared.state_reads.load(Ordering::SeqCst), 1);
        assert_eq!(shared.add_writes.load(Ordering::SeqCst), 1, "one add");
        assert_eq!(
            shared.last_add.lock().expect("lock").clone(),
            Some((CREATED.to_owned(), revision(4), 0, tracks)),
            "the whole pick lands at index zero, on the rename's reply"
        );
    }

    #[test]
    fn a_create_that_could_not_add_the_pick_says_so_after_the_create() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.state_script.lock().expect("lock") = StateScript::Ready(PlaylistAccess {
            revision: revision(9),
            editable: true,
        });
        *shared.add_script.lock().expect("lock") = AddScript::Fail;

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::CreatePlaylist {
                    name: "Night Drive".to_owned(),
                    revision: revision(1),
                    tracks: vec!["spotify:track:a".to_owned()],
                })
                .expect("create");
            let created = ui_end.updates.recv().expect("created");
            // The rename landed, so the only failure to report is the pick.
            let unfilled = ui_end.updates.recv().expect("unfilled");
            (created, unfilled)
        });

        assert!(
            matches!(
                &updates.0,
                Update::PlaylistCreated { entry, .. }
                    if entry.tracks == Some(0) && entry.named
            ),
            "the list exists, is named, and counts nothing: {:?}",
            updates.0
        );
        assert_eq!(
            updates.1,
            Update::PlaylistUnfilled {
                playlist: CREATED.to_owned(),
            }
        );
        assert_eq!(
            shared.add_writes.load(Ordering::SeqCst),
            1,
            "the refusal is spent with the gesture, not retried"
        );
    }

    #[test]
    fn a_create_with_no_pick_makes_no_add_call() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.state_script.lock().expect("lock") = StateScript::Ready(PlaylistAccess {
            revision: revision(9),
            editable: true,
        });

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::CreatePlaylist {
                    name: "Night Drive".to_owned(),
                    revision: revision(1),
                    tracks: Vec::new(),
                })
                .expect("create");
            ui_end.updates.recv().expect("created")
        });

        assert!(matches!(update, Update::PlaylistCreated { entry, .. } if entry.tracks == Some(0)));
        assert_eq!(shared.add_writes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_create_whose_state_read_names_no_revision_adds_nothing_and_says_so() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::CreatePlaylist {
                    name: "Night Drive".to_owned(),
                    revision: revision(1),
                    tracks: vec!["spotify:track:a".to_owned()],
                })
                .expect("create");
            let created = ui_end.updates.recv().expect("created");
            let unnamed = ui_end.updates.recv().expect("unnamed");
            let unfilled = ui_end.updates.recv().expect("unfilled");
            (created, unnamed, unfilled)
        });

        assert!(
            matches!(
                &updates.0,
                Update::PlaylistCreated { entry, .. }
                    if entry.tracks == Some(0) && entry.access.is_none() && !entry.named
            ),
            "{:?}",
            updates.0
        );
        assert_eq!(
            updates.1,
            Update::PlaylistUnnamed {
                playlist: CREATED.to_owned(),
            },
            "a list with no revision cannot be renamed either"
        );
        assert_eq!(
            updates.2,
            Update::PlaylistUnfilled {
                playlist: CREATED.to_owned(),
            }
        );
        assert_eq!(
            shared.add_writes.load(Ordering::SeqCst),
            0,
            "a list with no revision names no version to add on"
        );
    }

    #[test]
    fn a_failed_state_read_still_announces_created_with_no_access() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::CreatePlaylist {
                    name: "Night Drive".to_owned(),
                    revision: revision(1),
                    tracks: Vec::new(),
                })
                .expect("create");
            let created = ui_end.updates.recv().expect("created");
            let unnamed = ui_end.updates.recv().expect("unnamed");
            (created, unnamed)
        });

        assert_eq!(
            updates.0,
            Update::PlaylistCreated {
                entry: PlaylistEntry {
                    uri: CREATED.to_owned(),
                    name: "Night Drive".to_owned(),
                    // Nothing could name it: the read that gives the revision
                    // the rename needs did not answer.
                    named: false,
                    cover: None,
                    description: None,
                    tracks: Some(0),
                    owner: Some(Owner::You),
                    access: None,
                },
                revision: revision(4),
            },
            "a list that could not be read is still a list to draw"
        );
        assert_eq!(
            updates.1,
            Update::PlaylistUnnamed {
                playlist: CREATED.to_owned(),
            }
        );
        assert_eq!(shared.state_reads.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_create_before_connect_is_announced_as_a_failure() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::default());
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
        let shared = Arc::clone(&host.shared);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            assert_eq!(
                ui_end.updates.recv().expect("resume update"),
                Update::NeedsAuthorization
            );
            ui_end
                .commands
                .send(Command::CreatePlaylist {
                    name: "Night Drive".to_owned(),
                    revision: revision(1),
                    tracks: Vec::new(),
                })
                .expect("create");
            ui_end.updates.recv().expect("create failed")
        });

        assert_eq!(update, Update::PlaylistCreateFailed);
        assert_eq!(shared.link_writes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_stop_during_each_step_ends_the_task() {
        for step in ["make", "link", "state"] {
            let store = Arc::new(MemoryStore::seeded("stored"));
            let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
            let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
            let shared = Arc::clone(&host.shared);
            let (started_tx, started_rx) = channel();
            match step {
                "make" => {
                    *shared.create_script.lock().expect("lock") = CreateScript::Pending;
                    *shared.create_started.lock().expect("lock") = Some(started_tx);
                }
                "link" => {
                    *shared.link_script.lock().expect("lock") = LinkScript::Pending;
                    *shared.link_started.lock().expect("lock") = Some(started_tx);
                }
                _ => {
                    *shared.state_script.lock().expect("lock") = StateScript::Pending;
                    *shared.state_started.lock().expect("lock") = Some(started_tx);
                }
            }

            let runtime = runtime();
            let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

            let window = move || {
                loop {
                    if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                        break;
                    }
                }
                ui_end
                    .commands
                    .send(Command::CreatePlaylist {
                        name: "Night Drive".to_owned(),
                        revision: revision(1),
                        tracks: Vec::new(),
                    })
                    .expect("create");
                started_rx.recv().expect("the step started");
            };
            let teardown = move || async move {
                tokio::time::timeout(Duration::from_secs(1), worker.finish())
                    .await
                    .unwrap_or_else(|_| {
                        panic!("finish must resolve when the stop interrupts {step}")
                    });
                assert_eq!(
                    shared.shutdowns.load(Ordering::SeqCst),
                    1,
                    "the shutdown must run when the stop interrupts {step}"
                );
            };

            run_ordered(runtime, window, teardown);
        }
    }

    #[test]
    fn the_edge_steps_encode_the_gap_the_server_reads_for_the_rootlist() {
        let base = revision(1);
        let write = |len: usize, from: usize, boundary: usize| {
            let found = takofy_ui::reorder::step(len, from, boundary).expect("a move");
            takofy_core::playlist_edit::move_body(
                &base,
                &[Move {
                    from: found.from,
                    length: 1,
                    to: found.to,
                }],
            )
        };

        // The rootlist reads `to` the same way a playlist does: the gap in the
        // list before the move.
        assert!(write(5, 0, 2).ends_with(&[0x08, 0x00, 0x10, 0x01, 0x18, 0x02]));
        assert!(write(5, 0, 5).ends_with(&[0x08, 0x00, 0x10, 0x01, 0x18, 0x05]));
        assert!(write(5, 3, 0).ends_with(&[0x08, 0x03, 0x10, 0x01, 0x18, 0x00]));
        assert!(write(5, 2, 5).ends_with(&[0x08, 0x02, 0x10, 0x01, 0x18, 0x05]));
    }

    #[test]
    fn the_probes_swap_is_a_move_of_row_zero_to_gap_two() {
        // MOV 0, length 1, to 2 swaps entries 0 and 1 on the live account.
        let found = takofy_ui::reorder::step(23, 0, 2).expect("a move");
        assert_eq!((found.from, found.to, found.landed), (0, 2, 1));
        let body = takofy_core::playlist_edit::move_body(
            &revision(1),
            &[Move {
                from: found.from,
                length: 1,
                to: found.to,
            }],
        );
        assert!(body.ends_with(&[0x08, 0x00, 0x10, 0x01, 0x18, 0x02]));
    }

    #[test]
    fn rootlist_replies_keep_the_order_their_commands_were_sent_in() {
        // the sidebar's read rule rests on this order
        fn run(move_first: bool) -> Vec<Update> {
            let store = Arc::new(MemoryStore::seeded("stored"));
            let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
            let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
            let shared = Arc::clone(&host.shared);
            *shared.rootlist_move_script.lock().expect("lock") =
                RootlistMoveScript::Ready(revision(2));

            run_window(runtime(), host, store, source, |ui_end, slot| {
                slot.install(Arc::new(RecordingWaker::default()));
                let _ = expect_updates(&ui_end, 2);
                let move_command = Command::MovePlaylist {
                    revision: revision(1),
                    from: 0,
                    to: 1,
                };
                let reload = Command::ReloadLibrary;
                if move_first {
                    ui_end.commands.send(move_command).expect("move");
                    ui_end.commands.send(reload).expect("reload");
                } else {
                    ui_end.commands.send(reload).expect("reload");
                    ui_end.commands.send(move_command).expect("move");
                }
                vec![
                    ui_end.updates.recv().expect("first reply"),
                    ui_end.updates.recv().expect("second reply"),
                ]
            })
        }

        fn moved(update: &Update) -> bool {
            matches!(update, Update::PlaylistMoved { .. })
        }

        fn read(update: &Update) -> bool {
            matches!(update, Update::Playlists(_))
        }

        let move_first = run(true);
        assert!(
            moved(&move_first[0]) && read(&move_first[1]),
            "{move_first:?}"
        );

        let reload_first = run(false);
        assert!(
            read(&reload_first[0]) && moved(&reload_first[1]),
            "{reload_first:?}"
        );
    }

    #[test]
    fn an_empty_rootlist_is_announced_as_empty() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            expect_updates(&ui_end, 2)
        });

        assert!(updates.contains(&Update::Playlists(Vec::new())));
    }

    #[test]
    fn a_failed_connect_is_announced_as_a_library_failure() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Fail, RootlistScript::default());
        let shared = Arc::clone(&host.shared);

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            expect_updates(&ui_end, 2)
        });

        assert!(updates.contains(&Update::SignedIn));
        assert!(updates.contains(&Update::PlaylistFailed(Failure::Load)));
        assert_eq!(shared.rootlists.load(Ordering::SeqCst), 0);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_failed_rootlist_is_announced_as_a_library_failure() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Fail);
        let shared = Arc::clone(&host.shared);

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            expect_updates(&ui_end, 2)
        });

        assert!(updates.contains(&Update::SignedIn));
        assert!(updates.contains(&Update::PlaylistFailed(Failure::Load)));
        assert_eq!(shared.connects.load(Ordering::SeqCst), 1);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn reload_library_runs_the_rootlist_again() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let mut seen = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::ReloadLibrary)
                .expect("reload");
            seen.extend(expect_updates(&ui_end, 1));
            seen
        });

        let listed = updates
            .iter()
            .filter(|update| matches!(update, Update::Playlists(_)))
            .count();
        assert_eq!(listed, 2);
        assert_eq!(shared.connects.load(Ordering::SeqCst), 1);
        assert_eq!(shared.rootlists.load(Ordering::SeqCst), 2);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_second_token_does_not_reconnect() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::with(
            Ok(token_set("access-one", None)),
            Ok(token_set("access-two", Some("refresh-two"))),
        ));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let mut seen = expect_updates(&ui_end, 2);
            let mut sign_ins = seen
                .iter()
                .filter(|update| **update == Update::SignedIn)
                .count();

            ui_end.commands.send(Command::Authorize).expect("authorize");
            while sign_ins < 2 {
                let update = ui_end.updates.recv().expect("update");
                if update == Update::SignedIn {
                    sign_ins += 1;
                }
                seen.push(update);
            }

            ui_end
                .commands
                .send(Command::ReloadLibrary)
                .expect("reload");
            loop {
                let update = ui_end.updates.recv().expect("update");
                let listed = matches!(update, Update::Playlists(_));
                seen.push(update);
                if listed {
                    break;
                }
            }
            seen
        });

        assert_eq!(
            shared.connects.load(Ordering::SeqCst),
            1,
            "a second token must not open a second session"
        );
        assert_eq!(shared.rootlists.load(Ordering::SeqCst), 2);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
        let listed = updates
            .iter()
            .filter(|update| matches!(update, Update::Playlists(_)))
            .count();
        assert_eq!(listed, 2);
    }

    #[test]
    fn a_stop_during_the_rootlist_shuts_down_before_the_task_returns() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Pending);
        let shared = Arc::clone(&host.shared);
        let (started_tx, started_rx) = channel();
        *shared.rootlist_started.lock().expect("lock") = Some(started_tx);

        let runtime = runtime();
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        let window = move || {
            started_rx.recv().expect("rootlist started");
        };
        let teardown = move || async move {
            // A hung stop must fail the test, not hang the suite.
            tokio::time::timeout(Duration::from_secs(1), worker.finish())
                .await
                .expect("finish must resolve when the stop interrupts the rootlist");
            assert!(
                ui_end.commands.is_closed(),
                "the task must have returned before finish resolves"
            );
            assert_eq!(
                shared.shutdowns.load(Ordering::SeqCst),
                1,
                "the shutdown must run when the stop interrupts the rootlist"
            );
        };

        run_ordered(runtime, window, teardown);
    }

    #[test]
    fn a_stop_during_the_connect_shuts_down_before_the_task_returns() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Pending, RootlistScript::default());
        let shared = Arc::clone(&host.shared);
        let (started_tx, started_rx) = channel();
        *shared.connect_started.lock().expect("lock") = Some(started_tx);

        let runtime = runtime();
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        let window = move || {
            started_rx.recv().expect("connect started");
        };
        let teardown = move || async move {
            // A hung stop must fail the test, not hang the suite.
            tokio::time::timeout(Duration::from_secs(1), worker.finish())
                .await
                .expect("finish must resolve when the stop interrupts the connect");
            assert!(
                ui_end.commands.is_closed(),
                "the task must have returned before finish resolves"
            );
            assert_eq!(
                shared.shutdowns.load(Ordering::SeqCst),
                1,
                "the shutdown must run when the stop interrupts the connect"
            );
        };

        run_ordered(runtime, window, teardown);
    }

    #[test]
    fn reload_library_retries_a_failed_connect() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:one", "One")];
        let host = FakeHost::with_connect_scripts(
            vec![ConnectScript::Fail, ConnectScript::Ok],
            RootlistScript::Ready(list.clone()),
        );
        let shared = Arc::clone(&host.shared);

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let mut seen = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::ReloadLibrary)
                .expect("reload");
            seen.extend(expect_updates(&ui_end, 1));
            seen
        });

        assert!(updates.contains(&Update::SignedIn));
        assert!(updates.contains(&Update::PlaylistFailed(Failure::Load)));
        assert!(updates.contains(&Update::Playlists(list)));
        assert_eq!(
            shared.connects.load(Ordering::SeqCst),
            2,
            "a retry after a failed connect must re-enter connect"
        );
        assert_eq!(shared.rootlists.load(Ordering::SeqCst), 1);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn reload_with_no_token_announces_a_library_failure() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::default());
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
        let shared = Arc::clone(&host.shared);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            assert_eq!(
                ui_end.updates.recv().expect("resume update"),
                Update::NeedsAuthorization
            );
            ui_end
                .commands
                .send(Command::ReloadLibrary)
                .expect("reload");
            ui_end.updates.recv().expect("library failure")
        });

        assert_eq!(update, Update::PlaylistFailed(Failure::Load));
        assert_eq!(shared.connects.load(Ordering::SeqCst), 0);
        assert_eq!(shared.rootlists.load(Ordering::SeqCst), 0);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn run_ordered_returns_the_window_result() {
        let result = run_ordered(runtime(), || 42, || async {});
        assert_eq!(result, 42);
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

    #[test]
    fn transport_of_maps_every_event() {
        assert_eq!(
            transport_of(&PlaybackEvent::Loading),
            Update::Transport(TransportUpdate::Loading)
        );
        assert_eq!(
            transport_of(&PlaybackEvent::Playing { position_ms: 5 }),
            Update::Transport(TransportUpdate::Position {
                position_ms: 5,
                paused: false,
            })
        );
        assert_eq!(
            transport_of(&PlaybackEvent::Paused { position_ms: 5 }),
            Update::Transport(TransportUpdate::Position {
                position_ms: 5,
                paused: true,
            })
        );
        assert_eq!(
            transport_of(&PlaybackEvent::Ended),
            Update::Transport(TransportUpdate::Ended)
        );
        assert_eq!(
            transport_of(&PlaybackEvent::Unavailable),
            Update::Transport(TransportUpdate::Unavailable)
        );
        assert_eq!(
            transport_of(&PlaybackEvent::Stopped),
            Update::Transport(TransportUpdate::Stopped)
        );
        assert_eq!(
            transport_of(&PlaybackEvent::Duration {
                duration_ms: 222_000,
            }),
            Update::Transport(TransportUpdate::Duration {
                duration_ms: 222_000,
            })
        );
        assert_eq!(
            transport_of(&PlaybackEvent::Seeked {
                position_ms: 41_000
            }),
            Update::Transport(TransportUpdate::Seeked {
                position_ms: 41_000,
            })
        );
    }

    #[test]
    fn playback_failure_names_a_missing_device() {
        assert_eq!(
            playback_failure(&SessionError::Playback(PlaybackError::NoAudioDevice)),
            Failure::NoAudioDevice
        );
        assert_eq!(
            playback_failure(&SessionError::NotConnected),
            Failure::Other
        );
        assert_eq!(
            playback_failure(&SessionError::Playback(PlaybackError::NoAudioBackend)),
            Failure::Other
        );
    }

    #[test]
    fn announce_sends_tracks_for_a_page() {
        let (tx, rx) = channel();
        let notifier = Notifier {
            tx,
            slot: WakeSlot::default(),
        };
        let playlist = "spotify:playlist:a".to_owned();
        let cover = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
        announce(
            &notifier,
            Ok(TrackPage {
                tracks: vec![track("t")],
                cover: Some(cover.clone()),
                access: None,
            }),
            |page| Update::Tracks {
                playlist: playlist.clone(),
                tracks: page.tracks,
                cover: page.cover,
                access: None,
            },
            |failure| Update::TracksFailed {
                playlist: playlist.clone(),
                failure,
            },
        );
        assert_eq!(
            rx.recv().expect("update"),
            Update::Tracks {
                playlist,
                tracks: vec![track("t")],
                cover: Some(cover),
                access: None,
            }
        );
    }

    #[test]
    fn announce_sends_tracks_failed_for_an_error() {
        let (tx, rx) = channel();
        let notifier = Notifier {
            tx,
            slot: WakeSlot::default(),
        };
        let playlist = "spotify:playlist:a".to_owned();
        let result: Result<TrackPage, SessionError> = Err(SessionError::NotConnected);
        announce(
            &notifier,
            result,
            |page| Update::Tracks {
                playlist: playlist.clone(),
                tracks: page.tracks,
                cover: page.cover,
                access: None,
            },
            |failure| Update::TracksFailed {
                playlist: playlist.clone(),
                failure,
            },
        );
        assert_eq!(
            rx.recv().expect("update"),
            Update::TracksFailed {
                playlist,
                failure: Failure::Load,
            }
        );
    }

    #[test]
    fn a_load_tracks_before_connect_announces_a_failure() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::default());
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
        let shared = Arc::clone(&host.shared);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            assert_eq!(
                ui_end.updates.recv().expect("resume update"),
                Update::NeedsAuthorization
            );
            ui_end
                .commands
                .send(Command::LoadTracks {
                    playlist: "spotify:playlist:a".to_owned(),
                })
                .expect("load");
            ui_end.updates.recv().expect("failure")
        });

        assert_eq!(
            update,
            Update::TracksFailed {
                playlist: "spotify:playlist:a".to_owned(),
                failure: Failure::Load,
            }
        );
        assert_eq!(shared.tracks.load(Ordering::SeqCst), 1);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_load_album_before_connect_announces_a_failure() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::default());
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
        let shared = Arc::clone(&host.shared);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            assert_eq!(
                ui_end.updates.recv().expect("resume update"),
                Update::NeedsAuthorization
            );
            ui_end
                .commands
                .send(Command::LoadAlbum {
                    album: "spotify:album:a".to_owned(),
                })
                .expect("load");
            ui_end.updates.recv().expect("failure")
        });

        assert_eq!(
            update,
            Update::AlbumFailed {
                album: "spotify:album:a".to_owned(),
                failure: Failure::Load,
            }
        );
        assert_eq!(shared.albums.load(Ordering::SeqCst), 1);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    }

    /// Drives one scripted album through the live `load_album` path and returns
    /// the update the window receives, so every hop from the host's answer to the
    /// window's vocabulary is real.
    fn run_album(detail: AlbumDetail) -> Update {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.album_script.lock().expect("lock") = AlbumScript::Ready(detail);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadAlbum {
                    album: "spotify:album:a".to_owned(),
                })
                .expect("load");
            ui_end.updates.recv().expect("album")
        })
    }

    #[test]
    fn a_scripted_album_reaches_the_window() {
        let detail = album_detail("spotify:album:a");
        assert_eq!(run_album(detail.clone()), Update::Album(detail));
    }

    #[test]
    fn a_failed_album_load_is_announced() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.album_script.lock().expect("lock") = AlbumScript::Fail;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadAlbum {
                    album: "spotify:album:a".to_owned(),
                })
                .expect("load");
            ui_end.updates.recv().expect("failure")
        });

        assert_eq!(
            update,
            Update::AlbumFailed {
                album: "spotify:album:a".to_owned(),
                failure: Failure::Load,
            }
        );
        assert_eq!(shared.albums.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_load_show_before_connect_announces_a_failure() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::default());
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
        let shared = Arc::clone(&host.shared);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            assert_eq!(
                ui_end.updates.recv().expect("resume update"),
                Update::NeedsAuthorization
            );
            ui_end
                .commands
                .send(Command::LoadShow {
                    show: SHOW.to_owned(),
                })
                .expect("load");
            ui_end.updates.recv().expect("failure")
        });

        assert_eq!(
            update,
            Update::ShowFailed {
                show: SHOW.to_owned(),
                failure: Failure::Load,
            }
        );
        assert_eq!(shared.shows.load(Ordering::SeqCst), 1);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    }

    /// Drives one scripted show through the live `load_show` path and returns
    /// the update the window receives, so every hop from the host's answer to the
    /// window's vocabulary is real.
    fn run_show(detail: ShowDetail) -> Update {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.show_script.lock().expect("lock") = ShowScript::Ready(detail);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadShow {
                    show: SHOW.to_owned(),
                })
                .expect("load");
            ui_end.updates.recv().expect("show")
        })
    }

    #[test]
    fn a_scripted_show_reaches_the_window() {
        let detail = show_detail(SHOW);
        assert_eq!(run_show(detail.clone()), Update::Show(detail));
    }

    #[test]
    fn a_failed_show_load_is_announced() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.show_script.lock().expect("lock") = ShowScript::Fail;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadShow {
                    show: SHOW.to_owned(),
                })
                .expect("load");
            ui_end.updates.recv().expect("failure")
        });

        assert_eq!(
            update,
            Update::ShowFailed {
                show: SHOW.to_owned(),
                failure: Failure::Load,
            }
        );
        assert_eq!(shared.shows.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_stop_during_a_show_load_shuts_down() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.show_script.lock().expect("lock") = ShowScript::Pending;
        let (started_tx, started_rx) = channel();
        *shared.show_started.lock().expect("lock") = Some(started_tx);

        let runtime = runtime();
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        let window = move || {
            loop {
                if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                    break;
                }
            }
            ui_end
                .commands
                .send(Command::LoadShow {
                    show: SHOW.to_owned(),
                })
                .expect("load");
            started_rx.recv().expect("show started");
        };
        let teardown = move || async move {
            // A hung stop must fail the test, not hang the suite.
            tokio::time::timeout(Duration::from_secs(1), worker.finish())
                .await
                .expect("finish must resolve when the stop interrupts the show load");
            assert_eq!(
                shared.shutdowns.load(Ordering::SeqCst),
                1,
                "the shutdown must run when the stop interrupts the show load"
            );
        };

        run_ordered(runtime, window, teardown);
    }

    #[test]
    fn a_followed_load_before_connect_announces_followed_failure() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::default());
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
        let shared = Arc::clone(&host.shared);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            assert_eq!(
                ui_end.updates.recv().expect("resume update"),
                Update::NeedsAuthorization
            );
            ui_end
                .commands
                .send(Command::LoadFollowedShows)
                .expect("load");
            ui_end.updates.recv().expect("failure")
        });

        assert_eq!(
            update,
            Update::FollowedShowsFailed(Failure::Followed),
            "a followed failure is not the library's"
        );
        assert_eq!(shared.followed_lists.load(Ordering::SeqCst), 1);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    }

    /// Drives one scripted followed list through the live `load_followed_shows`
    /// path and returns the update the window receives.
    fn run_followed(shows: Vec<FollowedShow>) -> Update {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.followed_script.lock().expect("lock") = FollowedScript::Ready(shows);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadFollowedShows)
                .expect("load");
            ui_end.updates.recv().expect("followed")
        })
    }

    #[test]
    fn a_scripted_followed_list_reaches_the_window() {
        let shows = vec![
            followed_show("spotify:show:a"),
            followed_show("spotify:show:b"),
        ];
        assert_eq!(run_followed(shows.clone()), Update::FollowedShows(shows));
    }

    #[test]
    fn a_failed_followed_load_is_announced_as_followed() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.followed_script.lock().expect("lock") = FollowedScript::Fail;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadFollowedShows)
                .expect("load");
            ui_end.updates.recv().expect("failure")
        });

        assert_eq!(update, Update::FollowedShowsFailed(Failure::Followed));
        assert_eq!(shared.followed_lists.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_signed_in_window_gets_no_followed_update_unprompted() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            assert_eq!(
                ui_end.updates.recv_timeout(Duration::from_millis(250)),
                Err(RecvTimeoutError::Timeout),
                "the worker must not load the followed list on its own"
            );
        });
        assert_eq!(
            shared.followed_lists.load(Ordering::SeqCst),
            0,
            "the list is want-driven, never chained onto the rootlist"
        );
    }

    #[test]
    fn a_followed_artists_load_before_connect_announces_followed_artists_failure() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::default());
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
        let shared = Arc::clone(&host.shared);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            assert_eq!(
                ui_end.updates.recv().expect("resume update"),
                Update::NeedsAuthorization
            );
            ui_end
                .commands
                .send(Command::LoadFollowedArtists)
                .expect("load");
            ui_end.updates.recv().expect("failure")
        });

        assert_eq!(
            update,
            Update::FollowedArtistsFailed(Failure::FollowedArtists),
            "a followed-artists failure is not the library's"
        );
        assert_eq!(shared.followed_artist_lists.load(Ordering::SeqCst), 1);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    }

    /// Drives one scripted followed-artist list through the live
    /// `load_followed_artists` path and returns the update the window receives.
    fn run_followed_artists(artists: Vec<FollowedArtist>) -> Update {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.followed_artists_script.lock().expect("lock") =
            FollowedArtistsScript::Ready(artists);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadFollowedArtists)
                .expect("load");
            ui_end.updates.recv().expect("followed artists")
        })
    }

    #[test]
    fn a_scripted_followed_artist_list_reaches_the_window() {
        let artists = vec![
            followed_artist("spotify:artist:a"),
            followed_artist("spotify:artist:b"),
        ];
        assert_eq!(
            run_followed_artists(artists.clone()),
            Update::FollowedArtists(artists)
        );
    }

    #[test]
    fn a_failed_followed_artists_load_is_announced_as_followed_artists() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.followed_artists_script.lock().expect("lock") = FollowedArtistsScript::Fail;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadFollowedArtists)
                .expect("load");
            ui_end.updates.recv().expect("failure")
        });

        assert_eq!(
            update,
            Update::FollowedArtistsFailed(Failure::FollowedArtists)
        );
        assert_eq!(shared.followed_artist_lists.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_signed_in_window_gets_no_followed_artist_update_unprompted() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            assert_eq!(
                ui_end.updates.recv_timeout(Duration::from_millis(250)),
                Err(RecvTimeoutError::Timeout),
                "the worker must not load the followed-artist list on its own"
            );
        });
        assert_eq!(
            shared.followed_artist_lists.load(Ordering::SeqCst),
            0,
            "the list is want-driven, never chained onto the rootlist"
        );
    }

    #[test]
    fn a_saved_albums_load_before_connect_announces_saved_albums_failure() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::default());
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
        let shared = Arc::clone(&host.shared);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            assert_eq!(
                ui_end.updates.recv().expect("resume update"),
                Update::NeedsAuthorization
            );
            ui_end
                .commands
                .send(Command::LoadSavedAlbums)
                .expect("load");
            ui_end.updates.recv().expect("failure")
        });

        assert_eq!(
            update,
            Update::SavedAlbumsFailed(Failure::SavedAlbums),
            "a saved-albums failure is not the library's"
        );
        assert_eq!(shared.saved_album_lists.load(Ordering::SeqCst), 1);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    }

    /// Drives one scripted saved-album list through the live `load_saved_albums`
    /// path and returns the update the window receives.
    fn run_saved_albums(albums: Vec<SavedAlbum>) -> Update {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.saved_albums_script.lock().expect("lock") = SavedAlbumsScript::Ready(albums);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadSavedAlbums)
                .expect("load");
            ui_end.updates.recv().expect("saved albums")
        })
    }

    #[test]
    fn a_scripted_saved_album_list_reaches_the_window() {
        let albums = vec![
            saved_album("spotify:album:a"),
            saved_album("spotify:album:b"),
        ];
        assert_eq!(
            run_saved_albums(albums.clone()),
            Update::SavedAlbums(albums)
        );
    }

    #[test]
    fn a_failed_saved_albums_load_is_announced_as_saved_albums() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.saved_albums_script.lock().expect("lock") = SavedAlbumsScript::Fail;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadSavedAlbums)
                .expect("load");
            ui_end.updates.recv().expect("failure")
        });

        assert_eq!(update, Update::SavedAlbumsFailed(Failure::SavedAlbums));
        assert_eq!(shared.saved_album_lists.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_signed_in_window_gets_no_saved_album_update_unprompted() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            assert_eq!(
                ui_end.updates.recv_timeout(Duration::from_millis(250)),
                Err(RecvTimeoutError::Timeout),
                "the worker must not load the saved-album list on its own"
            );
        });
        assert_eq!(
            shared.saved_album_lists.load(Ordering::SeqCst),
            0,
            "the list is want-driven, never chained onto the rootlist"
        );
    }

    /// Runs one liked-tracks command through the live path and returns the
    /// update the window receives.
    fn run_liked(script: LikedScript) -> Update {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.liked_script.lock().expect("lock") = script;

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadLikedTracks)
                .expect("load");
            ui_end.updates.recv().expect("liked tracks")
        })
    }

    #[test]
    fn loading_the_liked_tracks_announces_the_whole_set() {
        assert_eq!(
            run_liked(LikedScript::Ready(vec![TRACK.to_owned()])),
            Update::LikedTracks(vec![TRACK.to_owned()])
        );
    }

    #[test]
    fn a_liked_tracks_failure_announces_the_failed_update() {
        assert_eq!(
            run_liked(LikedScript::Fail),
            Update::LikedTracksFailed(Failure::Load),
            "a liked read has no surface of its own, so its failure is the library's"
        );
    }

    /// Runs one like write through the live path, with the scripted answer.
    fn run_like(script: LikeScript, liked: bool) -> Update {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.like_script.lock().expect("lock") = script;

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::SetLiked {
                    tracks: vec![TRACK.to_owned()],
                    liked,
                })
                .expect("like");
            ui_end.updates.recv().expect("like answer")
        })
    }

    #[test]
    fn a_like_announces_the_requested_tracks_and_value_on_success() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::SetLiked {
                    tracks: vec![TRACK.to_owned()],
                    liked: true,
                })
                .expect("like");
            ui_end.updates.recv().expect("liked")
        });

        assert_eq!(
            update,
            Update::Liked {
                tracks: vec![TRACK.to_owned()],
                liked: true,
            }
        );
        assert_eq!(shared.like_writes.load(Ordering::SeqCst), 1);
        assert_eq!(
            shared.last_like.lock().expect("lock").clone(),
            Some((vec![TRACK.to_owned()], true)),
            "the write is sent with the pick the user made"
        );
    }

    #[test]
    fn a_like_failure_announces_like_failed_with_save() {
        assert_eq!(
            run_like(LikeScript::Fail, false),
            Update::LikeFailed {
                tracks: vec![TRACK.to_owned()],
                failure: Failure::Save,
            }
        );
    }

    #[test]
    fn a_stop_during_a_like_write_ends_the_task() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.like_script.lock().expect("lock") = LikeScript::Pending;
        let (started_tx, started_rx) = channel();
        *shared.like_started.lock().expect("lock") = Some(started_tx);

        let runtime = runtime();
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        let window = move || {
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::SetLiked {
                    tracks: vec![TRACK.to_owned()],
                    liked: true,
                })
                .expect("like");
            started_rx.recv().expect("like started");
        };
        let teardown = move || async move {
            tokio::time::timeout(Duration::from_secs(1), worker.finish())
                .await
                .expect("finish must resolve when the stop interrupts the like write");
            assert_eq!(
                shared.shutdowns.load(Ordering::SeqCst),
                1,
                "the shutdown must run when the stop interrupts the like write"
            );
        };

        run_ordered(runtime, window, teardown);
    }

    #[test]
    fn a_scripted_artist_announces_the_page_then_the_followed_state() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        let detail = artist_detail(ARTIST);
        *shared.artist_script.lock().expect("lock") = ArtistScript::Ready(ArtistPage {
            artist: detail.clone(),
            followed: Some(true),
        });

        let (first, second) = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadArtist {
                    artist: ARTIST.to_owned(),
                })
                .expect("load");
            let first = ui_end.updates.recv().expect("artist");
            let second = ui_end.updates.recv().expect("followed");
            (first, second)
        });

        assert_eq!(first, Update::Artist(detail));
        assert_eq!(
            second,
            Update::Followed {
                artist: ARTIST.to_owned(),
                followed: true,
            },
            "the page is announced first, then what the server said about it"
        );
    }

    #[test]
    fn an_artist_without_a_probe_answer_announces_no_followed_state() {
        let detail = artist_detail(ARTIST);
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.artist_script.lock().expect("lock") = ArtistScript::Ready(ArtistPage {
            artist: detail.clone(),
            followed: None,
        });

        let (first, second) = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadArtist {
                    artist: ARTIST.to_owned(),
                })
                .expect("load");
            let first = ui_end.updates.recv().expect("artist");
            ui_end
                .commands
                .send(Command::LoadReleaseTracks {
                    release: "spotify:album:r".to_owned(),
                    tracks: Vec::new(),
                })
                .expect("labels");
            let second = ui_end.updates.recv().expect("labels");
            (first, second)
        });

        assert_eq!(first, Update::Artist(detail));
        assert_eq!(
            second,
            Update::ReleaseTracks {
                release: "spotify:album:r".to_owned(),
                tracks: Vec::new(),
            },
            "no Followed state was queued behind the artist"
        );
    }

    #[test]
    fn a_failed_artist_load_is_announced() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.artist_script.lock().expect("lock") = ArtistScript::Fail;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadArtist {
                    artist: ARTIST.to_owned(),
                })
                .expect("load");
            ui_end.updates.recv().expect("failure")
        });

        assert_eq!(
            update,
            Update::ArtistFailed {
                artist: ARTIST.to_owned(),
                failure: Failure::Load,
            }
        );
        assert_eq!(shared.artists.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_scripted_lyrics_load_reaches_the_window() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        let lyrics = lyrics_of("first line");
        *shared.lyrics_script.lock().expect("lock") = LyricsScript::Ready(lyrics.clone());

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadLyrics {
                    track: TRACK.to_owned(),
                })
                .expect("load");
            ui_end.updates.recv().expect("lyrics")
        });

        assert_eq!(
            update,
            Update::Lyrics {
                track: TRACK.to_owned(),
                lyrics,
            }
        );
        assert_eq!(shared.lyrics_loads.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_failed_lyrics_load_is_announced() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.lyrics_script.lock().expect("lock") = LyricsScript::Fail;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadLyrics {
                    track: TRACK.to_owned(),
                })
                .expect("load");
            ui_end.updates.recv().expect("failure")
        });

        assert_eq!(
            update,
            Update::LyricsFailed {
                track: TRACK.to_owned(),
                failure: Failure::Lyrics,
            }
        );
        assert_eq!(shared.lyrics_loads.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_stop_during_a_lyrics_load_shuts_down() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.lyrics_script.lock().expect("lock") = LyricsScript::Pending;
        let (started_tx, started_rx) = channel();
        *shared.lyrics_started.lock().expect("lock") = Some(started_tx);

        let runtime = runtime();
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        let window = move || {
            loop {
                if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                    break;
                }
            }
            ui_end
                .commands
                .send(Command::LoadLyrics {
                    track: TRACK.to_owned(),
                })
                .expect("load");
            started_rx.recv().expect("lyrics started");
        };
        let teardown = move || async move {
            // A hung stop must fail the test, not hang the suite.
            tokio::time::timeout(Duration::from_secs(1), worker.finish())
                .await
                .expect("finish must resolve when the stop interrupts the lyrics load");
            assert_eq!(
                shared.shutdowns.load(Ordering::SeqCst),
                1,
                "the shutdown must run when the stop interrupts the lyrics load"
            );
        };

        run_ordered(runtime, window, teardown);
    }

    fn portrait(artist: &str) -> ArtistPortrait {
        ArtistPortrait {
            artist: artist.to_owned(),
            portrait: CoverUrl::from_wire(&format!("https://i.scdn.co/image/{}", "ab".repeat(20))),
        }
    }

    #[test]
    fn a_scripted_portrait_read_echoes_the_query_and_the_list() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        let portraits = vec![portrait("spotify:artist:a"), portrait("spotify:artist:b")];
        *shared.portrait_script.lock().expect("lock") = PortraitScript::Ready(portraits.clone());
        let query = "slow  tide";
        let artists = vec!["spotify:artist:a".to_owned(), "spotify:artist:b".to_owned()];

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadPortraits {
                    query: query.to_owned(),
                    artists: artists.clone(),
                })
                .expect("portraits");
            ui_end.updates.recv().expect("portraits")
        });

        assert_eq!(
            update,
            Update::Portraits {
                query: query.to_owned(),
                portraits,
            }
        );
        assert_eq!(shared.portrait_reads.load(Ordering::SeqCst), 1);
        assert_eq!(
            *shared.portraited.lock().expect("lock"),
            vec![artists],
            "the host must be asked for exactly the artists sent"
        );
    }

    #[test]
    fn a_failed_portrait_read_is_announced_silently() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.portrait_script.lock().expect("lock") = PortraitScript::Fail;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadPortraits {
                    query: "slow tide".to_owned(),
                    artists: vec!["spotify:artist:a".to_owned()],
                })
                .expect("portraits");
            ui_end.updates.recv().expect("failure")
        });

        assert_eq!(
            update,
            Update::PortraitsFailed {
                query: "slow tide".to_owned(),
            },
            "a failed portrait read carries no failure word"
        );
    }

    #[test]
    fn a_portrait_read_before_the_session_connects_is_announced_as_silent() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Fail, RootlistScript::default());
        let shared = Arc::clone(&host.shared);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadPortraits {
                    query: "slow tide".to_owned(),
                    artists: vec!["spotify:artist:a".to_owned()],
                })
                .expect("portraits");
            ui_end.updates.recv().expect("failure")
        });

        assert_eq!(
            update,
            Update::PortraitsFailed {
                query: "slow tide".to_owned(),
            }
        );
        assert_eq!(shared.portrait_reads.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_stop_during_a_portrait_read_shuts_down() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.portrait_script.lock().expect("lock") = PortraitScript::Pending;
        let (started_tx, started_rx) = channel();
        *shared.portrait_started.lock().expect("lock") = Some(started_tx);

        let runtime = runtime();
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        let window = move || {
            loop {
                if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                    break;
                }
            }
            ui_end
                .commands
                .send(Command::LoadPortraits {
                    query: "slow tide".to_owned(),
                    artists: vec!["spotify:artist:a".to_owned()],
                })
                .expect("portraits");
            started_rx.recv().expect("portraits started");
        };
        let teardown = move || async move {
            // A hung stop must fail the test, not hang the suite.
            tokio::time::timeout(Duration::from_secs(1), worker.finish())
                .await
                .expect("finish must resolve when the stop interrupts the portrait read");
            assert_eq!(
                shared.shutdowns.load(Ordering::SeqCst),
                1,
                "the shutdown must run when the stop interrupts the portrait read"
            );
        };

        run_ordered(runtime, window, teardown);
    }

    #[test]
    fn two_portrait_reads_queued_together_echo_their_own_queries() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        let portraits = vec![portrait("spotify:artist:a")];
        *shared.portrait_script.lock().expect("lock") = PortraitScript::Ready(portraits.clone());

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            for query in ["first", "second"] {
                ui_end
                    .commands
                    .send(Command::LoadPortraits {
                        query: query.to_owned(),
                        artists: vec!["spotify:artist:a".to_owned()],
                    })
                    .expect("portraits");
            }
            let mut received = Vec::new();
            for _ in 0..2 {
                received.push(ui_end.updates.recv().expect("portraits"));
            }
            received
        });

        assert_eq!(
            updates,
            vec![
                Update::Portraits {
                    query: "first".to_owned(),
                    portraits: portraits.clone(),
                },
                Update::Portraits {
                    query: "second".to_owned(),
                    portraits,
                },
            ]
        );
        assert_eq!(
            *shared.portraited.lock().expect("lock"),
            vec![
                vec!["spotify:artist:a".to_owned()],
                vec!["spotify:artist:a".to_owned()],
            ],
            "each read asks for its own artists, in the order sent"
        );
    }

    #[test]
    fn a_scripted_search_echoes_the_query_it_was_asked() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        let tracks = vec![track("spotify:track:a")];
        *shared.search_script.lock().expect("lock") = SearchScript::Ready(tracks.clone());
        // An interior double space: the echo must be the command's string
        // byte for byte, because the window clears its in-flight marker on a
        // match and would otherwise never ask again.
        let query = "never  gonna";

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::Search {
                    query: query.to_owned(),
                })
                .expect("search");
            ui_end.updates.recv().expect("results")
        });

        assert_eq!(
            update,
            Update::Search {
                query: query.to_owned(),
                tracks,
            }
        );
        assert_eq!(shared.searches.load(Ordering::SeqCst), 1);
        assert_eq!(
            *shared.searched.lock().expect("lock"),
            vec![query.to_owned()],
            "the worker must ask for the query the window sent"
        );
    }

    #[test]
    fn a_failed_search_is_announced() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.search_script.lock().expect("lock") = SearchScript::Fail;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::Search {
                    query: "never gonna".to_owned(),
                })
                .expect("search");
            ui_end.updates.recv().expect("failure")
        });

        assert_eq!(
            update,
            Update::SearchFailed {
                query: "never gonna".to_owned(),
                failure: Failure::Search,
            }
        );
        assert_eq!(shared.searches.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_stop_during_a_search_shuts_down() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.search_script.lock().expect("lock") = SearchScript::Pending;
        let (started_tx, started_rx) = channel();
        *shared.search_started.lock().expect("lock") = Some(started_tx);

        let runtime = runtime();
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        let window = move || {
            loop {
                if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                    break;
                }
            }
            ui_end
                .commands
                .send(Command::Search {
                    query: "never gonna".to_owned(),
                })
                .expect("search");
            started_rx.recv().expect("search started");
        };
        let teardown = move || async move {
            // A hung stop must fail the test, not hang the suite.
            tokio::time::timeout(Duration::from_secs(1), worker.finish())
                .await
                .expect("finish must resolve when the stop interrupts the search");
            assert_eq!(
                shared.shutdowns.load(Ordering::SeqCst),
                1,
                "the shutdown must run when the stop interrupts the search"
            );
        };

        run_ordered(runtime, window, teardown);
    }

    #[test]
    fn two_back_to_back_searches_announce_their_own_queries() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        let tracks = vec![track("spotify:track:a")];
        *shared.search_script.lock().expect("lock") = SearchScript::Ready(tracks.clone());

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            let mut received = Vec::new();
            for query in ["first", "second"] {
                ui_end
                    .commands
                    .send(Command::Search {
                        query: query.to_owned(),
                    })
                    .expect("search");
                received.push(ui_end.updates.recv().expect("results"));
            }
            received
        });

        assert_eq!(
            updates,
            vec![
                Update::Search {
                    query: "first".to_owned(),
                    tracks: tracks.clone(),
                },
                Update::Search {
                    query: "second".to_owned(),
                    tracks,
                },
            ]
        );
        assert_eq!(
            *shared.searched.lock().expect("lock"),
            vec!["first".to_owned(), "second".to_owned()],
            "the two reads must be asked for in the order they were sent"
        );
    }

    #[test]
    fn two_searches_queued_before_either_reply_announce_their_own_queries() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        let tracks = vec![track("spotify:track:a")];
        *shared.search_script.lock().expect("lock") = SearchScript::Ready(tracks.clone());

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            // Both reads are queued before either answer is read: a window
            // that is typing does not wait for the first reply to ask again.
            for query in ["first", "second"] {
                ui_end
                    .commands
                    .send(Command::Search {
                        query: query.to_owned(),
                    })
                    .expect("search");
            }
            let mut received = Vec::new();
            for _ in 0..2 {
                received.push(ui_end.updates.recv().expect("results"));
            }
            received
        });

        assert_eq!(
            updates,
            vec![
                Update::Search {
                    query: "first".to_owned(),
                    tracks: tracks.clone(),
                },
                Update::Search {
                    query: "second".to_owned(),
                    tracks,
                },
            ]
        );
        assert_eq!(
            *shared.searched.lock().expect("lock"),
            vec!["first".to_owned(), "second".to_owned()],
            "the queued reads are asked for in the order they were sent"
        );
    }

    #[test]
    fn a_scripted_release_announces_its_labelled_tracks() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.release_script.lock().expect("lock") =
            ReleaseScript::Named(vec![track("spotify:track:a")]);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadReleaseTracks {
                    release: "spotify:album:r".to_owned(),
                    tracks: vec!["spotify:track:a".to_owned()],
                })
                .expect("labels");
            ui_end.updates.recv().expect("tracks")
        });

        assert_eq!(
            update,
            Update::ReleaseTracks {
                release: "spotify:album:r".to_owned(),
                tracks: vec![track("spotify:track:a")],
            }
        );
        assert_eq!(shared.releases.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_failed_release_load_is_announced() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.release_script.lock().expect("lock") = ReleaseScript::Fail;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadReleaseTracks {
                    release: "spotify:album:r".to_owned(),
                    tracks: Vec::new(),
                })
                .expect("labels");
            ui_end.updates.recv().expect("failure")
        });

        assert_eq!(
            update,
            Update::ReleaseTracksFailed {
                release: "spotify:album:r".to_owned(),
                failure: Failure::Load,
            }
        );
    }

    #[test]
    fn a_set_follow_announces_the_requested_value() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::SetFollow {
                    artist: ARTIST.to_owned(),
                    followed: true,
                })
                .expect("follow");
            ui_end.updates.recv().expect("followed")
        });

        assert_eq!(
            update,
            Update::Followed {
                artist: ARTIST.to_owned(),
                followed: true,
            }
        );
        assert_eq!(shared.follow_writes.load(Ordering::SeqCst), 1);
        assert_eq!(
            shared.last_follow.lock().expect("lock").clone(),
            Some((ARTIST.to_owned(), true)),
            "the write is sent with the state the user asked for"
        );
    }

    #[test]
    fn a_failed_follow_write_is_announced_as_a_save_failure() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.follow_script.lock().expect("lock") = FollowScript::Fail;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::SetFollow {
                    artist: ARTIST.to_owned(),
                    followed: false,
                })
                .expect("follow");
            ui_end.updates.recv().expect("follow failed")
        });

        assert_eq!(
            update,
            Update::FollowFailed {
                artist: ARTIST.to_owned(),
                failure: Failure::Save,
            }
        );
    }

    #[test]
    fn a_move_announces_the_reply_revision_and_sends_what_was_asked() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        let base = revision(1);
        let reply = revision(2);
        *shared.move_script.lock().expect("lock") = MoveScript::Ready(reply.clone());

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::MoveTrack {
                    playlist: "spotify:playlist:a".to_owned(),
                    revision: base.clone(),
                    moves: vec![Move {
                        from: 1,
                        length: 1,
                        to: 4,
                    }],
                })
                .expect("move");
            ui_end.updates.recv().expect("moved")
        });

        assert_eq!(
            update,
            Update::TrackMoved {
                playlist: "spotify:playlist:a".to_owned(),
                revision: reply,
            }
        );
        assert_eq!(shared.move_writes.load(Ordering::SeqCst), 1);
        assert_eq!(
            shared.last_move.lock().expect("lock").clone(),
            Some((
                "spotify:playlist:a".to_owned(),
                base,
                vec![Move {
                    from: 1,
                    length: 1,
                    to: 4,
                }],
            )),
            "the write is sent with the revision and the indices that were asked for"
        );
    }

    #[test]
    fn a_two_move_command_is_one_write_and_one_revision() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        let base = revision(1);
        let reply = revision(2);
        *shared.move_script.lock().expect("lock") = MoveScript::Ready(reply.clone());
        let moves = vec![
            Move {
                from: 0,
                length: 1,
                to: 4,
            },
            Move {
                from: 1,
                length: 1,
                to: 4,
            },
        ];

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::MoveTrack {
                    playlist: "spotify:playlist:a".to_owned(),
                    revision: base.clone(),
                    moves: moves.clone(),
                })
                .expect("move");
            ui_end.updates.recv().expect("moved")
        });

        assert_eq!(
            update,
            Update::TrackMoved {
                playlist: "spotify:playlist:a".to_owned(),
                revision: reply,
            }
        );
        assert_eq!(shared.move_writes.load(Ordering::SeqCst), 1);
        assert_eq!(
            shared.last_move.lock().expect("lock").clone(),
            Some(("spotify:playlist:a".to_owned(), base, moves)),
            "both moves are one write with one revision"
        );
    }

    #[test]
    fn a_removal_announces_the_reply_revision_and_sends_what_was_asked() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        let base = revision(1);
        let reply = revision(2);
        *shared.remove_script.lock().expect("lock") = MoveScript::Ready(reply.clone());
        let removals = vec![
            Removal { from: 5, length: 1 },
            Removal { from: 3, length: 1 },
        ];

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::RemoveTracks {
                    playlist: "spotify:playlist:a".to_owned(),
                    revision: base.clone(),
                    removals: removals.clone(),
                })
                .expect("remove");
            ui_end.updates.recv().expect("removed")
        });

        assert_eq!(
            update,
            Update::TracksRemoved {
                playlist: "spotify:playlist:a".to_owned(),
                revision: reply,
            }
        );
        assert_eq!(shared.remove_writes.load(Ordering::SeqCst), 1);
        assert_eq!(
            shared.last_remove.lock().expect("lock").clone(),
            Some(("spotify:playlist:a".to_owned(), base, removals)),
            "the write is sent with the revision and the chain that was asked for"
        );
    }

    #[test]
    fn a_removal_failure_announces_remove_failed() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.remove_script.lock().expect("lock") = MoveScript::Fail;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::RemoveTracks {
                    playlist: "spotify:playlist:a".to_owned(),
                    revision: revision(1),
                    removals: vec![Removal { from: 0, length: 1 }],
                })
                .expect("remove");
            ui_end.updates.recv().expect("remove failed")
        });

        assert_eq!(
            update,
            Update::RemoveFailed {
                playlist: "spotify:playlist:a".to_owned(),
                failure: Failure::Save,
            }
        );
        assert_eq!(shared.remove_writes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_two_run_removal_is_one_write() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.remove_script.lock().expect("lock") = MoveScript::Ready(revision(2));

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::RemoveTracks {
                    playlist: "spotify:playlist:a".to_owned(),
                    revision: revision(1),
                    removals: vec![
                        Removal { from: 6, length: 2 },
                        Removal { from: 1, length: 1 },
                    ],
                })
                .expect("remove");
            ui_end.updates.recv().expect("removed")
        });

        assert!(matches!(update, Update::TracksRemoved { .. }));
        assert_eq!(
            shared.remove_writes.load(Ordering::SeqCst),
            1,
            "a gapped pick is one change set, not one write per run"
        );
        assert_eq!(
            shared
                .last_remove
                .lock()
                .expect("lock")
                .as_ref()
                .map(|(_, _, removals)| removals.len()),
            Some(2)
        );
    }

    #[test]
    fn the_edge_steps_encode_the_gap_the_server_reads() {
        let base = revision(1);
        let write = |len: usize, from: usize, boundary: usize| {
            let found = takofy_ui::reorder::step(len, from, boundary).expect("a move");
            takofy_core::playlist_edit::move_body(
                &base,
                &[Move {
                    from: found.from,
                    length: 1,
                    to: found.to,
                }],
            )
        };

        // The server reads `to` as a gap in the list before the move: moving down
        // has to name the gap after the row, or the move lands one short and a
        // one-place nudge reads as a move onto itself.
        assert!(write(5, 0, 2).ends_with(&[0x08, 0x00, 0x10, 0x01, 0x18, 0x02]));
        assert!(write(5, 0, 5).ends_with(&[0x08, 0x00, 0x10, 0x01, 0x18, 0x05]));
        assert!(write(5, 3, 0).ends_with(&[0x08, 0x03, 0x10, 0x01, 0x18, 0x00]));
        assert!(write(5, 2, 5).ends_with(&[0x08, 0x02, 0x10, 0x01, 0x18, 0x05]));
    }
    #[test]
    fn a_failed_move_is_a_save_failure() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.move_script.lock().expect("lock") = MoveScript::Fail;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::MoveTrack {
                    playlist: "spotify:playlist:a".to_owned(),
                    revision: revision(1),
                    moves: vec![Move {
                        from: 0,
                        length: 1,
                        to: 1,
                    }],
                })
                .expect("move");
            ui_end.updates.recv().expect("move failed")
        });

        assert_eq!(
            update,
            Update::MoveFailed {
                playlist: "spotify:playlist:a".to_owned(),
                failure: Failure::Save,
            }
        );
        assert_eq!(shared.move_writes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_move_before_connect_is_announced_as_a_failure() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::default());
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
        let shared = Arc::clone(&host.shared);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            assert_eq!(
                ui_end.updates.recv().expect("resume update"),
                Update::NeedsAuthorization
            );
            ui_end
                .commands
                .send(Command::MoveTrack {
                    playlist: "spotify:playlist:a".to_owned(),
                    revision: revision(1),
                    moves: vec![Move {
                        from: 0,
                        length: 1,
                        to: 1,
                    }],
                })
                .expect("move");
            ui_end.updates.recv().expect("move failed")
        });

        assert_eq!(
            update,
            Update::MoveFailed {
                playlist: "spotify:playlist:a".to_owned(),
                failure: Failure::Save,
            }
        );
        assert_eq!(shared.move_writes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_stop_with_the_move_pending_shuts_down() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.move_script.lock().expect("lock") = MoveScript::Pending;
        let (started_tx, started_rx) = channel();
        *shared.move_started.lock().expect("lock") = Some(started_tx);

        let runtime = runtime();
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        let window = move || {
            loop {
                if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                    break;
                }
            }
            ui_end
                .commands
                .send(Command::MoveTrack {
                    playlist: "spotify:playlist:a".to_owned(),
                    revision: revision(1),
                    moves: vec![Move {
                        from: 0,
                        length: 1,
                        to: 1,
                    }],
                })
                .expect("move");
            started_rx.recv().expect("move started");
        };
        let teardown = move || async move {
            // A hung stop must fail the test, not hang the suite.
            tokio::time::timeout(Duration::from_secs(1), worker.finish())
                .await
                .expect("finish must resolve when the stop interrupts the move");
            assert_eq!(
                shared.shutdowns.load(Ordering::SeqCst),
                1,
                "the shutdown must run when the stop interrupts the move"
            );
        };

        run_ordered(runtime, window, teardown);
    }

    #[test]
    fn replies_keep_the_order_their_commands_were_sent_in() {
        // the page's read rule rests on this order
        fn run(move_first: bool) -> Vec<Update> {
            let store = Arc::new(MemoryStore::seeded("stored"));
            let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
            let list = vec![playlist("spotify:playlist:a", "A")];
            let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
            let shared = Arc::clone(&host.shared);
            *shared.move_script.lock().expect("lock") = MoveScript::Ready(revision(2));

            run_window(runtime(), host, store, source, |ui_end, slot| {
                slot.install(Arc::new(RecordingWaker::default()));
                let _ = expect_updates(&ui_end, 2);
                let move_command = Command::MoveTrack {
                    playlist: "spotify:playlist:a".to_owned(),
                    revision: revision(1),
                    moves: vec![Move {
                        from: 0,
                        length: 1,
                        to: 1,
                    }],
                };
                let load_command = Command::LoadTracks {
                    playlist: "spotify:playlist:a".to_owned(),
                };
                if move_first {
                    ui_end.commands.send(move_command).expect("move");
                    ui_end.commands.send(load_command).expect("load");
                } else {
                    ui_end.commands.send(load_command).expect("load");
                    ui_end.commands.send(move_command).expect("move");
                }
                vec![
                    ui_end.updates.recv().expect("first reply"),
                    ui_end.updates.recv().expect("second reply"),
                ]
            })
        }

        fn moved(update: &Update) -> bool {
            matches!(update, Update::TrackMoved { .. })
        }

        fn read(update: &Update) -> bool {
            matches!(update, Update::Tracks { .. })
        }

        let move_first = run(true);
        assert!(
            moved(&move_first[0]) && read(&move_first[1]),
            "a move sent first must answer first: {move_first:?}"
        );

        let read_first = run(false);
        assert!(
            read(&read_first[0]) && moved(&read_first[1]),
            "a read sent first must answer first: {read_first:?}"
        );
    }

    #[test]
    fn a_failed_track_load_is_announced_and_the_task_shuts_down() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.tracks_script.lock().expect("lock") = TracksScript::Fail;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadTracks {
                    playlist: "spotify:playlist:a".to_owned(),
                })
                .expect("load");
            ui_end.updates.recv().expect("failure")
        });

        assert_eq!(
            update,
            Update::TracksFailed {
                playlist: "spotify:playlist:a".to_owned(),
                failure: Failure::Load,
            }
        );
        assert_eq!(shared.tracks.load(Ordering::SeqCst), 1);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    }

    /// Drives one scripted track page through the live `load_tracks` path and
    /// returns the `Update::Tracks` the window receives. This is the end-to-end
    /// guard the direct-`Update` tests cannot give: every hop from the host's
    /// page to the update is real.
    fn run_track_page(page: TrackPage) -> Update {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.tracks_script.lock().expect("lock") = TracksScript::Ready(page);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LoadTracks {
                    playlist: "spotify:playlist:a".to_owned(),
                })
                .expect("load");
            ui_end.updates.recv().expect("tracks")
        })
    }

    #[test]
    fn a_track_page_cover_reaches_the_update() {
        let cover = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
        let update = run_track_page(TrackPage {
            tracks: vec![track("t")],
            cover: Some(cover.clone()),
            access: None,
        });
        assert_eq!(
            update,
            Update::Tracks {
                playlist: "spotify:playlist:a".to_owned(),
                tracks: vec![track("t")],
                cover: Some(cover),
                access: None,
            }
        );
    }

    #[test]
    fn a_track_page_without_a_cover_reaches_the_update_as_none() {
        let update = run_track_page(TrackPage {
            tracks: vec![track("t")],
            cover: None,
            access: None,
        });
        assert_eq!(
            update,
            Update::Tracks {
                playlist: "spotify:playlist:a".to_owned(),
                tracks: vec![track("t")],
                cover: None,
                access: None,
            }
        );
    }

    #[test]
    fn a_track_page_access_reaches_the_update() {
        let access = PlaylistAccess {
            revision: revision(1),
            editable: true,
        };
        let update = run_track_page(TrackPage {
            tracks: vec![track("t")],
            cover: None,
            access: Some(access.clone()),
        });
        assert_eq!(
            update,
            Update::Tracks {
                playlist: "spotify:playlist:a".to_owned(),
                tracks: vec![track("t")],
                cover: None,
                access: Some(access),
            }
        );
    }

    #[test]
    fn a_fetched_cover_decodes_and_reaches_the_window() {
        let runtime = runtime();
        let (tx, rx) = channel();
        let notifier = Notifier {
            tx,
            slot: WakeSlot::default(),
        };
        let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
        let in_flight = Arc::new(Mutex::new(HashSet::new()));

        let update = runtime.block_on(async {
            let mut tasks = JoinSet::new();
            spawn_cover(
                &mut tasks,
                Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS)),
                Arc::clone(&in_flight),
                url.clone(),
                async { Ok(include_bytes!("../tests/fixtures/cover.jpg").to_vec()) },
                |bytes| crate::decode::decode(&bytes),
                notifier,
            );
            let finished = tasks.join_next().await.expect("one task");
            finished.expect("the task does not panic");
            rx.recv().expect("update")
        });

        match update {
            Update::Cover { url: seen, image } => {
                assert_eq!(seen, url);
                assert_eq!((image.width, image.height), (16, 8));
            }
            other => panic!("expected a cover, got {other:?}"),
        }
        assert!(in_flight.lock().expect("lock").is_empty());
    }

    #[test]
    fn a_failed_fetch_reaches_the_window_as_cover_failed() {
        let runtime = runtime();
        let (tx, rx) = channel();
        let notifier = Notifier {
            tx,
            slot: WakeSlot::default(),
        };
        let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
        let in_flight = Arc::new(Mutex::new(HashSet::new()));

        let update = runtime.block_on(async {
            let mut tasks = JoinSet::new();
            spawn_cover(
                &mut tasks,
                Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS)),
                Arc::clone(&in_flight),
                url.clone(),
                async { Err(crate::cover::CoverError::Fetch) },
                |_| Err(crate::cover::CoverError::Decode),
                notifier,
            );
            let finished = tasks.join_next().await.expect("one task");
            finished.expect("the task does not panic");
            rx.recv().expect("update")
        });

        assert_eq!(update, Update::CoverFailed { url });
        assert!(in_flight.lock().expect("lock").is_empty());
    }

    #[test]
    fn a_fetch_with_no_session_or_client_is_answered_with_cover_failed() {
        let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
        for (connected, client) in [
            (
                false,
                Some(reqwest::Client::builder().build().expect("client")),
            ),
            (true, None),
        ] {
            let (tx, rx) = channel();
            let notifier = Notifier {
                tx,
                slot: WakeSlot::default(),
            };
            let mut tasks = JoinSet::new();
            handle_fetch(
                connected,
                client.as_ref(),
                &mut tasks,
                Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS)),
                Arc::new(Mutex::new(HashSet::new())),
                url.clone(),
                notifier,
            );
            assert!(tasks.is_empty(), "no fetch is spawned");
            assert_eq!(
                rx.try_recv().expect("one reply"),
                Update::CoverFailed { url: url.clone() }
            );
            assert!(rx.try_recv().is_err(), "exactly one reply");
        }
    }

    #[test]
    fn a_fetch_before_sign_in_is_answered_with_cover_failed() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::default());
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            assert_eq!(
                ui_end.updates.recv().expect("resume update"),
                Update::NeedsAuthorization
            );
            let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
            ui_end
                .commands
                .send(Command::Fetch { url: url.clone() })
                .expect("fetch");
            assert_eq!(
                ui_end.updates.recv().expect("cover reply"),
                Update::CoverFailed { url }
            );
        });
    }

    #[test]
    fn a_failed_decode_reaches_the_window_as_cover_failed() {
        let runtime = runtime();
        let (tx, rx) = channel();
        let notifier = Notifier {
            tx,
            slot: WakeSlot::default(),
        };
        let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
        let in_flight = Arc::new(Mutex::new(HashSet::new()));

        let update = runtime.block_on(async {
            let mut tasks = JoinSet::new();
            spawn_cover(
                &mut tasks,
                Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS)),
                Arc::clone(&in_flight),
                url.clone(),
                async { Ok(vec![0u8; 16]) },
                |bytes| crate::decode::decode(&bytes),
                notifier,
            );
            let finished = tasks.join_next().await.expect("one task");
            finished.expect("the task does not panic");
            rx.recv().expect("update")
        });

        assert_eq!(update, Update::CoverFailed { url });
        assert!(in_flight.lock().expect("lock").is_empty());
    }

    #[test]
    fn a_decode_that_outruns_its_budget_fails_instead_of_hanging() {
        let runtime = runtime();
        let (tx, rx) = channel();
        let notifier = Notifier {
            tx,
            slot: WakeSlot::default(),
        };
        let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
        let in_flight = Arc::new(Mutex::new(HashSet::new()));

        let update = runtime.block_on(async {
            let mut tasks = JoinSet::new();
            crate::cover::spawn_cover_with_timeout(
                &mut tasks,
                Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS)),
                Arc::clone(&in_flight),
                url.clone(),
                async { Ok(include_bytes!("../tests/fixtures/cover.jpg").to_vec()) },
                |_| {
                    std::thread::sleep(Duration::from_millis(500));
                    Ok(CoverImage::new(1, 1, vec![0, 0, 0, 255]).expect("one pixel"))
                },
                notifier,
                Duration::from_millis(20),
            );
            let finished = tasks.join_next().await.expect("one task");
            finished.expect("the task does not panic");
            rx.recv().expect("update")
        });

        assert_eq!(update, Update::CoverFailed { url });
        assert!(in_flight.lock().expect("lock").is_empty());
    }

    /// Records whether the in-flight set was already empty when the wake ran,
    /// which is the moment the reply itself has just been queued.
    struct InFlightProbe {
        in_flight: Arc<Mutex<HashSet<CoverUrl>>>,
        empty_at_wake: AtomicBool,
    }

    impl Waker for InFlightProbe {
        fn wake(&self) {
            let empty = self.in_flight.lock().expect("lock").is_empty();
            self.empty_at_wake.store(empty, Ordering::SeqCst);
        }
    }

    #[test]
    fn a_retry_after_a_failed_fetch_is_not_deduped() {
        let runtime = runtime();
        let (tx, rx) = channel();
        let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
        let in_flight = Arc::new(Mutex::new(HashSet::new()));
        let probe = Arc::new(InFlightProbe {
            in_flight: Arc::clone(&in_flight),
            empty_at_wake: AtomicBool::new(false),
        });
        let slot = WakeSlot::default();
        slot.install(probe.clone());
        let notifier = Notifier { tx, slot };
        let fetches = Arc::new(AtomicUsize::new(0));

        runtime.block_on(async {
            let mut tasks = JoinSet::new();
            let permits = Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS));
            for _ in 0..2 {
                let counting = Arc::clone(&fetches);
                spawn_cover(
                    &mut tasks,
                    Arc::clone(&permits),
                    Arc::clone(&in_flight),
                    url.clone(),
                    async move {
                        counting.fetch_add(1, Ordering::SeqCst);
                        Err(crate::cover::CoverError::Fetch)
                    },
                    |_| Err(crate::cover::CoverError::Decode),
                    notifier.clone(),
                );
                assert_eq!(
                    rx.recv().expect("failure"),
                    Update::CoverFailed { url: url.clone() }
                );
            }
            while tasks.join_next().await.is_some() {}
        });

        // The second request is a real fetch: the first released its claim
        // before it announced the failure.
        assert_eq!(fetches.load(Ordering::SeqCst), 2);
        assert!(
            probe.empty_at_wake.load(Ordering::SeqCst),
            "the in-flight entry must be gone before the reply"
        );
        assert!(in_flight.lock().expect("lock").is_empty());
    }

    #[test]
    fn a_finished_cover_task_is_reaped() {
        let runtime = current_thread_runtime();
        runtime.block_on(async {
            let mut tasks = JoinSet::new();
            tasks.spawn(async {});
            // One yield runs the ready task to completion, leaving its cell in
            // the set until something joins it.
            tokio::task::yield_now().await;
            assert_eq!(tasks.len(), 1);
            reap_finished_covers(&mut tasks);
            assert_eq!(tasks.len(), 0);
        });
    }

    #[test]
    fn a_duplicate_fetch_for_an_in_flight_url_spawns_one_task() {
        let runtime = runtime();
        let (tx, _rx) = channel();
        let notifier = Notifier {
            tx,
            slot: WakeSlot::default(),
        };
        let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
        let in_flight = Arc::new(Mutex::new(HashSet::new()));
        let fetches = Arc::new(AtomicUsize::new(0));

        runtime.block_on(async {
            let mut tasks = JoinSet::new();
            let permits = Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS));
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            spawn_cover(
                &mut tasks,
                Arc::clone(&permits),
                Arc::clone(&in_flight),
                url.clone(),
                async move {
                    let _ = started_tx.send(());
                    std::future::pending().await
                },
                |_| Err(crate::cover::CoverError::Decode),
                notifier.clone(),
            );
            started_rx.await.expect("the first fetch started");

            // A second request for the same URL while it is in flight is
            // dropped rather than fetched again.
            let counting = Arc::clone(&fetches);
            spawn_cover(
                &mut tasks,
                Arc::clone(&permits),
                Arc::clone(&in_flight),
                url.clone(),
                async move {
                    counting.fetch_add(1, Ordering::SeqCst);
                    Ok(Vec::new())
                },
                |_| Err(crate::cover::CoverError::Decode),
                notifier.clone(),
            );
            assert_eq!(tasks.len(), 1);

            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
        });

        assert_eq!(fetches.load(Ordering::SeqCst), 0);
        assert!(in_flight.lock().expect("lock").is_empty());
    }

    #[test]
    fn a_fetch_after_completion_spawns_a_new_task() {
        let runtime = runtime();
        let (tx, _rx) = channel();
        let notifier = Notifier {
            tx,
            slot: WakeSlot::default(),
        };
        let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
        let in_flight = Arc::new(Mutex::new(HashSet::new()));
        let fetches = Arc::new(AtomicUsize::new(0));

        runtime.block_on(async {
            let permits = Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS));
            for _ in 0..2 {
                let mut tasks = JoinSet::new();
                let counting = Arc::clone(&fetches);
                spawn_cover(
                    &mut tasks,
                    Arc::clone(&permits),
                    Arc::clone(&in_flight),
                    url.clone(),
                    async move {
                        counting.fetch_add(1, Ordering::SeqCst);
                        Ok(include_bytes!("../tests/fixtures/cover.jpg").to_vec())
                    },
                    |bytes| crate::decode::decode(&bytes),
                    notifier.clone(),
                );
                while tasks.join_next().await.is_some() {}
            }
        });

        // Nothing completed is retained, so the second request is a real fetch.
        assert_eq!(fetches.load(Ordering::SeqCst), 2);
        assert!(in_flight.lock().expect("lock").is_empty());
    }

    #[test]
    fn an_aborted_fetch_clears_the_in_flight_set_promptly() {
        let runtime = runtime();
        let (tx, _rx) = channel();
        let notifier = Notifier {
            tx,
            slot: WakeSlot::default(),
        };
        let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
        let in_flight = Arc::new(Mutex::new(HashSet::new()));

        let started = std::time::Instant::now();
        runtime.block_on(async {
            let mut tasks = JoinSet::new();
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            spawn_cover(
                &mut tasks,
                Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS)),
                Arc::clone(&in_flight),
                url.clone(),
                async move {
                    let _ = started_tx.send(());
                    std::future::pending().await
                },
                |_| Err(crate::cover::CoverError::Decode),
                notifier,
            );
            started_rx.await.expect("the fetch started");
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
        });

        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(in_flight.lock().expect("lock").is_empty());
    }

    #[test]
    fn a_slow_decode_does_not_hold_up_the_runtime_drop() {
        let runtime = runtime();
        let (tx, _rx) = channel();
        let notifier = Notifier {
            tx,
            slot: WakeSlot::default(),
        };
        let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
        let in_flight = Arc::new(Mutex::new(HashSet::new()));
        let (decode_started_tx, decode_started_rx) = channel();

        runtime.block_on(async {
            let mut tasks = JoinSet::new();
            spawn_cover(
                &mut tasks,
                Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS)),
                Arc::clone(&in_flight),
                url.clone(),
                async { Ok(include_bytes!("../tests/fixtures/cover.jpg").to_vec()) },
                move |_bytes| {
                    let _ = decode_started_tx.send(());
                    std::thread::sleep(Duration::from_millis(400));
                    Ok(CoverImage::new(1, 1, vec![0, 0, 0, 255]).expect("one pixel"))
                },
                notifier,
            );
            // The task is parked on its oneshot while the thread sleeps.
            decode_started_rx.recv().expect("the decode started");
            tasks.abort_all();
        });

        let dropped = std::time::Instant::now();
        drop(runtime);
        assert!(
            dropped.elapsed() < Duration::from_millis(300),
            "dropping the runtime must not join the detached decode thread"
        );
    }

    #[test]
    fn a_second_play_does_not_start_a_second_player() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        let (play_started_tx, play_started_rx) = channel();
        *shared.play_started.lock().expect("lock") = Some(play_started_tx);

        run_window(runtime(), host, store, source, move |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::Play {
                    uri: "spotify:track:x".to_owned(),
                })
                .expect("play");
            play_started_rx.recv().expect("play started");
            ui_end
                .commands
                .send(Command::Play {
                    uri: "spotify:track:x".to_owned(),
                })
                .expect("play");
            // Reload is processed after the second play, so it is a barrier.
            ui_end
                .commands
                .send(Command::ReloadLibrary)
                .expect("reload");
            loop {
                if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                    break;
                }
            }
        });

        assert_eq!(shared.plays.load(Ordering::SeqCst), 2);
        assert_eq!(shared.players.load(Ordering::SeqCst), 1);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn pause_resume_and_stop_with_no_player_do_not_start_one() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end.commands.send(Command::Pause).expect("pause");
            ui_end.commands.send(Command::Resume).expect("resume");
            ui_end.commands.send(Command::Stop).expect("stop");
            ui_end
                .commands
                .send(Command::ReloadLibrary)
                .expect("reload");
            loop {
                if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                    break;
                }
            }
        });

        assert_eq!(shared.pauses.load(Ordering::SeqCst), 1);
        assert_eq!(shared.resumes.load(Ordering::SeqCst), 1);
        assert_eq!(shared.stops.load(Ordering::SeqCst), 1);
        assert_eq!(shared.players.load(Ordering::SeqCst), 0);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_seek_with_no_player_does_not_start_one() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::Seek { position_ms: 12 })
                .expect("seek");
            ui_end
                .commands
                .send(Command::ReloadLibrary)
                .expect("reload");
            loop {
                if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                    break;
                }
            }
        });

        assert_eq!(shared.seeks.load(Ordering::SeqCst), 1);
        assert_eq!(shared.players.load(Ordering::SeqCst), 0);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_seek_reaches_the_host() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        let (play_started_tx, play_started_rx) = channel();
        *shared.play_started.lock().expect("lock") = Some(play_started_tx);

        run_window(runtime(), host, store, source, move |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::Play {
                    uri: "spotify:track:x".to_owned(),
                })
                .expect("play");
            play_started_rx.recv().expect("play started");
            ui_end
                .commands
                .send(Command::Seek {
                    position_ms: 41_000,
                })
                .expect("seek");
            ui_end
                .commands
                .send(Command::Volume { percent: 35 })
                .expect("volume");
            ui_end
                .commands
                .send(Command::ReloadLibrary)
                .expect("reload");
            loop {
                if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                    break;
                }
            }
        });

        assert_eq!(shared.seeks.load(Ordering::SeqCst), 1);
        assert_eq!(shared.last_seek.load(Ordering::SeqCst), 41_000);
        assert_eq!(shared.volumes.load(Ordering::SeqCst), 1);
        assert_eq!(
            shared.last_volume.load(Ordering::SeqCst),
            35,
            "the level the window drew is the level the player was told"
        );
        assert_eq!(shared.players.load(Ordering::SeqCst), 1);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_volume_command_ahead_of_the_play_is_the_level_the_player_starts_at() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        let (play_started_tx, play_started_rx) = channel();
        *shared.play_started.lock().expect("lock") = Some(play_started_tx);

        run_window(runtime(), host, store, source, move |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            ui_end
                .commands
                .send(Command::Volume { percent: 35 })
                .expect("volume");
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::Play {
                    uri: "spotify:track:x".to_owned(),
                })
                .expect("play");
            play_started_rx.recv().expect("play started");
        });

        assert_eq!(
            shared.level_at_player_start.load(Ordering::SeqCst),
            35,
            "the queue delivers in order, and the player is built with the level the host holds"
        );
        assert_eq!(
            shared.volumes.load(Ordering::SeqCst),
            1,
            "the queue delivers in order, so the one level sent arrives once"
        );
        assert_eq!(
            shared.players.load(Ordering::SeqCst),
            1,
            "the queue delivers in order, so the play after the level starts one player"
        );
    }

    #[test]
    fn a_player_with_no_volume_command_starts_at_full() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        let (play_started_tx, play_started_rx) = channel();
        *shared.play_started.lock().expect("lock") = Some(play_started_tx);

        run_window(runtime(), host, store, source, move |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::Play {
                    uri: "spotify:track:x".to_owned(),
                })
                .expect("play");
            play_started_rx.recv().expect("play started");
        });

        assert_eq!(
            shared.level_at_player_start.load(Ordering::SeqCst),
            100,
            "a player started without a level is at full, not silent"
        );
        assert_eq!(shared.volumes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn playback_events_reach_the_window() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        let window_shared = Arc::clone(&shared);
        let (play_started_tx, play_started_rx) = channel();
        *shared.play_started.lock().expect("lock") = Some(play_started_tx);

        let update = run_window(runtime(), host, store, source, move |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::Play {
                    uri: "spotify:track:x".to_owned(),
                })
                .expect("play");
            play_started_rx.recv().expect("play started");
            let tx = window_shared
                .event_tx
                .lock()
                .expect("lock")
                .clone()
                .expect("event sender");
            tx.send(PlaybackEvent::Playing { position_ms: 7 })
                .expect("event");
            ui_end.updates.recv().expect("transport")
        });

        assert_eq!(
            update,
            Update::Transport(TransportUpdate::Position {
                position_ms: 7,
                paused: false,
            })
        );
    }

    #[test]
    fn a_stop_during_tracks_shuts_down_before_the_task_returns() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        *shared.tracks_script.lock().expect("lock") = TracksScript::Pending;
        let (started_tx, started_rx) = channel();
        *shared.tracks_started.lock().expect("lock") = Some(started_tx);

        let runtime = runtime();
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        let window = move || {
            loop {
                if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                    break;
                }
            }
            ui_end
                .commands
                .send(Command::LoadTracks {
                    playlist: "spotify:playlist:a".to_owned(),
                })
                .expect("load");
            started_rx.recv().expect("tracks started");
        };
        let teardown = move || async move {
            // A hung stop must fail the test, not hang the suite.
            tokio::time::timeout(Duration::from_secs(1), worker.finish())
                .await
                .expect("finish must resolve when the stop interrupts the track load");
            assert_eq!(
                shared.shutdowns.load(Ordering::SeqCst),
                1,
                "the shutdown must run when the stop interrupts the track load"
            );
        };

        run_ordered(runtime, window, teardown);
    }

    #[test]
    fn a_stop_with_the_player_started_shuts_down_before_the_task_returns() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        let (play_started_tx, play_started_rx) = channel();
        *shared.play_started.lock().expect("lock") = Some(play_started_tx);

        let runtime = runtime();
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        let window = move || {
            loop {
                if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                    break;
                }
            }
            ui_end
                .commands
                .send(Command::Play {
                    uri: "spotify:track:x".to_owned(),
                })
                .expect("play");
            play_started_rx.recv().expect("play started");
        };
        let teardown = move || async move {
            tokio::time::timeout(Duration::from_secs(1), worker.finish())
                .await
                .expect("finish must resolve when the stop interrupts playback");
            assert_eq!(
                shared.shutdowns.load(Ordering::SeqCst),
                1,
                "the shutdown must run after a player was started"
            );
            assert_eq!(shared.players.load(Ordering::SeqCst), 1);
        };

        run_ordered(runtime, window, teardown);
    }

    #[test]
    fn a_burst_of_play_seek_pause_resume_then_stop_shuts_down_once() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        let (play_started_tx, play_started_rx) = channel();
        *shared.play_started.lock().expect("lock") = Some(play_started_tx);

        let runtime = runtime();
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);
        let commands = ui_end.commands.clone();

        let window = move || {
            loop {
                if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                    break;
                }
            }
            ui_end
                .commands
                .send(Command::Play {
                    uri: "spotify:track:x".to_owned(),
                })
                .expect("play");
            play_started_rx.recv().expect("play started");
            ui_end
                .commands
                .send(Command::Seek {
                    position_ms: 41_000,
                })
                .expect("seek");
            ui_end.commands.send(Command::Pause).expect("pause");
            ui_end.commands.send(Command::Resume).expect("resume");
            ui_end.commands.send(Command::Stop).expect("stop");
        };
        let teardown = move || async move {
            tokio::time::timeout(Duration::from_secs(1), worker.finish())
                .await
                .expect("finish must resolve after a burst of commands");
            assert!(
                commands.is_closed(),
                "the task must have returned before finish resolves"
            );
            assert_eq!(
                shared.shutdowns.load(Ordering::SeqCst),
                1,
                "a burst of commands must still shut down exactly once"
            );
            assert_eq!(shared.players.load(Ordering::SeqCst), 1);
        };

        run_ordered(runtime, window, teardown);
    }

    #[test]
    fn commands_sent_after_the_stop_are_dropped_not_run() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Pending);
        let shared = Arc::clone(&host.shared);
        let (started_tx, started_rx) = channel();
        *shared.rootlist_started.lock().expect("lock") = Some(started_tx);

        let runtime = runtime();
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        let window = move || {
            started_rx.recv().expect("rootlist started");
        };
        let teardown = move || async move {
            // The task is parked on the pending rootlist, so these queue up
            // behind it and the stop is what resumes the task.
            ui_end
                .commands
                .send(Command::Play {
                    uri: "spotify:track:x".to_owned(),
                })
                .expect("queued play");
            ui_end
                .commands
                .send(Command::Seek { position_ms: 5 })
                .expect("queued seek");
            tokio::time::timeout(Duration::from_secs(1), worker.finish())
                .await
                .expect("finish must resolve");
            assert!(
                ui_end.commands.is_closed(),
                "the task must have returned before finish resolves"
            );
            assert_eq!(shared.plays.load(Ordering::SeqCst), 0);
            assert_eq!(shared.seeks.load(Ordering::SeqCst), 0);
            assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
        };

        run_ordered(runtime, window, teardown);
    }

    #[test]
    fn playback_events_after_the_window_closes_do_not_wedge_the_drain_thread() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let list = vec![playlist("spotify:playlist:a", "A")];
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
        let shared = Arc::clone(&host.shared);
        let window_shared = Arc::clone(&shared);
        let (play_started_tx, play_started_rx) = channel();
        *shared.play_started.lock().expect("lock") = Some(play_started_tx);

        let runtime = runtime();
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        let window = move || {
            loop {
                if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                    break;
                }
            }
            ui_end
                .commands
                .send(Command::Play {
                    uri: "spotify:track:x".to_owned(),
                })
                .expect("play");
            play_started_rx.recv().expect("play started");

            // The window goes away, then playback events keep arriving. The
            // drain thread must survive the closed receiver.
            let UiEnd { updates, commands } = ui_end;
            drop(updates);
            drop(commands);
            let tx = window_shared
                .event_tx
                .lock()
                .expect("lock")
                .clone()
                .expect("event sender");
            tx.send(PlaybackEvent::Ended).expect("event");
            tx.send(PlaybackEvent::Unavailable).expect("event");
            window_shared.event_tx.lock().expect("lock").take();
        };
        let teardown = move || async move {
            tokio::time::timeout(Duration::from_secs(1), worker.finish())
                .await
                .expect("finish must resolve with a window that closed first");
            assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
            assert_eq!(shared.players.load(Ordering::SeqCst), 1);
        };

        run_ordered(runtime, window, teardown);
    }
    #[test]
    fn a_rename_reads_the_state_then_writes_and_announces_the_reply_revision() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.state_script.lock().expect("lock") = StateScript::Ready(PlaylistAccess {
            revision: revision(9),
            editable: true,
        });
        *shared.rename_script.lock().expect("lock") = RenameScript::Ready(revision(11));
        let uri = "spotify:playlist:a";

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::RenamePlaylist {
                    playlist: uri.to_owned(),
                    name: "Night Drive".to_owned(),
                })
                .expect("rename");
            ui_end.updates.recv().expect("renamed")
        });

        assert_eq!(
            update,
            Update::PlaylistRenamed {
                playlist: uri.to_owned(),
                name: "Night Drive".to_owned(),
                revision: revision(11),
            }
        );
        assert_eq!(shared.state_reads.load(Ordering::SeqCst), 1);
        assert_eq!(shared.rename_writes.load(Ordering::SeqCst), 1);
        assert_eq!(shared.create_writes.load(Ordering::SeqCst), 0);
        assert_eq!(
            shared.last_rename.lock().expect("lock").clone(),
            Some((uri.to_owned(), revision(9), "Night Drive".to_owned())),
            "the rename must name the revision the state read answered"
        );
    }

    #[test]
    fn a_rename_of_a_list_that_is_not_editable_is_refused_without_a_write() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.state_script.lock().expect("lock") = StateScript::Ready(PlaylistAccess {
            revision: revision(9),
            editable: false,
        });
        let uri = "spotify:playlist:a";

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::RenamePlaylist {
                    playlist: uri.to_owned(),
                    name: "Night Drive".to_owned(),
                })
                .expect("rename");
            ui_end.updates.recv().expect("failed")
        });

        assert_eq!(
            update,
            Update::PlaylistRenameFailed {
                playlist: uri.to_owned(),
            }
        );
        assert_eq!(shared.state_reads.load(Ordering::SeqCst), 1);
        assert_eq!(shared.rename_writes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_rename_with_no_state_fails_without_a_write() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        let uri = "spotify:playlist:a";

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::RenamePlaylist {
                    playlist: uri.to_owned(),
                    name: "Night Drive".to_owned(),
                })
                .expect("rename");
            ui_end.updates.recv().expect("failed")
        });

        assert_eq!(
            update,
            Update::PlaylistRenameFailed {
                playlist: uri.to_owned(),
            }
        );
        assert_eq!(shared.state_reads.load(Ordering::SeqCst), 1);
        assert_eq!(shared.rename_writes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_failed_rename_write_announces_the_failure() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.state_script.lock().expect("lock") = StateScript::Ready(PlaylistAccess {
            revision: revision(9),
            editable: true,
        });
        *shared.rename_script.lock().expect("lock") = RenameScript::Fail;
        let uri = "spotify:playlist:a";

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::RenamePlaylist {
                    playlist: uri.to_owned(),
                    name: "Night Drive".to_owned(),
                })
                .expect("rename");
            ui_end.updates.recv().expect("failed")
        });

        assert_eq!(
            update,
            Update::PlaylistRenameFailed {
                playlist: uri.to_owned(),
            }
        );
        assert_eq!(shared.rename_writes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_stop_with_a_rename_in_flight_shuts_down() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.state_script.lock().expect("lock") = StateScript::Ready(PlaylistAccess {
            revision: revision(9),
            editable: true,
        });
        *shared.rename_script.lock().expect("lock") = RenameScript::Pending;
        let (started_tx, started_rx) = channel();
        *shared.rename_started.lock().expect("lock") = Some(started_tx);

        let runtime = runtime();
        let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

        let window = move || {
            loop {
                if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                    break;
                }
            }
            ui_end
                .commands
                .send(Command::RenamePlaylist {
                    playlist: "spotify:playlist:a".to_owned(),
                    name: "Night Drive".to_owned(),
                })
                .expect("rename");
            started_rx.recv().expect("rename started");
        };
        let teardown = move || async move {
            tokio::time::timeout(Duration::from_secs(1), worker.finish())
                .await
                .expect("finish must resolve when the stop interrupts the rename");
            assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
        };

        run_ordered(runtime, window, teardown);
    }
    // ---- the account's devices ----

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

    #[test]
    fn core_steer_speaks_every_action() {
        assert_eq!(core_steer(Steer::Pause), CoreSteer::Pause);
        assert_eq!(core_steer(Steer::Resume), CoreSteer::Resume);
        assert_eq!(core_steer(Steer::SkipNext), CoreSteer::SkipNext);
        assert_eq!(core_steer(Steer::SkipPrev), CoreSteer::SkipPrev);
        assert_eq!(core_steer(Steer::SeekTo(60_000)), CoreSteer::SeekTo(60_000));
    }

    #[test]
    fn a_watch_before_connect_says_the_devices_are_unreachable() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::default());
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
        let shared = Arc::clone(&host.shared);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            assert_eq!(
                ui_end.updates.recv().expect("resume update"),
                Update::NeedsAuthorization
            );
            ui_end.commands.send(Command::WatchDevices).expect("watch");
            ui_end.updates.recv().expect("watch failed")
        });

        assert_eq!(
            update,
            Update::Devices(DeviceUpdate::Failed(Failure::Devices))
        );
        assert_eq!(
            shared.watches.load(Ordering::SeqCst),
            0,
            "a signed-out window never reaches the dealer"
        );
    }

    #[test]
    fn a_pushed_view_reaches_the_window_as_updates() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            let tx = shared
                .device_tx
                .lock()
                .expect("lock")
                .clone()
                .expect("a watch opened a stream");
            tx.send(ClusterEvent::View(Box::new(phone_view())))
                .expect("view");
            expect_updates(&ui_end, 2)
        });

        assert_eq!(updates, vec![device_roster(), device_playing()]);
        assert_eq!(shared.watches.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_second_watch_request_is_a_no_op() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            ui_end.commands.send(Command::WatchDevices).expect("watch");
            expect_no_update(&ui_end);
        });

        assert_eq!(
            shared.watches.load(Ordering::SeqCst),
            1,
            "the dealer's one start is spent on the first watch"
        );
    }

    #[test]
    fn a_failed_watch_is_retried_by_a_refresh() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.watch_scripts.lock().expect("lock") = vec![WatchScript::Fail, WatchScript::Ready];

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);

            ui_end.commands.send(Command::WatchDevices).expect("watch");
            let failed = ui_end.updates.recv().expect("watch failed");
            assert_eq!(
                failed,
                Update::Devices(DeviceUpdate::Failed(Failure::Devices)),
                "a failed watch is the retryable failure"
            );

            // A refresh after a failed watch starts it again rather than
            // believing it is running.
            let (started_tx, started) = channel();
            *shared.watch_started.lock().expect("lock") = Some(started_tx);
            ui_end
                .commands
                .send(Command::RefreshDevices)
                .expect("refresh");
            started.recv().expect("watch retried");
            let tx = shared
                .device_tx
                .lock()
                .expect("lock")
                .clone()
                .expect("the retry opened a stream");
            tx.send(ClusterEvent::View(Box::new(phone_view())))
                .expect("view");
            expect_updates(&ui_end, 2)
        });

        assert_eq!(updates, vec![device_roster(), device_playing()]);
        assert_eq!(shared.watches.load(Ordering::SeqCst), 2);
        assert_eq!(shared.refreshes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_spent_dealer_stays_terminal_on_watch_and_refresh() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.watch_scripts.lock().expect("lock") = vec![WatchScript::Spent];

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end.commands.send(Command::WatchDevices).expect("watch");
            assert_eq!(
                ui_end.updates.recv().expect("watch failed"),
                Update::Devices(DeviceUpdate::Failed(Failure::DevicesOff)),
                "a start that is spent cannot be retried by asking again"
            );
            ui_end
                .commands
                .send(Command::RefreshDevices)
                .expect("refresh");
            ui_end.updates.recv().expect("refresh stayed off")
        });

        assert_eq!(
            update,
            Update::Devices(DeviceUpdate::Failed(Failure::DevicesOff)),
            "a later refresh keeps the terminal answer"
        );
        assert_eq!(shared.watches.load(Ordering::SeqCst), 2);
        assert_eq!(
            shared.refreshes.load(Ordering::SeqCst),
            0,
            "a spent watch never reaches the announce path"
        );
    }

    #[test]
    fn a_refresh_answers_with_the_roster() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            ui_end
                .commands
                .send(Command::RefreshDevices)
                .expect("refresh");
            expect_updates(&ui_end, 2)
        });

        assert_eq!(updates, vec![device_roster(), device_playing()]);
        assert_eq!(shared.refreshes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_refresh_roster_is_not_undone_by_an_older_push() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.refresh_script.lock().expect("lock") = RefreshScript::TwoDevices;

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            let tx = shared
                .device_tx
                .lock()
                .expect("lock")
                .clone()
                .expect("a watch opened a stream");

            tx.send(ClusterEvent::View(Box::new(phone_view())))
                .expect("old roster");
            let _ = expect_updates(&ui_end, 2);

            ui_end
                .commands
                .send(Command::RefreshDevices)
                .expect("refresh");
            assert_eq!(
                expect_updates(&ui_end, 2),
                vec![
                    Update::Devices(DeviceUpdate::Roster {
                        devices: vec![phone(), tablet()],
                        holder: Some(PHONE.to_owned()),
                        held_here: false,
                    }),
                    device_playing(),
                ]
            );

            tx.send(ClusterEvent::View(Box::new(phone_view())))
                .expect("an older push");
            expect_updates(&ui_end, 2)
        });

        assert_eq!(
            updates,
            vec![device_roster(), device_playing()],
            "a push carrying the old roster replaces the newer one"
        );
    }

    #[test]
    fn a_failed_refresh_says_the_devices_are_unreachable() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.refresh_script.lock().expect("lock") = RefreshScript::Fail;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            ui_end
                .commands
                .send(Command::RefreshDevices)
                .expect("refresh");
            ui_end.updates.recv().expect("refresh failed")
        });

        assert_eq!(
            update,
            Update::Devices(DeviceUpdate::Failed(Failure::Devices))
        );
        assert_eq!(shared.refreshes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_steer_before_connect_never_reaches_the_host() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::default());
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
        let shared = Arc::clone(&host.shared);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            assert_eq!(
                ui_end.updates.recv().expect("resume update"),
                Update::NeedsAuthorization
            );
            ui_end
                .commands
                .send(Command::Steer {
                    device: PHONE.to_owned(),
                    steer: Steer::Pause,
                })
                .expect("steer");
            ui_end.updates.recv().expect("steer failed")
        });

        assert_eq!(update, Update::Devices(DeviceUpdate::SteerFailed));
        assert!(shared.steers.lock().expect("lock").is_empty());
    }

    #[test]
    fn a_steer_that_lands_sends_nothing() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        // The first steer lands, the second is refused: the refusal is the
        // receipt that the landed one was processed and sent nothing.
        *shared.steer_scripts.lock().expect("lock") = vec![SteerScript::Ok, SteerScript::Fail];

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::Steer {
                    device: PHONE.to_owned(),
                    steer: Steer::SkipNext,
                })
                .expect("steer");
            ui_end
                .commands
                .send(Command::Steer {
                    device: PHONE.to_owned(),
                    steer: Steer::Resume,
                })
                .expect("steer");
            ui_end.updates.recv().expect("the refused steer")
        });

        assert_eq!(update, Update::Devices(DeviceUpdate::SteerFailed));
        assert_eq!(
            *shared.steers.lock().expect("lock"),
            vec![
                (PHONE.to_owned(), CoreSteer::SkipNext),
                (PHONE.to_owned(), CoreSteer::Resume),
            ]
        );
    }

    #[test]
    fn a_steer_that_fails_is_reported_once() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.steer_scripts.lock().expect("lock") = vec![SteerScript::Fail];

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::Steer {
                    device: PHONE.to_owned(),
                    steer: Steer::SeekTo(60_000),
                })
                .expect("steer");
            ui_end.updates.recv().expect("steer failed")
        });

        assert_eq!(update, Update::Devices(DeviceUpdate::SteerFailed));
        assert_eq!(
            *shared.steers.lock().expect("lock"),
            vec![(PHONE.to_owned(), CoreSteer::SeekTo(60_000))],
            "a refused steer is never retried"
        );
    }

    #[test]
    fn a_pick_before_connect_reports_failure_without_a_call() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::default());
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());
        let shared = Arc::clone(&host.shared);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            assert_eq!(
                ui_end.updates.recv().expect("resume update"),
                Update::NeedsAuthorization
            );
            ui_end
                .commands
                .send(Command::Transfer {
                    to: PHONE.to_owned(),
                })
                .expect("transfer");
            ui_end.updates.recv().expect("transfer failed")
        });

        assert_eq!(
            update,
            Update::Devices(DeviceUpdate::TransferFailed(Failure::Transfer))
        );
        assert!(shared.transfers.lock().expect("lock").is_empty());
    }

    #[test]
    fn a_successful_pick_sends_no_update() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        let elsewhere = tablet().id;
        // The first pick lands and reports nothing; the second is refused, and
        // that failure is the receipt the first was processed. The two targets
        // differ, so a failure raised by the landed pick names a target it was
        // never sent to; and the script flips only once the host has the first
        // call in hand, so it cannot answer for it.
        let (transfer_tx, transfer_rx) = channel();
        *shared.transfer_started.lock().expect("lock") = Some(transfer_tx);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            ui_end
                .commands
                .send(Command::Transfer {
                    to: PHONE.to_owned(),
                })
                .expect("transfer");
            transfer_rx.recv().expect("the hand-over started");
            expect_no_update(&ui_end);
            *shared.transfer_script.lock().expect("lock") = TransferScript::Fail;
            ui_end
                .commands
                .send(Command::Transfer {
                    to: elsewhere.clone(),
                })
                .expect("transfer");
            ui_end.updates.recv().expect("the refused transfer")
        });

        assert_eq!(
            update,
            Update::Devices(DeviceUpdate::TransferFailed(Failure::Transfer))
        );
        assert_eq!(
            *shared.transfers.lock().expect("lock"),
            vec![PHONE.to_owned(), elsewhere],
            "a landed pick is not retried and reports nothing"
        );
    }

    #[test]
    fn a_failed_pick_sends_transfer_failed() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.transfer_script.lock().expect("lock") = TransferScript::Fail;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            ui_end
                .commands
                .send(Command::Transfer {
                    to: PHONE.to_owned(),
                })
                .expect("transfer");
            ui_end.updates.recv().expect("transfer failed")
        });

        assert_eq!(
            update,
            Update::Devices(DeviceUpdate::TransferFailed(Failure::Transfer))
        );
        assert_eq!(
            *shared.transfers.lock().expect("lock"),
            vec![PHONE.to_owned()],
            "a refused pick is never retried"
        );
    }

    #[test]
    fn a_pick_with_nothing_to_hand_over_says_what_is_missing() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.transfer_script.lock().expect("lock") = TransferScript::NoContext;

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            ui_end
                .commands
                .send(Command::Transfer {
                    to: PHONE.to_owned(),
                })
                .expect("transfer");
            ui_end.updates.recv().expect("transfer failed")
        });

        assert_eq!(
            update,
            Update::Devices(DeviceUpdate::TransferFailed(Failure::NoContext)),
            "the copy names the real problem: no list is playing"
        );
    }

    #[test]
    fn a_stop_during_a_pending_pick_leaves_the_loop() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.transfer_script.lock().expect("lock") = TransferScript::Pending;

        let (transfer_tx, transfer_rx) = channel();
        *shared.transfer_started.lock().expect("lock") = Some(transfer_tx);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            ui_end
                .commands
                .send(Command::Transfer {
                    to: PHONE.to_owned(),
                })
                .expect("transfer");
            transfer_rx.recv().expect("the hand-over started");
            expect_no_update(&ui_end);
        });

        assert_eq!(
            shared.exit_calls.lock().expect("lock").as_slice(),
            ["withdraw", "shutdown"],
            "the stop leaves the pending hand-over and still withdraws"
        );
        assert_eq!(
            *shared.transfers.lock().expect("lock"),
            vec![PHONE.to_owned()],
            "nothing is handed over twice"
        );
    }

    #[test]
    fn a_label_answer_names_the_remote_track() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        let named = track("spotify:track:remote");
        *shared.release_script.lock().expect("lock") = ReleaseScript::Named(vec![named.clone()]);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LabelRemote {
                    track: "spotify:track:remote".to_owned(),
                })
                .expect("label");
            ui_end.updates.recv().expect("labelled")
        });

        assert_eq!(update, Update::Devices(DeviceUpdate::Labelled(named)));
        assert_eq!(shared.releases.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_missing_label_sends_nothing() {
        for script in [ReleaseScript::Ready, ReleaseScript::Fail] {
            let store = Arc::new(MemoryStore::seeded("stored"));
            let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
            let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
            let shared = Arc::clone(&host.shared);
            *shared.release_script.lock().expect("lock") = script;
            *shared.steer_scripts.lock().expect("lock") = vec![SteerScript::Fail];

            let update = run_window(runtime(), host, store, source, |ui_end, slot| {
                slot.install(Arc::new(RecordingWaker::default()));
                let _ = expect_updates(&ui_end, 2);
                ui_end
                    .commands
                    .send(Command::LabelRemote {
                        track: "spotify:track:remote".to_owned(),
                    })
                    .expect("label");
                // A steer that fails answers after the label, so its own update
                // is the receipt that the label sent nothing.
                ui_end
                    .commands
                    .send(Command::Steer {
                        device: PHONE.to_owned(),
                        steer: Steer::Pause,
                    })
                    .expect("steer");
                ui_end.updates.recv().expect("steer failed")
            });

            assert_eq!(update, Update::Devices(DeviceUpdate::SteerFailed));
        }
    }

    #[test]
    fn a_window_that_never_watches_hears_nothing_about_devices() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let updates = expect_updates(&ui_end, 2);
            assert!(
                !updates
                    .iter()
                    .any(|update| matches!(update, Update::Devices(_))),
                "nothing asks for the devices, so nothing reports them"
            );
            expect_no_update(&ui_end);
        });
    }

    #[test]
    fn a_drain_that_ends_while_the_window_lives_says_the_devices_are_lost() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            // The pump is gone while the window is not.
            shared.device_tx.lock().expect("lock").take();
            ui_end
                .updates
                .recv_timeout(Duration::from_secs(2))
                .expect("the drain must report its own end")
        });

        assert_eq!(
            update,
            Update::Devices(DeviceUpdate::Failed(Failure::DevicesOff)),
            "a dead pump is terminal, not a silence to retry"
        );
    }

    #[test]
    fn a_refresh_after_the_drain_dies_stays_off() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            shared.device_tx.lock().expect("lock").take();
            assert_eq!(
                ui_end
                    .updates
                    .recv_timeout(Duration::from_secs(2))
                    .expect("the drain must report its own end"),
                Update::Devices(DeviceUpdate::Failed(Failure::DevicesOff))
            );

            ui_end
                .commands
                .send(Command::RefreshDevices)
                .expect("refresh");
            ui_end
                .updates
                .recv_timeout(Duration::from_secs(2))
                .expect("a dead watch cannot announce again")
        });

        assert_eq!(
            update,
            Update::Devices(DeviceUpdate::Failed(Failure::DevicesOff))
        );
        assert_eq!(
            shared.refreshes.load(Ordering::SeqCst),
            0,
            "the dead watch is not asked for a roster"
        );
    }

    #[test]
    fn a_stop_after_watching_withdraws_before_the_player_goes() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);

        let runtime = runtime();
        let (ui_end, slot, worker) = start(runtime.handle(), host, store, source);
        slot.install(Arc::new(RecordingWaker::default()));

        let watched = Arc::clone(&shared);
        let window = move || {
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &watched);
            // Returned to the caller so it lives through teardown: the stop is
            // the only exit this test exercises.
            ui_end
        };
        let teardown = move || async move {
            tokio::time::timeout(Duration::from_secs(2), worker.finish())
                .await
                .expect("finish resolves");
            assert_eq!(shared.withdrawals.load(Ordering::SeqCst), 1);
            assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
            assert_eq!(
                &*shared.exit_calls.lock().expect("lock"),
                &["withdraw", "shutdown"],
                "the account hears about the withdraw before the player stops"
            );
        };

        let _ui_end = run_ordered(runtime, window, teardown);
    }

    #[test]
    fn a_closed_window_after_watching_withdraws_before_the_player_goes() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);

        let runtime = runtime();
        let (ui_end, slot, worker) = start(runtime.handle(), host, store, source);
        slot.install(Arc::new(RecordingWaker::default()));

        let watched = Arc::clone(&shared);
        let window = move || {
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &watched);
            // Dropping the window's end ends the command loop by itself; there
            // is no stop signal to lean on.
            drop(ui_end);
        };
        let teardown = move || async move {
            tokio::time::timeout(Duration::from_secs(2), worker.finish())
                .await
                .expect("finish resolves");
            assert_eq!(shared.withdrawals.load(Ordering::SeqCst), 1);
            assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
            assert_eq!(
                &*shared.exit_calls.lock().expect("lock"),
                &["withdraw", "shutdown"]
            );
        };

        run_ordered(runtime, window, teardown);
    }

    #[test]
    fn a_stop_during_a_pending_watch_still_withdraws() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.watch_scripts.lock().expect("lock") = vec![WatchScript::Pending];

        let runtime = runtime();
        let (ui_end, slot, worker) = start(runtime.handle(), host, store, source);
        slot.install(Arc::new(RecordingWaker::default()));

        let watched = Arc::clone(&shared);
        let window = move || {
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &watched);
            drop(ui_end);
        };
        let teardown = move || async move {
            tokio::time::timeout(Duration::from_secs(2), worker.finish())
                .await
                .expect("finish must resolve when the stop interrupts the watch");
            assert_eq!(
                shared.withdrawals.load(Ordering::SeqCst),
                1,
                "a watch that was asked for is still withdrawn"
            );
            assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
            assert_eq!(
                &*shared.exit_calls.lock().expect("lock"),
                &["withdraw", "shutdown"]
            );
        };

        run_ordered(runtime, window, teardown);
    }

    #[test]
    fn a_stop_during_a_pending_refresh_still_withdraws() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.refresh_script.lock().expect("lock") = RefreshScript::Pending;

        let runtime = runtime();
        let (ui_end, slot, worker) = start(runtime.handle(), host, store, source);
        slot.install(Arc::new(RecordingWaker::default()));

        let (refresh_tx, refresh_rx) = channel();
        *shared.refresh_started.lock().expect("lock") = Some(refresh_tx);
        let watched = Arc::clone(&shared);
        let window = move || {
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &watched);
            ui_end
                .commands
                .send(Command::RefreshDevices)
                .expect("refresh");
            refresh_rx.recv().expect("refresh started");
            drop(ui_end);
        };
        let teardown = move || async move {
            tokio::time::timeout(Duration::from_secs(2), worker.finish())
                .await
                .expect("finish must resolve when the stop interrupts the refresh");
            assert_eq!(shared.withdrawals.load(Ordering::SeqCst), 1);
            assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
            assert_eq!(
                &*shared.exit_calls.lock().expect("lock"),
                &["withdraw", "shutdown"]
            );
        };

        run_ordered(runtime, window, teardown);
    }

    #[test]
    fn a_withdraw_that_never_answers_is_released() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.withdraw_script.lock().expect("lock") = WithdrawScript::Pending;

        let runtime = runtime();
        let bound = Duration::from_millis(50);
        let (ui_end, slot, worker) = start_with(runtime.handle(), host, store, source, bound);
        slot.install(Arc::new(RecordingWaker::default()));

        let watched = Arc::clone(&shared);
        let window = move || {
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &watched);
        };
        let teardown = move || async move {
            let started = std::time::Instant::now();
            tokio::time::timeout(Duration::from_secs(2), worker.finish())
                .await
                .expect("a hung withdraw cannot hold the exit");
            let waited = started.elapsed();
            assert!(waited >= bound, "the outer bound is the wait");
            assert!(waited < Duration::from_secs(1), "the exit is bounded");
            assert_eq!(shared.withdrawals.load(Ordering::SeqCst), 1);
            assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
        };

        run_ordered(runtime, window, teardown);
    }

    #[test]
    fn a_window_that_never_watched_withdraws_nothing() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
        });

        assert_eq!(
            shared.withdrawals.load(Ordering::SeqCst),
            0,
            "a window that never announced has nothing to withdraw"
        );
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
    }

    // ---- the account's requests ----

    #[test]
    fn an_inbound_command_is_refused_and_nothing_else_happens() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);

            let id = RequestId(7);
            inbound_sender(&shared)
                .send(Inbound {
                    id,
                    kind: Kind::Transfer,
                    instruction: Instruction::Other(Kind::Transfer),
                })
                .expect("inbound");
            until("the request to be refused", || {
                !refusals(&shared).is_empty()
            });
            assert_eq!(refusals(&shared), vec![id]);
            // A refusal is the whole answer: the window hears nothing.
            expect_no_update(&ui_end);
        });

        assert_eq!(
            calls(&shared),
            vec![Call::Refuse(RequestId(7))],
            "the request is refused once and no other call is made"
        );
    }

    #[test]
    fn every_kind_is_refused_exactly_once() {
        const KINDS: [Kind; 15] = [
            Kind::Transfer,
            Kind::Play,
            Kind::Pause,
            Kind::Resume,
            Kind::SeekTo,
            Kind::SkipNext,
            Kind::SkipPrev,
            Kind::SetShufflingContext,
            Kind::SetRepeatingContext,
            Kind::SetRepeatingTrack,
            Kind::SetOptions,
            Kind::SetQueue,
            Kind::AddToQueue,
            Kind::UpdateContext,
            Kind::Unknown,
        ];

        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);

            let tx = inbound_sender(&shared);
            for (id, kind) in (0u64..).zip(KINDS) {
                tx.send(Inbound {
                    id: RequestId(id),
                    kind,
                    instruction: Instruction::Other(kind),
                })
                .expect("inbound");
            }
            until("every request to be refused", || {
                refusals(&shared).len() == KINDS.len()
            });
        });

        let refused = refusals(&shared);
        let expected: Vec<RequestId> = (0u64..).map(RequestId).take(KINDS.len()).collect();
        assert_eq!(refused, expected, "each request is answered, in order");
        assert_eq!(
            calls(&shared),
            refused.into_iter().map(Call::Refuse).collect::<Vec<_>>(),
            "every kind is refused exactly once"
        );
    }

    #[test]
    fn a_report_reaches_the_host_only_when_watching() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);

            // This command is in front of the watch in the same channel, so it
            // is handled before the watch starts.
            ui_end
                .commands
                .send(Command::Report(Some(report("spotify:track:before"))))
                .expect("report");
            watch_now(&ui_end, &shared);
            assert!(
                !calls(&shared)
                    .iter()
                    .any(|call| matches!(call, Call::Report(_))),
                "a report before the watch has no wire to reach"
            );

            ui_end
                .commands
                .send(Command::Report(Some(report("spotify:track:after"))))
                .expect("report");
            until("the report to reach the host", || {
                calls(&shared).iter().any(|call| {
                    matches!(
                        call,
                        Call::Report(report)
                            if report.as_ref() == &Some(self::report("spotify:track:after"))
                    )
                })
            });

            ui_end.commands.send(Command::Report(None)).expect("report");
            until("the clearing to reach the host", || {
                calls(&shared)
                    .iter()
                    .any(|call| matches!(call, Call::Report(report) if report.is_none()))
            });
        });
    }

    #[test]
    fn playing_a_track_makes_the_device_playable() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::Play {
                    uri: TRACK.to_owned(),
                })
                .expect("play");
            until("the device to become playable", || {
                calls(&shared).contains(&Call::SetPlayable(true))
            });
        });

        assert_eq!(
            calls(&shared),
            vec![Call::Play(TRACK.to_owned()), Call::SetPlayable(true)]
        );
        assert_eq!(shared.players.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_failed_play_for_no_audio_device_makes_it_unplayable() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.play_script.lock().expect("lock") = PlayScript::NoAudioDevice;

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::Play {
                    uri: TRACK.to_owned(),
                })
                .expect("play");
            assert_eq!(
                ui_end.updates.recv().expect("play failed"),
                Update::Transport(TransportUpdate::Failed(Failure::NoAudioDevice))
            );
        });

        assert_eq!(
            calls(&shared),
            vec![Call::Play(TRACK.to_owned()), Call::SetPlayable(false)],
            "a machine with no output can never be picked"
        );
    }

    #[test]
    fn a_not_playable_track_leaves_the_device_playable() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::Play {
                    uri: TRACK.to_owned(),
                })
                .expect("play");
            until("the device to become playable", || {
                calls(&shared).contains(&Call::SetPlayable(true))
            });

            // One bad track is not a device that cannot play, so the stage
            // must stay where the good one left it.
            *shared.play_script.lock().expect("lock") = PlayScript::NotPlayable;
            ui_end
                .commands
                .send(Command::Play {
                    uri: TRACK.to_owned(),
                })
                .expect("play");
            assert_eq!(
                ui_end.updates.recv().expect("play failed"),
                Update::Transport(TransportUpdate::Failed(Failure::Other))
            );
        });

        assert_eq!(
            calls(&shared),
            vec![
                Call::Play(TRACK.to_owned()),
                Call::SetPlayable(true),
                Call::Play(TRACK.to_owned()),
            ]
        );
    }

    #[test]
    fn a_closed_inbound_stream_is_forgotten_and_its_arm_goes_quiet() {
        runtime().block_on(async {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Inbound>();
            let mut slot = Some(rx);
            drop(tx);

            assert_eq!(
                next_inbound(&mut slot).await,
                None,
                "a closed stream yields nothing to answer"
            );
            assert!(
                slot.is_none(),
                "the dead receiver is dropped instead of polled again"
            );
            assert!(
                tokio::time::timeout(Duration::from_millis(20), next_inbound(&mut slot))
                    .await
                    .is_err(),
                "an arm with no stream stays pending rather than resolving in a spin"
            );
        });
    }

    #[test]
    fn commands_are_still_served_after_the_inbound_stream_closes() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);

            // The account stops sending requests; the loop must carry on with
            // the window's commands.
            shared.inbound_tx.lock().expect("lock").take();
            expect_no_update(&ui_end);

            ui_end
                .commands
                .send(Command::ReloadLibrary)
                .expect("reload");
            loop {
                if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                    break;
                }
            }
        });

        assert!(
            calls(&shared).is_empty(),
            "a stream that ended refuses nothing"
        );
        assert_eq!(shared.watches.load(Ordering::SeqCst), 1);
    }

    // ---- serving a landing ----

    /// The lines the bridge logged about its own decisions, so a test can tell
    /// two arms that answer the same request apart.
    static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    struct Recorder;

    impl log::Log for Recorder {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            metadata.level() <= log::Level::Info
        }

        fn log(&self, record: &log::Record<'_>) {
            if record.target() != LANDING_LOG_TARGET || !self.enabled(record.metadata()) {
                return;
            }
            LOGGED.lock().expect("lock").push(record.args().to_string());
        }

        fn flush(&self) {}
    }

    /// Begins recording the bridge's log lines, once for the whole test binary.
    fn record_logs() {
        static RECORDER: Recorder = Recorder;
        static INSTALL: std::sync::Once = std::sync::Once::new();
        INSTALL.call_once(|| {
            if log::set_logger(&RECORDER).is_ok() {
                log::set_max_level(log::LevelFilter::Info);
            }
        });
    }

    /// Whether the bridge logged exactly `line` about one of its decisions.
    fn logged(line: &str) -> bool {
        LOGGED.lock().expect("lock").iter().any(|seen| seen == line)
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

    #[test]
    fn a_phone_side_play_of_a_popular_list_lands() {
        use librespot::core::dealer::protocol::SkipTo;

        // An artist's popular tracks arrive as a `spotify:list:` context, which
        // resolves to its rows exactly like a playlist's does.
        const POPULAR: &str = "spotify:list:popular-release-segments-main-roles:artist_x";

        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        let rows = vec![SECOND.to_owned(), TRACK.to_owned()];
        *shared.resolve_script.lock().expect("lock") = ResolveScript::Ok(rows.clone());
        *shared.accept_script.lock().expect("lock") = AcceptScript::Landed(Box::new(Landing {
            rows: rows.clone(),
            index: 1,
            ..default_landing()
        }));
        hold_with(&shared, &[true]);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            inbound(
                &shared,
                51,
                Kind::Play,
                play_instruction(
                    POPULAR,
                    &[],
                    Some(SkipTo {
                        track_uri: Some(TRACK.to_owned()),
                        ..Default::default()
                    }),
                ),
            );
            until("the play to land", || {
                calls(&shared)
                    .iter()
                    .any(|call| matches!(call, Call::Load { .. }))
            });
        });

        let log = calls(&shared);
        assert!(
            !log.iter().any(|call| matches!(call, Call::Refuse(_))),
            "a popular list is not refused: {log:?}"
        );
        assert!(
            log.iter().any(|call| matches!(
                call,
                Call::Load { uri, .. } if uri == TRACK
            )),
            "the row the phone named is the row that loads: {log:?}"
        );
    }

    #[test]
    fn a_window_that_never_played_announces_that_it_can_play() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            let (tx, rx) = channel();
            *shared.watch_started.lock().expect("lock") = Some(tx);
            ui_end.commands.send(Command::WatchDevices).expect("watch");
            rx.recv().expect("watch started");
        });

        // The player is readied before the announcement, so the device is
        // pickable from launch rather than only after a local play.
        let log = calls(&shared);
        assert_eq!(
            log.iter()
                .filter(|call| matches!(call, Call::Prepare))
                .count(),
            1,
            "the watch readies the player once: {log:?}"
        );
        assert!(
            log.iter()
                .any(|call| matches!(call, Call::SetPlayable(true))),
            "the device is announced as able to play: {log:?}"
        );
    }

    #[test]
    fn a_machine_with_no_audio_device_announces_a_controller() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.prepare_script.lock().expect("lock") = PrepareScript::NoAudioDevice;

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            let (tx, rx) = channel();
            *shared.watch_started.lock().expect("lock") = Some(tx);
            ui_end.commands.send(Command::WatchDevices).expect("watch");
            rx.recv().expect("watch started");
        });

        // A device that cannot play says so: the phone lists it greyed out.
        let log = calls(&shared);
        assert!(
            !log.iter()
                .any(|call| matches!(call, Call::SetPlayable(true))),
            "nothing claims this machine can play: {log:?}"
        );
        assert!(
            log.iter()
                .any(|call| matches!(call, Call::SetPlayable(false))),
            "the stage is held down honestly: {log:?}"
        );
    }

    #[test]
    fn a_transfer_runs_prepare_resolve_accept_load_label_in_that_order() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        let id = RequestId(41);
        *shared.resolve_script.lock().expect("lock") = ResolveScript::Ok(vec![TRACK.to_owned()]);
        *shared.accept_script.lock().expect("lock") = AcceptScript::Landed(Box::new(Landing {
            position_ms: 7_000,
            ..default_landing()
        }));
        hold_with(&shared, &[true]);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            inbound(
                &shared,
                41,
                Kind::Transfer,
                transfer_instruction(TRACK, Some("spotify:playlist:p"), &[]),
            );
            until("the label to run", || {
                calls(&shared)
                    .iter()
                    .any(|call| matches!(call, Call::Label(_)))
            });
        });

        assert_eq!(
            calls(&shared),
            vec![
                Call::Prepare,
                Call::SetPlayable(true),
                Call::Resolve("spotify:playlist:p".to_owned()),
                Call::Accept(id),
                Call::Answer(id, true),
                Call::Load {
                    uri: TRACK.to_owned(),
                    position_ms: 7_000,
                    play: true,
                },
                Call::Label(vec![TRACK.to_owned()]),
            ],
            "the hand-over runs its steps in order, and the label waits for the load"
        );
        assert_eq!(
            shared.event_takes.load(Ordering::SeqCst),
            1,
            "the player's events are handed to one drain"
        );
    }

    #[test]
    fn a_play_command_lands_like_a_transfer() {
        use librespot::core::dealer::protocol::SkipTo;

        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        let id = RequestId(44);
        let rows = vec![TRACK.to_owned(), SECOND.to_owned()];
        *shared.resolve_script.lock().expect("lock") = ResolveScript::Ok(rows.clone());
        // The row the position named is the row the landing anchors on.
        *shared.accept_script.lock().expect("lock") = AcceptScript::Landed(Box::new(Landing {
            rows: rows.clone(),
            index: 1,
            position_ms: 4_000,
            ..default_landing()
        }));
        hold_with(&shared, &[true]);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            inbound(
                &shared,
                44,
                Kind::Play,
                play_instruction(
                    PLAYLIST,
                    &[],
                    Some(SkipTo {
                        track_index: Some(1),
                        ..Default::default()
                    }),
                ),
            );
            until("the label to run", || {
                calls(&shared)
                    .iter()
                    .any(|call| matches!(call, Call::Label(_)))
            });
        });

        assert_eq!(
            calls(&shared),
            vec![
                Call::Prepare,
                Call::SetPlayable(true),
                Call::Resolve(PLAYLIST.to_owned()),
                Call::Accept(id),
                Call::Answer(id, true),
                Call::Load {
                    uri: SECOND.to_owned(),
                    position_ms: 4_000,
                    play: true,
                },
                Call::Label(vec![SECOND.to_owned()]),
            ],
            "a play runs the hand-over's own steps and loads the row it anchored on"
        );
    }

    #[test]
    fn a_play_with_nothing_to_anchor_on_is_refused() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        let id = RequestId(45);
        // A play that names its row neither by uri nor by pages has only the
        // read to say what to load, and this read brought nothing.
        *shared.resolve_script.lock().expect("lock") = ResolveScript::Fail;
        hold_with(&shared, &[true]);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            inbound(
                &shared,
                45,
                Kind::Play,
                play_instruction(PLAYLIST, &[], None),
            );
            until("the play to be refused", || answers(&shared, id) == 1);
            expect_no_update(&ui_end);
        });

        assert_eq!(
            calls(&shared),
            vec![
                Call::Prepare,
                Call::SetPlayable(true),
                Call::Resolve(PLAYLIST.to_owned()),
                Call::Refuse(id),
            ],
            "nothing is put and nothing is loaded when there is no row to play"
        );
    }

    #[test]
    fn a_play_of_a_local_or_unsupported_row_is_refused_before_the_put() {
        use librespot::core::dealer::protocol::SkipTo;

        // A local file is a row the phone can hold and this window cannot fetch,
        // whether it names that row by uri or lands on it by position.
        let cases = [
            (
                47,
                vec![TRACK.to_owned(), SECOND.to_owned()],
                SkipTo {
                    track_uri: Some(LOCAL.to_owned()),
                    ..Default::default()
                },
            ),
            (
                48,
                vec![TRACK.to_owned(), LOCAL.to_owned()],
                SkipTo {
                    track_index: Some(1),
                    ..Default::default()
                },
            ),
        ];

        for (number, rows, skip) in cases {
            let store = Arc::new(MemoryStore::seeded("stored"));
            let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
            let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
            let shared = Arc::clone(&host.shared);
            let id = RequestId(number);
            *shared.resolve_script.lock().expect("lock") = ResolveScript::Ok(rows);
            hold_with(&shared, &[true]);

            run_window(runtime(), host, store, source, |ui_end, slot| {
                slot.install(Arc::new(RecordingWaker::default()));
                let _ = expect_updates(&ui_end, 2);
                watch_now(&ui_end, &shared);
                inbound(
                    &shared,
                    number,
                    Kind::Play,
                    play_instruction(PLAYLIST, &[], Some(skip)),
                );
                until("the play to be refused", || answers(&shared, id) == 1);
                expect_no_update(&ui_end);
            });

            assert_eq!(
                calls(&shared),
                vec![
                    Call::Prepare,
                    Call::SetPlayable(true),
                    Call::Resolve(PLAYLIST.to_owned()),
                    Call::Refuse(id),
                ],
                "a row that cannot be fetched must not take the sound and fail"
            );
        }
    }

    #[test]
    fn update_context_is_answered_without_changing_anything() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        let id = RequestId(46);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            inbound(&shared, 46, Kind::UpdateContext, Instruction::Ignored);
            until("the context to be answered", || answers(&shared, id) == 1);
            expect_no_update(&ui_end);
        });

        assert_eq!(
            calls(&shared),
            vec![Call::Answer(id, true)],
            "a context this window does not use is answered, not refused"
        );
    }

    #[test]
    fn landed_precedes_every_playback_update() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        // The load holds the loop where it stands, so the rows can only be
        // waiting in the window if they were sent before it was asked for.
        *shared.load_script.lock().expect("lock") = LoadScript::Held;
        let (load_started, started) = std::sync::mpsc::channel();
        let (release, in_flight) = std::sync::mpsc::channel();
        *shared.load_gate.lock().expect("lock") = Some((load_started, in_flight));
        hold_with(&shared, &[true]);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            inbound(
                &shared,
                42,
                Kind::Transfer,
                transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
            );
            started
                .recv_timeout(Duration::from_secs(2))
                .expect("the load is asked for");
            let landed = ui_end
                .updates
                .recv_timeout(Duration::from_secs(1))
                .expect("the rows are already in the window");
            assert!(
                matches!(landed, Update::Remote(RemoteUpdate::Landed(_))),
                "the rows land before the load that follows them: {landed:?}"
            );
            release.send(()).expect("release the load");

            shared
                .event_tx
                .lock()
                .expect("lock")
                .clone()
                .expect("the drain is open")
                .send(PlaybackEvent::Loading)
                .expect("playback event");

            assert_eq!(
                expect_updates(&ui_end, 1),
                vec![Update::Transport(TransportUpdate::Loading)],
                "the player's first event follows the rows"
            );
        });
    }

    #[test]
    fn a_refused_instruction_never_reaches_the_wire() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            inbound(
                &shared,
                43,
                Kind::Transfer,
                Instruction::Refused(Refusal::NoData),
            );
            until("the refusal", || answers(&shared, RequestId(43)) == 1);
            expect_no_update(&ui_end);
        });

        assert_eq!(
            calls(&shared),
            vec![Call::Refuse(RequestId(43))],
            "a refusal prepares nothing, reads nothing and puts nothing"
        );
        assert!(
            !calls(&shared).iter().any(
                |call| matches!(call, Call::Answer(answered, _) if *answered == RequestId(43))
            ),
            "the request is answered as a refusal, never taken"
        );
    }

    #[test]
    fn a_failed_prepare_refuses_before_resolve_or_accept() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.prepare_script.lock().expect("lock") = PrepareScript::NoAudioDevice;

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            inbound(
                &shared,
                44,
                Kind::Transfer,
                transfer_instruction(TRACK, Some("spotify:playlist:p"), &[]),
            );
            until("the refusal", || answers(&shared, RequestId(44)) == 1);
            expect_no_update(&ui_end);
        });

        assert_eq!(
            calls(&shared),
            vec![
                Call::Prepare,
                Call::SetPlayable(false),
                Call::Refuse(RequestId(44)),
            ],
            "a player that cannot start refuses before any read or put"
        );
    }

    #[test]
    fn a_failed_resolve_falls_back_to_the_one_track_and_still_accepts() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.resolve_script.lock().expect("lock") = ResolveScript::Fail;
        hold_with(&shared, &[true]);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            inbound(
                &shared,
                45,
                Kind::Transfer,
                transfer_instruction(TRACK, Some("spotify:playlist:p"), &[]),
            );
            until("the landing to load", || {
                calls(&shared)
                    .iter()
                    .any(|call| matches!(call, Call::Load { .. }))
            });
        });

        assert_eq!(
            *shared.last_resolution.lock().expect("lock"),
            Some(Resolution::Failed),
            "a failed read still hands the accept a resolution"
        );
        assert!(
            calls(&shared).iter().any(|call| matches!(
                call,
                Call::Load { uri, .. } if uri == TRACK
            )),
            "the one track is what lands"
        );
        assert_eq!(answers(&shared, RequestId(45)), 1);
    }

    #[test]
    fn a_failed_accept_does_not_load_or_land_and_answers_once() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.accept_script.lock().expect("lock") = AcceptScript::Fail;

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            inbound(
                &shared,
                46,
                Kind::Transfer,
                transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
            );
            until("the answer", || answers(&shared, RequestId(46)) == 1);
            expect_no_update(&ui_end);
        });

        assert_eq!(
            calls(&shared),
            vec![
                Call::Prepare,
                Call::SetPlayable(true),
                Call::Accept(RequestId(46)),
                Call::Answer(RequestId(46), false),
            ],
            "a failed accept is the whole answer: no label, no load"
        );
        assert_eq!(answers(&shared, RequestId(46)), 1);
    }

    #[test]
    fn a_load_failure_after_accept_releases_the_sound() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.load_script.lock().expect("lock") = LoadScript::Fail;
        hold_with(&shared, &[true]);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            inbound(
                &shared,
                47,
                Kind::Transfer,
                transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
            );
            until("the sound to be given back", || {
                calls(&shared).contains(&Call::Release)
            });
        });

        let logged = calls(&shared);
        let load = logged
            .iter()
            .position(|call| matches!(call, Call::Load { .. }))
            .expect("the load was attempted");
        let release = logged
            .iter()
            .position(|call| *call == Call::Release)
            .expect("the sound was given back");
        assert!(
            load < release,
            "a load that failed gives the sound back: {logged:?}"
        );
    }

    #[test]
    fn a_load_failure_pauses_the_window_it_just_adopted() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.load_script.lock().expect("lock") = LoadScript::Fail;
        hold_with(&shared, &[true]);

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            inbound(
                &shared,
                58,
                Kind::Transfer,
                transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
            );
            until("the sound to be given back", || {
                calls(&shared).contains(&Call::Release)
            });
            expect_updates_within(&ui_end, 3)
        });

        assert!(
            matches!(updates[0], Update::Remote(RemoteUpdate::Landed(_))),
            "the rows land before the load that failed: {updates:?}"
        );
        assert_eq!(
            updates[1],
            Update::Transport(TransportUpdate::Failed(Failure::Other)),
            "the track that could not be loaded is reported"
        );
        assert_eq!(
            updates[2],
            Update::Remote(RemoteUpdate::Lost { epoch: 1 }),
            "the transport the landing left waiting is paused, not left in Loading"
        );
    }

    #[test]
    fn a_failed_release_after_a_load_failure_is_not_silent() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.load_script.lock().expect("lock") = LoadScript::Fail;
        shared.sound_release_fails.store(true, Ordering::SeqCst);
        hold_with(&shared, &[true]);

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            inbound(
                &shared,
                59,
                Kind::Transfer,
                transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
            );
            until("the give-back to be refused", || {
                calls(&shared).contains(&Call::Release)
            });
            expect_updates_within(&ui_end, 4)
        });

        assert_eq!(
            &updates[1..],
            [
                Update::Transport(TransportUpdate::Failed(Failure::Other)),
                Update::Transport(TransportUpdate::Failed(Failure::Other)),
                Update::Remote(RemoteUpdate::Lost { epoch: 1 }),
            ],
            "a give-back that failed is reported, not dropped"
        );
    }

    #[test]
    fn a_loss_before_landed_sends_no_landed_and_no_load() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        hold_with(&shared, &[false]);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            inbound(
                &shared,
                48,
                Kind::Transfer,
                transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
            );
            until("the answer", || answers(&shared, RequestId(48)) == 1);
            expect_no_update(&ui_end);
        });

        let logged = calls(&shared);
        assert!(
            !logged
                .iter()
                .any(|call| matches!(call, Call::Load { .. } | Call::Label(_))),
            "a landing that lost the sound is neither named nor loaded: {logged:?}"
        );
    }

    #[test]
    fn a_loss_between_landed_and_load_loads_paused() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        hold_with(&shared, &[true, false]);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            inbound(
                &shared,
                49,
                Kind::Transfer,
                transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
            );
            let landed = ui_end.updates.recv().expect("the landing");
            assert!(
                matches!(landed, Update::Remote(RemoteUpdate::Landed(_))),
                "the landing is announced before the load"
            );
            until("the load", || {
                calls(&shared)
                    .iter()
                    .any(|call| matches!(call, Call::Load { .. }))
            });
        });

        assert!(
            calls(&shared)
                .iter()
                .any(|call| matches!(call, Call::Load { play: false, .. })),
            "a loss between the announcement and the load pauses it"
        );
    }

    #[test]
    fn a_transfer_that_creates_the_player_hands_its_events_to_a_drain() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        hold_with(&shared, &[true]);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            inbound(
                &shared,
                50,
                Kind::Transfer,
                transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
            );
            until("the load", || {
                calls(&shared)
                    .iter()
                    .any(|call| matches!(call, Call::Load { .. }))
            });
        });

        assert_eq!(
            shared.event_takes.load(Ordering::SeqCst),
            1,
            "a player created by the landing still gets a drain"
        );
        assert_eq!(shared.players.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_current_track_is_named_when_the_label_answers_and_a_placeholder_when_it_does_not() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        let named = TrackEntry {
            title: "Named".to_owned(),
            ..track(TRACK)
        };
        *shared.release_script.lock().expect("lock") = ReleaseScript::Named(vec![named.clone()]);
        hold_with(&shared, &[true, true, true, true]);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            inbound(
                &shared,
                51,
                Kind::Transfer,
                transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
            );
            until("the first load", || {
                calls(&shared)
                    .iter()
                    .filter(|call| matches!(call, Call::Load { .. }))
                    .count()
                    == 1
            });
            let updates = expect_updates(&ui_end, 2);
            match &updates[0] {
                Update::Remote(RemoteUpdate::Landed(landing)) => assert_eq!(
                    landing.report().track.map(|track| track.title),
                    Some(TRACK.to_owned()),
                    "the landing carries a placeholder until the label answers"
                ),
                other => panic!("expected a landing, got {other:?}"),
            }
            assert_eq!(
                updates[1],
                Update::Remote(RemoteUpdate::Labelled(vec![named])),
                "the answer names the current track"
            );

            *shared.release_script.lock().expect("lock") = ReleaseScript::Ready;
            inbound(
                &shared,
                52,
                Kind::Transfer,
                transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
            );
            until("the second load", || {
                calls(&shared)
                    .iter()
                    .filter(|call| matches!(call, Call::Load { .. }))
                    .count()
                    == 2
            });
            let updates = expect_updates(&ui_end, 1);
            match &updates[0] {
                Update::Remote(RemoteUpdate::Landed(landing)) => assert_eq!(
                    landing.report().track.map(|track| track.title),
                    Some(TRACK.to_owned()),
                    "a miss leaves the placeholder in place"
                ),
                other => panic!("expected a landing, got {other:?}"),
            }
            expect_no_update(&ui_end);
        });
    }

    #[test]
    fn a_stop_during_a_pending_accept_leaves_the_loop_and_still_withdraws() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.accept_script.lock().expect("lock") = AcceptScript::Pending;

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            inbound(
                &shared,
                53,
                Kind::Transfer,
                transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
            );
            until("the accept to start", || {
                calls(&shared)
                    .iter()
                    .any(|call| matches!(call, Call::Accept(_)))
            });
        });

        assert_eq!(
            shared.exit_calls.lock().expect("lock").as_slice(),
            ["withdraw", "shutdown"],
            "the stop leaves the pending accept and still withdraws"
        );
        assert!(
            !calls(&shared)
                .iter()
                .any(|call| matches!(call, Call::Load { .. })),
            "nothing is loaded after the stop"
        );
    }

    #[test]
    fn a_stop_during_a_pending_resolve_leaves_the_loop_and_still_withdraws() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        *shared.resolve_script.lock().expect("lock") = ResolveScript::Pending;

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            inbound(
                &shared,
                58,
                Kind::Transfer,
                transfer_instruction(TRACK, Some("spotify:playlist:p"), &[]),
            );
            until("the resolve to start", || {
                calls(&shared)
                    .iter()
                    .any(|call| matches!(call, Call::Resolve(_)))
            });
        });

        assert_eq!(
            shared.exit_calls.lock().expect("lock").as_slice(),
            ["withdraw", "shutdown"],
            "the stop leaves the pending read and still withdraws"
        );
        assert!(
            !calls(&shared)
                .iter()
                .any(|call| matches!(call, Call::Accept(_))),
            "nothing is accepted after the stop"
        );
    }

    #[test]
    fn every_inbound_is_answered_exactly_once() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        hold_with(&shared, &[true]);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            inbound(
                &shared,
                54,
                Kind::Transfer,
                Instruction::Refused(Refusal::NoData),
            );
            inbound(&shared, 55, Kind::Pause, Instruction::Other(Kind::Pause));
            inbound(
                &shared,
                56,
                Kind::Unknown,
                Instruction::Other(Kind::Unknown),
            );
            inbound(
                &shared,
                57,
                Kind::Transfer,
                transfer_instruction(TRACK, Some("spotify:playlist:p"), &[TRACK]),
            );
            // An order is answered by the window, so this test answers it the
            // way a frame does: once, after the effect.
            hold_request(&shared, RequestId(58));
            inbound(&shared, 58, Kind::Resume, Instruction::Order(Order::Resume));
            let _ = order_update(&ui_end, RequestId(58));
            ui_end
                .commands
                .send(Command::Answer {
                    id: RequestId(58),
                    ok: true,
                })
                .expect("answer");
            until("every inbound to be answered", || {
                (54..=57).all(|id| answers(&shared, RequestId(id)) == 1)
                    && answers(&shared, RequestId(58)) == 1
            });
        });

        for id in 54..=58 {
            assert_eq!(
                answers(&shared, RequestId(id)),
                1,
                "request {id} is answered exactly once"
            );
        }
    }

    #[test]
    fn an_order_while_not_holding_is_refused() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        hold_with(&shared, &[false]);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            hold_request(&shared, RequestId(60));
            inbound(&shared, 60, Kind::Pause, Instruction::Order(Order::Pause));
            until("the order to be refused", || !refusals(&shared).is_empty());
            expect_no_update(&ui_end);
        });

        assert_eq!(
            calls(&shared),
            vec![Call::Refuse(RequestId(60))],
            "a window with no sound to move refuses, and never forwards"
        );
    }

    #[test]
    fn an_order_for_an_expired_entry_is_not_forwarded() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        hold_with(&shared, &[true]);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            record_logs();
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            // The budget answered this one already, so the phone has been told
            // it failed and the window is never asked about it.
            inbound(&shared, 61, Kind::Pause, Instruction::Order(Order::Pause));
            until("the order to be refused", || !refusals(&shared).is_empty());
            expect_no_update(&ui_end);
            until("the order path to say why", || {
                logged("order refused, entry gone: pause")
            });
        });

        assert_eq!(calls(&shared), vec![Call::Refuse(RequestId(61))]);
        assert!(
            !calls(&shared).contains(&Call::Prepare),
            "the order path hands nothing to a player it never prepares"
        );
    }

    #[test]
    fn an_order_with_no_window_is_refused_rather_than_left_to_the_budget() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        hold_with(&shared, &[true]);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            hold_request(&shared, RequestId(63));
            // The window is gone: the receiver its updates would land on is
            // dropped, so the order reaches nobody.
            let UiEnd { updates, commands } = ui_end;
            drop(updates);
            inbound(&shared, 63, Kind::Pause, Instruction::Order(Order::Pause));
            until("the order to be refused", || !refusals(&shared).is_empty());
            drop(commands);
        });

        assert_eq!(
            calls(&shared),
            vec![Call::Refuse(RequestId(63))],
            "the phone hears at once instead of waiting out its budget"
        );
    }

    #[test]
    fn an_order_is_forwarded_and_answered_only_after_the_ui_replies() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        hold_with(&shared, &[true]);

        run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            let deadline = hold_request(&shared, RequestId(62));
            inbound(&shared, 62, Kind::Pause, Instruction::Order(Order::Pause));

            let (order, expires) = order_update(&ui_end, RequestId(62));
            assert_eq!(order, Order::Pause);
            assert_eq!(
                deadline.checked_sub(ORDER_MARGIN),
                Some(expires),
                "the window drops the order the margin before the entry's budget"
            );
            assert_eq!(
                answers(&shared, RequestId(62)),
                0,
                "the bridge does not answer for the window"
            );

            ui_end
                .commands
                .send(Command::Answer {
                    id: RequestId(62),
                    ok: true,
                })
                .expect("answer");
            until("the window's answer", || {
                answers(&shared, RequestId(62)) == 1
            });
        });

        assert_eq!(
            calls(&shared),
            vec![Call::Answer(RequestId(62), true)],
            "the order is neither refused nor answered before the window replies"
        );
    }

    #[test]
    fn lost_becomes_an_update_with_its_epoch_and_the_drain_keeps_running() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);

        let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            watch_now(&ui_end, &shared);
            let tx = shared
                .device_tx
                .lock()
                .expect("lock")
                .clone()
                .expect("a watch opened a stream");
            tx.send(ClusterEvent::Lost { epoch: 9 }).expect("loss");
            let lost = ui_end.updates.recv().expect("loss update");
            assert_eq!(lost, Update::Remote(RemoteUpdate::Lost { epoch: 9 }));

            tx.send(ClusterEvent::View(Box::new(phone_view())))
                .expect("view");
            expect_updates(&ui_end, 2)
        });

        assert_eq!(updates, vec![device_roster(), device_playing()]);
    }

    #[test]
    fn a_label_rows_command_replies_with_labelled_rows() {
        let store = Arc::new(MemoryStore::seeded("stored"));
        let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
        let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new()));
        let shared = Arc::clone(&host.shared);
        let named = TrackEntry {
            title: "Named".to_owned(),
            ..track(TRACK)
        };
        *shared.release_script.lock().expect("lock") = ReleaseScript::Named(vec![named.clone()]);

        let update = run_window(runtime(), host, store, source, |ui_end, slot| {
            slot.install(Arc::new(RecordingWaker::default()));
            let _ = expect_updates(&ui_end, 2);
            ui_end
                .commands
                .send(Command::LabelRows {
                    uris: vec![TRACK.to_owned()],
                })
                .expect("labels");
            ui_end.updates.recv().expect("labelled")
        });

        assert_eq!(update, Update::Remote(RemoteUpdate::Labelled(vec![named])));
        assert_eq!(calls(&shared), vec![Call::Label(vec![TRACK.to_owned()])]);
    }
}
