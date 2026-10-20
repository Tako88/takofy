//! The rendezvous between the window and the sign-in worker.
//!
//! The window owns only what it draws. The worker owns the blocking sign-in
//! calls, and teardown finishes it before the runtime is dropped. Sign-in work
//! runs on plain threads rather than the runtime: a blocking call parked on a
//! browser callback must not hold up runtime shutdown, and a detached thread
//! dies with the process.

use std::collections::HashSet;
use std::future::Future;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};

use takofy_core::credentials::{AccessToken, CredentialStore};
use takofy_core::login::{AuthError, SourceErrorKind, TokenSource, authorize, resume};
use takofy_core::playback::{PlaybackError, PlaybackEvent};
use takofy_model::CoverUrl;
use takofy_ui::message::{Command, Failure, TransportUpdate, Update};
use tokio::runtime::{Handle, Runtime};
use tokio::sync::Semaphore;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;
use tokio::sync::oneshot::Receiver as StopReceiver;
use tokio::task::JoinSet;

use crate::cover::{FETCH_TIMEOUT, MAX_CONCURRENT_COVERS, fetch_cover, spawn_cover};
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
    pub(crate) fn send(&self, update: Update) {
        // The update must be readable before the waker runs: a wake that beats
        // the queue into the channel would be spent on a frame with nothing to
        // drain.
        if self.tx.send(update).is_err() {
            // The window is gone; the task is being torn down anyway.
            return;
        }
        self.slot.wake();
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
                Event::Command(Some(Command::Play { uri })) => {
                    play(&mut host, &task_notifier, &uri);
                }
                Event::Command(Some(Command::Pause)) => host.pause(),
                Event::Command(Some(Command::Resume)) => host.resume(),
                Event::Command(Some(Command::Stop)) => host.stop(),
                Event::Command(Some(Command::Seek { position_ms })) => host.seek(position_ms),
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
            }
        }
        // Cancels every in-flight fetch. A detached decode thread is left to
        // finish and its result is dropped with the oneshot receiver.
        cover_tasks.abort_all();
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
        Some(result) => {
            announce(notifier, result, Update::Playlists, Update::PlaylistFailed);
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
    }
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

/// Loads one artist and announces the page, then the followed state when the
/// probe answered.
///
/// The two updates are separate because they are separate facts: the page is
/// drawn from the first, and the Follow label moves only on the second. A probe
/// that did not answer sends nothing, so the label stays where it was.
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

