//! Requests in flight: who will answer one, what its ack is, and when the
//! budget gives up on it.
//!
//! Every path out of here answers at most once. A request nobody answers is
//! answered `Failure` by its timer, so a stuck window cannot hang the phone;
//! dropping the registry drops the responders unanswered, which the dealer
//! reads as failure too.
//!
//! Only an order's answer becomes the ack the next put carries: a hand-over's
//! ack rides the put that answers it, and a refusal has nothing to add.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use takofy_model::RequestId;
use tokio::sync::Notify;
use tokio::sync::futures::Notified;

use crate::watch::Timing;

/// What one request is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The request was carried out: a hand-over the server took, or an order
    /// the window applied.
    Success,
    Failure,
}

/// What the dealer said the request was, so the next announcement can carry it
/// as its last command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Ack {
    pub(crate) message_id: u32,
    pub(crate) sender: String,
}

/// The way one request answers the dealer.
pub(crate) trait Responder: Send {
    fn answer(&mut self, verdict: Verdict);
}

struct Entry {
    ack: Ack,
    responder: Box<dyn Responder>,
    /// When the budget answers this request, so a reader can tell whether an
    /// answer is still worth waiting for.
    expires: Instant,
}

#[derive(Default)]
struct State {
    entries: HashMap<RequestId, Entry>,
    last_ack: Option<Ack>,
}

pub(crate) struct Registry {
    answer: Duration,
    state: Mutex<State>,
    changed: Notify,
}

impl Registry {
    /// `timing`'s answer budget is how long one request may wait for one.
    pub(crate) fn new(timing: Timing) -> Self {
        Self {
            answer: timing.answer,
            state: Mutex::new(State::default()),
            changed: Notify::new(),
        }
    }

    /// Registers a request and starts the timer that fails it.
    ///
    /// The timer holds only a weak count, so dropping the registry frees its
    /// responders at once instead of waiting out a budget nobody will read.
    pub(crate) fn insert(self: &Arc<Self>, id: RequestId, ack: Ack, responder: Box<dyn Responder>) {
        let expires = Instant::now()
            .checked_add(self.answer)
            .unwrap_or_else(Instant::now);
        self.state().entries.insert(
            id,
            Entry {
                ack,
                responder,
                expires,
            },
        );
        let budget = self.answer;
        let registry = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::time::sleep(budget).await;
            expire(&registry, id);
        });
    }

    /// Reads a request's ack without answering it, so a put can carry it.
    pub(crate) fn ack_of(&self, id: RequestId) -> Option<Ack> {
        self.state().entries.get(&id).map(|entry| entry.ack.clone())
    }

    /// When the budget answers the request, while it is still pending.
    pub(crate) fn expires(&self, id: RequestId) -> Option<Instant> {
        self.state().entries.get(&id).map(|entry| entry.expires)
    }

    /// Answers the dealer, and returns whether the entry was still pending.
    ///
    /// The answer is not carried by any announcement and asks for no put: the
    /// phone already has its reply, and a refusal changes nothing this device
    /// has to say about itself.
    pub(crate) fn answer(&self, id: RequestId, verdict: Verdict) -> bool {
        let Some(mut entry) = self.state().entries.remove(&id) else {
            return false;
        };
        entry.responder.answer(verdict);
        true
    }

    /// Answers an order, whose ack the next announcement carries.
    ///
    /// The publisher is woken because the transport has just moved: without a
    /// put the phone's display of this device stays where the order found it.
    pub(crate) fn answer_order(&self, id: RequestId, verdict: Verdict) -> bool {
        let mut state = self.state();
        let Some(mut entry) = state.entries.remove(&id) else {
            return false;
        };
        state.last_ack = Some(entry.ack.clone());
        drop(state);
        entry.responder.answer(verdict);
        self.changed.notify_one();
        true
    }

    /// Answers `Failure`: the request was seen and cannot be taken.
    pub(crate) fn refuse(&self, id: RequestId) {
        self.answer(id, Verdict::Failure);
    }

    /// The ack of the last order answered here, which an announcement carries.
    pub(crate) fn last_ack(&self) -> Option<Ack> {
        self.state().last_ack.clone()
    }

    /// Waits for an order's answer, so it reaches the next put.
    ///
    /// With `command_acks` announced, the phone reads the answer from the state
    /// this device puts next, and nothing else would ask for that put.
    pub(crate) fn changed(&self) -> Notified<'_> {
        self.changed.notified()
    }

    /// A panic elsewhere must not make the registry unusable: a request is
    /// either in the map or not.
    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Fails a request whose budget ran out. Its ack is not the last order answered
/// here, so the announcement is untouched.
fn expire(registry: &Weak<Registry>, id: RequestId) {
    let Some(registry) = registry.upgrade() else {
        return;
    };
    let entry = registry.state().entries.remove(&id);
    if let Some(mut entry) = entry {
        entry.responder.answer(Verdict::Failure);
    }
}

#[cfg(test)]
mod tests;
