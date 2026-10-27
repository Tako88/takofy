//! What this device says about itself: whether it can play, whether it holds
//! the sound, and the stage that follows from both.
//!
//! A flip wakes the publisher instead of announcing here, so every announcement
//! goes through one owner and one gate.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use tokio::sync::Notify;
use tokio::sync::futures::Notified;

use crate::connect::ClusterView;
use crate::loss::{Baseline, Judgement, judge};

/// What the account is told this device is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    /// It exists to read the list and steer.
    Controller,
    /// It can play, and is waiting to be picked.
    Ready,
    /// It holds the sound.
    Active,
}

/// What one pushed view means for the sound this window holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Observation {
    /// No activation to judge against.
    NotActive,
    /// The cluster names this window.
    Held,
    /// It names another device, but the push predates our activation.
    Stale,
    /// The sound is someone else's, and this activation is over.
    Lost { epoch: u64 },
}

/// One activation: what names it, and which pushes it may ignore.
#[derive(Debug, Clone, Copy)]
struct Active {
    epoch: u64,
    baseline: Baseline,
}

/// What letting go of the sound still owes the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Owed {
    /// Nothing was put, so the account never named this device.
    Nothing,
    /// An active put may have landed without an answer.
    Maybe,
    /// The activation that holds it, named so the caller can report the loss.
    Held { epoch: u64 },
}

impl Owed {
    /// Whether the account may still name this device as the holder.
    pub(crate) fn any(self) -> bool {
        !matches!(self, Self::Nothing)
    }
}

#[derive(Default)]
struct Slot {
    /// Counts activations, so a loss that crosses a landing names the right one.
    epoch: u64,
    active: Option<Active>,
    /// An active put that no answer or give-back has settled yet: a quit can
    /// drop the future mid-put, and the account may still have applied it.
    maybe_held: bool,
    /// A hand-over this device has sent and not yet seen settle. Cleared by
    /// whatever ends the hold it was sent from — the loss a view reports, a
    /// release, a new connection id — and by a move that comes back a failure.
    picking: bool,
}

#[derive(Default)]
pub(crate) struct Presence {
    playable: AtomicBool,
    slot: Mutex<Slot>,
    changed: Notify,
}

impl Presence {
    /// A panic elsewhere must not make this unusable: the stage is a few words
    /// of state, and refusing to read it would leave the account unpickable.
    fn slot(&self) -> MutexGuard<'_, Slot> {
        self.slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Returns whether the flag moved.
    pub(crate) fn set_playable(&self, playable: bool) -> bool {
        if self.playable.swap(playable, Ordering::SeqCst) == playable {
            return false;
        }
        // A stored permit, so a flip that lands while the publisher is putting
        // is not lost; the cost is one more put of the latest stage.
        self.changed.notify_one();
        true
    }

    pub(crate) fn playable(&self) -> bool {
        self.playable.load(Ordering::SeqCst)
    }

    /// Whether this window currently holds the account's sound.
    pub(crate) fn active(&self) -> bool {
        self.slot().active.is_some()
    }

    /// Records that an active put is going on the wire.
    ///
    /// The answer can be dropped with the future that awaited it, so the
    /// account may have taken the put with no activation recorded here; the
    /// hold is then still owed a give-back.
    pub(crate) fn put_issued(&self) {
        self.slot().maybe_held = true;
    }

    /// Records the account giving the sound back.
    ///
    /// A give-back the wire refused settles nothing: the account may still
    /// name this device, so the debt survives for the exit path.
    pub(crate) fn gave_back(&self, confirmed: bool) {
        if confirmed {
            self.slot().maybe_held = false;
        }
    }

    /// Records that the account taking the sound still has to arrive.
    ///
    /// The activation outlives the hand-over, so without this the device would
    /// read as the sound's owner until the cluster names the target, and a
    /// second pick would hand over from a device the server no longer counts.
    pub(crate) fn pick_issued(&self) {
        self.slot().picking = true;
    }

    /// Whether a hand-over is still waiting for the view that ends the hold.
    pub(crate) fn picking(&self) -> bool {
        self.slot().picking
    }

    /// Records that the hand-over ended with the sound still here.
    pub(crate) fn pick_settled(&self) {
        self.slot().picking = false;
    }

    /// Starts an activation and returns the epoch that names it.
    ///
    /// Only a put the server took calls this: the hold is reported after the
    /// account has it, never before.
    pub(crate) fn activate(&self, baseline: Baseline) -> u64 {
        let mut slot = self.slot();
        slot.maybe_held = false;
        // An activation is the account naming this device again, so whatever
        // move was in flight has been answered.
        slot.picking = false;
        slot.epoch = slot.epoch.wrapping_add(1);
        slot.active = Some(Active {
            epoch: slot.epoch,
            baseline,
        });
        slot.epoch
    }

    /// Gives the sound back, saying what the account is still owed.
    ///
    /// A put nobody answered left no activation, so the epoch alone cannot say
    /// whether the account names this device.
    pub(crate) fn release(&self) -> Owed {
        let mut slot = self.slot();
        // Nothing can be handed on from a hold that is over.
        slot.picking = false;
        match slot.active.take() {
            // The activation carries the debt now, so the marker is redundant.
            Some(active) => {
                slot.maybe_held = false;
                Owed::Held {
                    epoch: active.epoch,
                }
            }
            // A give-back the wire refused settles nothing, so the marker stays
            // until one is confirmed.
            None if slot.maybe_held => Owed::Maybe,
            None => Owed::Nothing,
        }
    }

    /// Gives the sound back because the registration lapsed.
    ///
    /// The device may have dropped out of the cluster, so re-asserting could
    /// take the sound from whoever picked it up; the caller reports a loss.
    pub(crate) fn release_for_new_connection(&self) -> Option<u64> {
        match self.release() {
            Owed::Held { epoch } => Some(epoch),
            Owed::Nothing | Owed::Maybe => None,
        }
    }

    /// Judges one pushed view, ending the activation it says is over.
    ///
    /// Clearing under the same lock that judged is what makes a loss report
    /// once: every later push finds nothing left to lose.
    pub(crate) fn observe(&self, view: &ClusterView, now: Instant) -> Observation {
        let mut slot = self.slot();
        let Some(active) = slot.active else {
            return Observation::NotActive;
        };
        match judge(view, active.baseline, now) {
            Judgement::Held => Observation::Held,
            Judgement::Stale => Observation::Stale,
            Judgement::Lost => {
                slot.active = None;
                slot.picking = false;
                Observation::Lost {
                    epoch: active.epoch,
                }
            }
        }
    }

    /// What the account should be told this device is.
    ///
    /// A stage that cannot hold the sound gives it back: neither an unplayable
    /// player nor a missing report is a loss, but neither can announce active.
    pub(crate) fn stage(&self, has_report: bool) -> Stage {
        let mut slot = self.slot();
        if slot.active.is_some() && !(self.playable() && has_report) {
            slot.active = None;
            slot.picking = false;
        }
        if slot.active.is_some() {
            Stage::Active
        } else if self.playable() {
            Stage::Ready
        } else {
            Stage::Controller
        }
    }

    pub(crate) fn changed(&self) -> Notified<'_> {
        self.changed.notified()
    }
}

#[cfg(test)]
mod tests;
