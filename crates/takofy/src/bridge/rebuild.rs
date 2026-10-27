//! The rebuild schedule: how one failed attempt is settled and the next timed.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use takofy_core::credentials::{AccessToken, CredentialStore};
use takofy_core::login::{TokenSource, resume};
use tokio::sync::oneshot;

use super::covers::Drains;
use super::devices::Devices;
use crate::playhead::Playhead;
use crate::recovery::{Attempt, Recovery};

/// How one rebuild attempt ended.
pub(super) enum Rebuilt {
    /// The session is back and the track reloaded.
    Done,
    /// A step failed, and the schedule decides when to try again.
    Failed,
    /// A command arrived first, and waits in the deferred queue.
    Interrupted,
    /// The window closed.
    Stopped,
}

/// Whether a host call finished or was abandoned because the window closed.
pub(super) enum Step {
    Done,
    Stopped,
}

impl Step {
    pub(super) fn stopped(&self) -> bool {
        matches!(self, Self::Stopped)
    }
}

/// Whether a drain's signal is about the current session and no rebuild is
/// already underway.
pub(super) fn link_signal_applies(
    generation: u64,
    playhead: &Playhead,
    recovery: &Recovery,
) -> bool {
    generation == playhead.generation() && !recovery.is_active()
}

/// Applies one attempt's outcome to the schedule, so every exit leaves the next
/// attempt either cleared or moved forward.
pub(super) fn settle(recovery: &mut Recovery, outcome: &Rebuilt, now: Instant) -> Option<Attempt> {
    match outcome {
        Rebuilt::Done => {
            recovery.recovered();
            None
        }
        Rebuilt::Failed => Some(recovery.failed(now)),
        Rebuilt::Interrupted => {
            recovery.interrupted(now);
            None
        }
        Rebuilt::Stopped => None,
    }
}

/// Silences what a session fed before it is replaced: its drains, its device
/// watch, and the flag that says an engine is up.
pub(super) fn silence_session(devices: &mut Devices, drains: &Drains, engine_up: &mut bool) {
    // The old streams end with the session; a retired drain must not announce
    // that end as a lost device.
    drains.retired.store(true, Ordering::SeqCst);
    devices.retire();
    *engine_up = false;
}

/// The one token refresh that may be in flight.
pub(super) type Refresh = oneshot::Receiver<Option<AccessToken>>;

/// Starts one refresh on a detached thread, so a blocking exchange cannot hold
/// up the runtime's drop.
pub(super) fn start_refresh<S: TokenSource + Send + Sync + 'static>(
    store: &Arc<dyn CredentialStore>,
    source: &Arc<S>,
) -> Refresh {
    let (tx, rx) = oneshot::channel();
    let store = Arc::clone(store);
    let source = Arc::clone(source);
    std::thread::spawn(move || {
        let answer = match resume(store.as_ref(), source.as_ref()) {
            Ok(Some(token)) => Some(token),
            // A revoked sign-in and an offline machine look the same here.
            Ok(None) | Err(_) => None,
        };
        let _ = tx.send(answer);
    });
    rx
}