/// Starts the player if needed and hands the one drain thread its events.
fn play<H: SessionHost>(host: &mut H, notifier: &Notifier, uri: &str) {
    match host.play(uri) {
        Ok(()) => {
            if let Some(events) = host.take_events() {
                spawn_drain(notifier.clone(), events);
            }
        }
        Err(error) => {
            notifier.send(Update::Transport(TransportUpdate::Failed(
                playback_failure(&error),
            )));
        }
    }
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

/// The lyrics pane has its own failure: the page around it may draw fine, and
/// its message names only the lyrics. The reason is for the log, never the
/// window.
pub(crate) fn lyrics_failure(error: &SessionError) -> Failure {
    log::warn!("session failure: {}", error.kind());
    Failure::Lyrics
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
    use std::time::Duration;

    use takofy_core::credentials::{AccessToken, CredentialStore, RefreshToken, StoreError};
    use takofy_core::login::{SourceError, SourceErrorKind, TokenSet, TokenSource};
    use takofy_model::{
        AlbumDetail, ArtistDetail, CoverImage, CoverUrl, Episode, FollowedArtist, FollowedShow,
        LyricLine, PlaylistEntry, SavedAlbum, Show, ShowDetail, TrackEntry, TrackLyrics,
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
        AuthError::Source(SourceError {
            kind,
            msg: "detail that must not reach the window".to_owned(),
        })
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

    #[derive(Default)]
    struct FakeState {
        connect_scripts: Mutex<Vec<ConnectScript>>,
        rootlist_script: Mutex<RootlistScript>,
        tracks_script: Mutex<TracksScript>,
        album_script: Mutex<AlbumScript>,
        show_script: Mutex<ShowScript>,
        followed_script: Mutex<FollowedScript>,
        followed_artists_script: Mutex<FollowedArtistsScript>,
        saved_albums_script: Mutex<SavedAlbumsScript>,
        artist_script: Mutex<ArtistScript>,
        lyrics_script: Mutex<LyricsScript>,
        release_script: Mutex<ReleaseScript>,
        follow_script: Mutex<FollowScript>,
        connected: AtomicBool,
        connects: AtomicUsize,
        rootlists: AtomicUsize,
        tracks: AtomicUsize,
        albums: AtomicUsize,
        shows: AtomicUsize,
        followed_lists: AtomicUsize,
        followed_artist_lists: AtomicUsize,
        saved_album_lists: AtomicUsize,
        artists: AtomicUsize,
        lyrics_loads: AtomicUsize,
        releases: AtomicUsize,
        follow_writes: AtomicUsize,
        artist_started: Mutex<Option<Sender<()>>>,
        lyrics_started: Mutex<Option<Sender<()>>>,
        show_started: Mutex<Option<Sender<()>>>,

        last_follow: Mutex<Option<(String, bool)>>,
        plays: AtomicUsize,
        pauses: AtomicUsize,
        resumes: AtomicUsize,
        stops: AtomicUsize,
        seeks: AtomicUsize,
        last_seek: AtomicUsize,
        players: AtomicUsize,
        player_started: AtomicBool,
        shutdowns: AtomicUsize,
        connect_started: Mutex<Option<Sender<()>>>,
        rootlist_started: Mutex<Option<Sender<()>>>,
        tracks_started: Mutex<Option<Sender<()>>>,
        album_started: Mutex<Option<Sender<()>>>,
        play_started: Mutex<Option<Sender<()>>>,
        events: Mutex<Option<UnboundedReceiver<PlaybackEvent>>>,
        event_tx: Mutex<Option<UnboundedSender<PlaybackEvent>>>,
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
                    ..FakeState::default()
                }),
            }
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

        fn rootlist(
            &self,
        ) -> impl Future<Output = Result<Vec<PlaylistEntry>, SessionError>> + Send {
            self.shared.rootlists.fetch_add(1, Ordering::SeqCst);
            if let Some(tx) = self.shared.rootlist_started.lock().expect("lock").take() {
                let _ = tx.send(());
            }
            let script = self.shared.rootlist_script.lock().expect("lock").clone();
            async move {
                match script {
                    RootlistScript::Ready(list) => Ok(list),
                    RootlistScript::Fail => Err(SessionError::NotConnected),
                    RootlistScript::Pending => std::future::pending().await,
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

        fn release_tracks(
            &self,
            _tracks: &[String],
        ) -> impl Future<Output = Result<Vec<TrackEntry>, SessionError>> + Send {
            self.shared.releases.fetch_add(1, Ordering::SeqCst);
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

        fn play(&mut self, _uri: &str) -> Result<(), SessionError> {
            self.shared.plays.fetch_add(1, Ordering::SeqCst);
            if !self.shared.connected.load(Ordering::SeqCst) {
                return Err(SessionError::NotConnected);
            }
            if !self.shared.player_started.swap(true, Ordering::SeqCst) {
                self.shared.players.fetch_add(1, Ordering::SeqCst);
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                *self.shared.event_tx.lock().expect("lock") = Some(tx);
                *self.shared.events.lock().expect("lock") = Some(rx);
            }
            if let Some(tx) = self.shared.play_started.lock().expect("lock").take() {
                let _ = tx.send(());
            }
            Ok(())
        }

        fn take_events(&mut self) -> Option<UnboundedReceiver<PlaybackEvent>> {
            self.shared.events.lock().expect("lock").take()
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

        fn shutdown(self) -> impl Future<Output = Result<(), SessionError>> + Send {
            self.shared.shutdowns.fetch_add(1, Ordering::SeqCst);
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
    const TRACK: &str = "spotify:track:a";

    /// One artist page's data, with no top tracks and no releases, so a test
    /// names only what it reads.
    fn artist_detail(uri: &str) -> ArtistDetail {
        ArtistDetail {
            uri: uri.to_owned(),
            name: "Vela Moreno".to_owned(),
            portrait: None,
            top_tracks: Vec::new(),
            releases: Vec::new(),
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
        }
    }

    fn expect_updates(ui_end: &UiEnd, count: usize) -> Vec<Update> {
        (0..count)
            .map(|_| ui_end.updates.recv().expect("update"))
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
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
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
            }),
            |page| Update::Tracks {
                playlist: playlist.clone(),
                tracks: page.tracks,
                cover: page.cover,
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
        });
        assert_eq!(
            update,
            Update::Tracks {
                playlist: "spotify:playlist:a".to_owned(),
                tracks: vec![track("t")],
                cover: Some(cover),
            }
        );
    }

    #[test]
    fn a_track_page_without_a_cover_reaches_the_update_as_none() {
        let update = run_track_page(TrackPage {
            tracks: vec![track("t")],
            cover: None,
        });
        assert_eq!(
            update,
            Update::Tracks {
                playlist: "spotify:playlist:a".to_owned(),
                tracks: vec![track("t")],
                cover: None,
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
        assert_eq!(shared.players.load(Ordering::SeqCst), 1);
        assert_eq!(shared.shutdowns.load(Ordering::SeqCst), 1);
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
}
