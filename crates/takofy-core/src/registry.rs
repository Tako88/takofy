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
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::testing::{FakeResponder, Log};

    /// Short enough to expire inside a test, long enough not to race an answer.
    const BUDGET: Duration = Duration::from_millis(50);

    /// Longer than any test takes, so only an answer can end a request.
    const PATIENT: Duration = Duration::from_secs(30);

    /// Long enough that an answer that must arrive has arrived.
    const ARRIVAL_WAIT: Duration = Duration::from_secs(1);

    /// Long enough for a wake that should have happened to reach the publisher.
    const QUIET: Duration = Duration::from_millis(20);

    fn ack(message_id: u32) -> Ack {
        Ack {
            message_id,
            sender: "phone".to_owned(),
        }
    }

    /// A registry whose budget expires inside the test.
    fn registry() -> Arc<Registry> {
        with_budget(BUDGET)
    }

    /// A registry that outlives the test, so no answer races its budget.
    fn patient() -> Arc<Registry> {
        with_budget(PATIENT)
    }

    fn with_budget(answer: Duration) -> Arc<Registry> {
        Arc::new(Registry::new(Timing {
            answer,
            ..Timing::default()
        }))
    }

    /// Waits until `check` holds, so no test has to guess at a duration.
    async fn until(what: &str, mut check: impl FnMut() -> bool) {
        let waited = tokio::time::timeout(ARRIVAL_WAIT, async {
            while !check() {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await;
        assert!(waited.is_ok(), "timed out waiting for {what}");
    }

    #[tokio::test]
    async fn an_answer_reaches_the_dealer_once_and_a_second_finds_nothing() {
        let log = Log::default();
        let registry = patient();
        let id = RequestId(1);
        registry.insert(id, ack(1), Box::new(FakeResponder::new(log.clone())));

        assert!(registry.answer(id, Verdict::Success));
        assert!(
            !registry.answer(id, Verdict::Failure),
            "a second answer finds nothing to answer"
        );
        assert_eq!(log.entries(), vec!["reply:success"]);
    }

    #[tokio::test]
    async fn answer_returns_false_when_the_entry_is_gone() {
        let registry = registry();

        assert!(
            !registry.answer(RequestId(7), Verdict::Success),
            "nothing was ever registered under that id"
        );
        assert!(registry.last_ack().is_none());
    }

    #[tokio::test]
    async fn ack_of_peeks_without_answering() {
        let log = Log::default();
        let registry = patient();
        let id = RequestId(2);
        registry.insert(id, ack(42), Box::new(FakeResponder::new(log.clone())));

        let peeked = registry.ack_of(id).expect("a pending request has an ack");
        assert_eq!(peeked.message_id, 42);
        assert_eq!(peeked.sender, "phone");
        assert!(log.entries().is_empty(), "a peek answers nothing");
        assert!(registry.last_ack().is_none(), "a peek is not an answer");

        assert!(registry.answer(id, Verdict::Success));
    }

    #[tokio::test]
    async fn ack_of_is_none_after_the_entry_expired() {
        let registry = registry();
        let id = RequestId(3);
        registry.insert(id, ack(3), Box::new(FakeResponder::new(Log::default())));

        until("the entry to expire", || registry.ack_of(id).is_none()).await;
    }

    #[tokio::test]
    async fn dropping_the_registry_never_answers_success() {
        let log = Log::default();
        let registry = patient();
        let id = RequestId(4);
        registry.insert(id, ack(4), Box::new(FakeResponder::new(log.clone())));

        drop(registry);

        assert_eq!(
            log.entries(),
            vec!["dropped"],
            "the dropped responder is the dealer's failure"
        );
    }

    #[tokio::test]
    async fn an_unanswered_request_fails_when_the_budget_runs_out() {
        let log = Log::default();
        let registry = registry();
        let id = RequestId(6);
        registry.insert(id, ack(6), Box::new(FakeResponder::new(log.clone())));

        until("the budget to answer failure", || {
            log.entries() == vec!["reply:failure"]
        })
        .await;

        assert!(
            !registry.answer(id, Verdict::Success),
            "the budget already answered"
        );
    }

    #[tokio::test]
    async fn an_order_answer_makes_its_ack_the_last_and_wakes_the_publisher() {
        let registry = patient();
        let id = RequestId(7);
        registry.insert(id, ack(70), Box::new(FakeResponder::new(Log::default())));

        assert!(registry.answer_order(id, Verdict::Success));
        assert_eq!(
            registry.last_ack().map(|ack| ack.message_id),
            Some(70),
            "the entry's own ack is the sticky one"
        );
        tokio::time::timeout(ARRIVAL_WAIT, registry.changed())
            .await
            .expect("an answer wakes the publisher");
    }

    #[tokio::test]
    async fn an_answer_that_is_not_an_order_leaves_the_ack_and_the_publisher_alone() {
        for (verdict, said) in [
            (Verdict::Success, "reply:success"),
            (Verdict::Failure, "reply:failure"),
        ] {
            let log = Log::default();
            let registry = patient();
            let id = RequestId(11);
            registry.insert(id, ack(110), Box::new(FakeResponder::new(log.clone())));

            assert!(registry.answer(id, verdict));

            assert_eq!(log.entries(), vec![said], "the phone hears its answer");
            assert!(
                registry.last_ack().is_none(),
                "a request that moved no transport is not the command a put names"
            );
            assert!(
                tokio::time::timeout(QUIET, registry.changed())
                    .await
                    .is_err(),
                "nothing else would ask for a put, so nothing is asked for"
            );
        }
    }

    #[tokio::test]
    async fn an_expired_request_does_not_become_the_last_ack() {
        let registry = registry();
        let id = RequestId(8);
        registry.insert(id, ack(80), Box::new(FakeResponder::new(Log::default())));

        until("the entry to expire", || registry.ack_of(id).is_none()).await;

        assert!(
            registry.last_ack().is_none(),
            "an expiry is not an ack the phone sent"
        );
    }

    #[tokio::test]
    async fn an_unanswered_order_fails_after_the_budget() {
        let log = Log::default();
        let registry = registry();
        let id = RequestId(9);
        registry.insert(id, ack(90), Box::new(FakeResponder::new(log.clone())));

        // An order that reached the window leaves its entry pending: only the
        // window's answer or this budget ends it.
        assert!(registry.expires(id).is_some());

        until("the order's budget to answer failure", || {
            log.entries() == vec!["reply:failure"]
        })
        .await;
        assert!(registry.expires(id).is_none(), "a budgeted order is gone");
    }

    #[tokio::test]
    async fn an_answered_order_no_longer_waits_for_its_budget() {
        let registry = patient();
        let id = RequestId(10);
        registry.insert(id, ack(100), Box::new(FakeResponder::new(Log::default())));

        assert!(registry.expires(id).is_some());
        assert!(registry.answer_order(id, Verdict::Success));
        assert!(
            registry.expires(id).is_none(),
            "an answered request is gone"
        );
    }
}
