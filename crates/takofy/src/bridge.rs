//! The rendezvous between the window and the sign-in worker.
//!
//! The window owns only what it draws. The worker owns the blocking sign-in
//! calls, and teardown finishes it before the runtime is dropped. Sign-in work
//! runs on plain threads rather than the runtime: a blocking call parked on a
//! browser callback must not hold up runtime shutdown, and a detached thread
//! dies with the process.

use std::future::Future;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, OnceLock};

use takofy_core::credentials::{AccessToken, CredentialStore};
use takofy_core::login::{AuthError, SourceErrorKind, TokenSource, authorize, resume};
use takofy_ui::message::{Command, Failure, Update};
use tokio::runtime::{Handle, Runtime};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

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
struct Notifier {
    tx: Sender<Update>,
    slot: WakeSlot,
}

impl Notifier {
    fn send(&self, update: Update) {
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
        let _ = self.task.await;
    }
}

/// Starts the sign-in worker and kicks off the resume attempt immediately, so
/// it overlaps window creation.
pub(crate) fn start<S: TokenSource + Send + Sync + 'static>(
    handle: &Handle,
    store: Arc<dyn CredentialStore>,
    source: Arc<S>,
) -> (UiEnd, WakeSlot, Worker) {
    let (updates_tx, updates_rx) = channel();
    let (commands_tx, mut commands_rx) = tokio::sync::mpsc::unbounded_channel();
    let (stop_tx, mut stop_rx) = oneshot::channel();
    let slot = WakeSlot::default();
    let notifier = Notifier {
        tx: updates_tx,
        slot: slot.clone(),
    };

    {
        let notifier = notifier.clone();
        let store = Arc::clone(&store);
        let source = Arc::clone(&source);
        std::thread::spawn(move || {
            send_update(
                &notifier,
                resume_update(resume(store.as_ref(), source.as_ref())),
            );
        });
    }

    let task_notifier = notifier.clone();
    let task = handle.spawn(async move {
        loop {
            tokio::select! {
                biased;
                _ = &mut stop_rx => break,
                command = commands_rx.recv() => match command {
                    Some(Command::Authorize) => {
                        let notifier = task_notifier.clone();
                        let store = Arc::clone(&store);
                        let source = Arc::clone(&source);
                        std::thread::spawn(move || {
                            send_update(
                                &notifier,
                                authorize_update(authorize(store.as_ref(), source.as_ref())),
                            );
                        });
                    }
                    None => break,
                },
            }
        }
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
pub(crate) fn run_window<S, R>(
    runtime: Runtime,
    store: Arc<dyn CredentialStore>,
    source: Arc<S>,
    window: impl FnOnce(UiEnd, WakeSlot) -> R,
) -> R
where
    S: TokenSource + Send + Sync + 'static,
{
    let (ui_end, slot, worker) = start(runtime.handle(), store, source);
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
    use std::sync::mpsc::{Receiver, channel};
    use std::sync::{Arc, Mutex};

    use takofy_core::credentials::{AccessToken, CredentialStore, RefreshToken, StoreError};
    use takofy_core::login::{SourceError, SourceErrorKind, TokenSet, TokenSource};
    use tokio::runtime::Runtime;

    use super::*;

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
            *self.seen.lock().expect("lock")
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
    }

    impl MemoryStore {
        fn seeded(value: &str) -> Self {
            Self {
                token: Mutex::new(RefreshToken::new(value.to_owned())),
            }
        }
    }

    impl CredentialStore for MemoryStore {
        fn load(&self) -> Result<Option<RefreshToken>, StoreError> {
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

        let update = run_window(runtime(), store, source, |ui_end, slot| {
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

        let updates = run_window(runtime(), store, source, |ui_end, slot| {
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
        let (ui_end, _slot, worker) = start(runtime.handle(), store, source);

        // The assertion must run between `finish` and `drop(runtime)`, while
        // the runtime still owns the command task. After the runtime drops, the
        // channel would read as closed even if `finish` never joined.
        let teardown = move || async move {
            worker.finish().await;
            assert!(
                ui_end.commands.is_closed(),
                "closing the command channel is the proof that the task was joined"
            );
        };

        run_ordered(runtime, || {}, teardown);
    }

    #[test]
    fn teardown_runs_when_the_window_closure_returns_an_error() {
        let store = Arc::new(MemoryStore::default());
        let source = Arc::new(FixedSource::default());
        let runtime = current_thread_runtime();
        let (ui_end, _slot, worker) = start(runtime.handle(), store, source);

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

        let entered = run_window(runtime(), store, source, |_ui_end, _slot| {
            assert!(tokio::runtime::Handle::try_current().is_ok());
            tokio::spawn(async {}).abort();
            true
        });

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
        let (ui_end, _slot, worker) = start(runtime.handle(), store, source);

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
        let (_ui_end, _slot, worker) = start(runtime.handle(), store, source);

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
    fn run_ordered_returns_the_window_result() {
        let result = run_ordered(runtime(), || 42, || async {});
        assert_eq!(result, 42);
    }
}
