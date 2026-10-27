//! The one task that keeps the account's copy of this device current.
//!
//! A flip or a report only marks the stage that should be announced; this task
//! turns the newest state into one put. That is what makes a burst of changes
//! cost one request, and what keeps a slow put out of the pump's loop, so a
//! cluster push is read while a put is still on the wire.

use std::sync::Arc;

use takofy_model::Report;

use crate::connect::{ClusterEvent, ConnectError};
use crate::hold;
use crate::registry::Ack;
use crate::watch::{Gate, Shared, Timing, Wire, put_and_view};

/// How many times one stage is put before its failure is the answer.
///
/// A put that never landed leaves the device showing as something the phone
/// cannot pick, so a failure is worth riding out; three tries covers a blip
/// without hammering an account that is refusing us.
pub(crate) const PUT_ATTEMPTS: u8 = 3;

/// What a put would say, without the report's age.
///
/// The age moves every instant, so it cannot be part of a change test; the
/// report and the command acknowledgment are what the phone would see differ.
#[derive(Clone, PartialEq, Eq)]
enum Signature {
    Controller,
    Ready,
    Active {
        report: Box<Report>,
        ack: Option<Ack>,
    },
}

/// The body this bundle would put now, without changing what is held.
fn signature(shared: &Shared) -> Signature {
    let playable = shared.presence.playable();
    if shared.presence.active()
        && playable
        && let Some((report, _)) = shared.latest.report()
    {
        return Signature::Active {
            report: Box::new(report),
            ack: shared.registry.last_ack(),
        };
    }
    if playable {
        Signature::Ready
    } else {
        Signature::Controller
    }
}

/// How one attempt to announce the stage ended.
enum Progress {
    /// The stage went out.
    Sent(Signature),
    /// Nothing the account would see has changed.
    Unchanged,
    /// The wire was busy; the stage is still the truth, so it is tried again.
    Busy,
    /// The put failed; the stage is still the truth, so it is tried again.
    Failed(Signature),
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
/// put that follows, because the body is built under the gate from the newest
/// report at that moment. A put that would repeat what was just put sends
/// nothing, so a stored wake from an old report costs no request.
pub(crate) async fn run<W: Wire>(
    wire: Arc<W>,
    gate: Arc<Gate>,
    shared: Arc<Shared>,
    timing: Timing,
) {
    let mut last: Option<Signature> = None;
    loop {
        // A flip in what this device is, an order that has just been answered,
        // or a new report. All three futures are cancel safe, so the one that
        // loses the race is not a notification lost.
        tokio::select! {
            _ = shared.presence.changed() => {}
            _ = shared.registry.changed() => {}
            _ = shared.latest.changed() => {}
        }
        let mut failures = 0;
        let mut reported = false;
        loop {
            if gate.withdrawn() {
                return;
            }
            tokio::time::sleep(timing.debounce).await;
            if gate.withdrawn() {
                return;
            }
            // A report that cannot hold the sound gives it back before the
            // body below would clear the activation without telling the
            // account. The release owns the gate, so it runs before announce.
            if shared.presence.active()
                && (!shared.presence.playable() || !shared.latest.has_report())
            {
                match hold::release(&*wire, &gate, &shared, timing).await {
                    Ok(view) => {
                        if !shared.emit(ClusterEvent::View(Box::new(view))) {
                            return;
                        }
                        last = Some(signature(&shared));
                        break;
                    }
                    Err(ConnectError::Withdrawn) => return,
                    Err(ConnectError::Busy) => continue,
                    Err(ConnectError::NoConnectionId) => break,
                    Err(_) => {
                        if !report_failure(&shared, &mut reported) {
                            return;
                        }
                        failures += 1;
                        if failures >= PUT_ATTEMPTS {
                            break;
                        }
                    }
                }
                continue;
            }
            match announce(&*wire, &gate, &shared, timing, &last).await {
                Progress::Sent(sent) => {
                    last = Some(sent.clone());
                    failures = 0;
                    reported = false;
                    // A change during the put is one more put, with the newest
                    // state; a stored wake for a state already sent is not.
                    if signature(&shared) == sent {
                        break;
                    }
                }
                Progress::Unchanged | Progress::Done => break,
                Progress::Gone | Progress::Unwatched => return,
                Progress::Busy => continue,
                Progress::Failed(attempted) => {
                    if !report_failure(&shared, &mut reported) {
                        return;
                    }
                    failures += 1;
                    // A new state is a new run with something else to say, so
                    // this run has nothing left to do.
                    if failures >= PUT_ATTEMPTS || signature(&shared) != attempted {
                        break;
                    }
                }
            }
        }
    }
}

/// Reports one failed run at most, so a retry is not a second failure the
/// window has to explain. `false` means nobody is reading any more.
fn report_failure(shared: &Shared, reported: &mut bool) -> bool {
    if *reported {
        return true;
    }
    if !shared.emit(ClusterEvent::Failed) {
        return false;
    }
    *reported = true;
    true
}

/// One attempt under the gate, bounded the way every other write is.
async fn announce<W: Wire>(
    wire: &W,
    gate: &Gate,
    shared: &Shared,
    timing: Timing,
    last: &Option<Signature>,
) -> Progress {
    let Ok(_guard) = tokio::time::timeout(timing.gate_wait, gate.take()).await else {
        return Progress::Busy;
    };
    if gate.withdrawn() {
        return Progress::Gone;
    }
    let attempt = signature(shared);
    if last.as_ref() == Some(&attempt) {
        return Progress::Unchanged;
    }
    match put_and_view(wire, gate, shared, timing).await {
        Ok(view) => {
            if shared.emit(ClusterEvent::View(Box::new(view))) {
                Progress::Sent(attempt)
            } else {
                Progress::Unwatched
            }
        }
        Err(ConnectError::Withdrawn) => Progress::Gone,
        // A device the account does not know yet has nobody to tell; the pump
        // announces the stage as soon as the connection id arrives.
        Err(ConnectError::NoConnectionId) => Progress::Done,
        Err(_) => Progress::Failed(attempt),
    }
}

#[cfg(test)]
mod tests;
