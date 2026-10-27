//! Waking the window: the waker slot, the notifier, and both ends of the loop.

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, OnceLock};

use takofy_ui::message::{Command, Update};
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
pub(crate) struct Notifier {
    pub(super) tx: Sender<Update>,
    pub(super) slot: WakeSlot,
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
    pub(super) stop: oneshot::Sender<()>,
    pub(super) task: tokio::task::JoinHandle<()>,
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
