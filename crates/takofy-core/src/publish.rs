//! The one task that keeps the account's copy of this device current.
//!
//! A flip only marks the stage that should be announced; this task turns the
//! newest stage into one put. That is what makes a burst of flips cost one
//! request, and what keeps a slow put out of the pump's loop, so a cluster push
//! is read while a put is still on the wire.

use std::sync::Arc;

use crate::connect::{ClusterEvent, ConnectError};
use crate::watch::{Gate, Shared, Timing, Wire, put_and_view};

/// How many times one stage is put before its failure is the answer.
///
/// A put that never landed leaves the device showing as something the phone
/// cannot pick, so a failure is worth riding out; three tries covers a blip
/// without hammering an account that is refusing us.
pub(crate) const PUT_ATTEMPTS: u8 = 3;

/// How one attempt to announce the stage ended.
enum Progress {
    /// The stage went out.
    Sent,
    /// The wire was busy; the stage is still the truth, so it is tried again.
    Busy,
    /// The put failed; the stage is still the truth, so it is tried again.
    Failed,
    /// There is nothing to announce to, or nothing left to announce.
    Done,
    /// The device has been withdrawn.
    Gone,
    /// Nobody is reading the events any more, so there is nobody to tell.
    Unwatched,
}

/// Announces the current stage after every flip, until the device goes.
///
/// A flip that lands during the debounce or during a put collapses into the one
/// put that follows, because the body is built under the gate from the stage the
/// presence holds at that moment.
pub(crate) async fn run<W: Wire>(
    wire: Arc<W>,
    gate: Arc<Gate>,
    shared: Arc<Shared>,
    timing: Timing,
) {
    loop {
        shared.presence.changed().await;
        let stage = shared.presence.stage();
        let mut failures = 0;
        let mut reported = false;
        loop {
            if gate.withdrawn() {
                return;
            }
            tokio::time::sleep(timing.debounce).await;
            match announce(&*wire, &gate, &shared, timing).await {
                Progress::Sent | Progress::Done => break,
                Progress::Gone | Progress::Unwatched => return,
                Progress::Busy => continue,
                Progress::Failed => {
                    // One failure per run of them, so a retry is not a second
                    // failure the window has to explain.
                    if !reported {
                        if !shared.emit(ClusterEvent::Failed) {
                            return;
                        }
                        reported = true;
                    }
                    failures += 1;
                    // A flip during the attempt is a new run with a new stage
                    // of its own, so this one has nothing left to say.
                    if failures >= PUT_ATTEMPTS || shared.presence.stage() != stage {
                        break;
                    }
                }
            }
        }
    }
}

/// One attempt under the gate, bounded the way every other write is.
async fn announce<W: Wire>(wire: &W, gate: &Gate, shared: &Shared, timing: Timing) -> Progress {
    let Ok(_guard) = tokio::time::timeout(timing.gate_wait, gate.take()).await else {
        return Progress::Busy;
    };
    if gate.withdrawn() {
        return Progress::Gone;
    }
    match put_and_view(wire, gate, shared, timing).await {
        Ok(view) => {
            if shared.emit(ClusterEvent::View(Box::new(view))) {
                Progress::Sent
            } else {
                Progress::Unwatched
            }
        }
        Err(ConnectError::Withdrawn) => Progress::Gone,
        // A device the account does not know yet has nobody to tell; the pump
        // announces the stage as soon as the connection id arrives.
        Err(ConnectError::NoConnectionId) => Progress::Done,
        Err(_) => Progress::Failed,
    }
}
