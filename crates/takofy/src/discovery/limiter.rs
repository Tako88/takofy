//! The outside's pacing, as pure state over injected instants.
//!
//! The published budget is 50 calls per five seconds; the limiter admits 40 so
//! three rows' worst case stays inside it. A failure pauses the outside, and a
//! pause longer than the lookup may wait is refused rather than slept through.

use std::collections::VecDeque;
use std::time::Duration;

use tokio::time::Instant;

use super::{BACKOFF_CEILING, MAX_WAIT};

/// Calls admitted per window.
pub(crate) const LIMIT: usize = 40;
/// The window the limit is measured over.
pub(crate) const WINDOW: Duration = Duration::from_secs(5);
/// The first pause after a failure; later ones double.
const BACKOFF_START: Duration = Duration::from_secs(2);

/// What a request may do at an instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    Go,
    Wait(Duration),
    Refuse,
}

/// The sliding window and the failure pause.
#[derive(Debug, Clone, Default)]
pub(crate) struct Limiter {
    admitted: VecDeque<Instant>,
    pause_until: Option<Instant>,
    step: Option<Duration>,
}

impl Limiter {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Whether the request may go now, must wait, or must be refused.
    pub(crate) fn admit(&mut self, now: Instant) -> Decision {
        if let Some(until) = self.pause_until {
            if until > now {
                let wait = until.saturating_duration_since(now);
                return if wait > MAX_WAIT {
                    Decision::Refuse
                } else {
                    Decision::Wait(wait)
                };
            }
            self.pause_until = None;
        }
        while let Some(&oldest) = self.admitted.front() {
            if now.saturating_duration_since(oldest) < WINDOW {
                break;
            }
            self.admitted.pop_front();
        }
        if let Some(&oldest) = self.admitted.front()
            && self.admitted.len() >= LIMIT
        {
            let wait = (oldest + WINDOW).saturating_duration_since(now);
            return if wait > MAX_WAIT {
                Decision::Refuse
            } else {
                Decision::Wait(wait)
            };
        }
        self.admitted.push_back(now);
        Decision::Go
    }

    /// Pauses the outside after a failure.
    ///
    /// `Retry-After` wins when the response named one, capped at
    /// [`BACKOFF_CEILING`]; otherwise each consecutive failure doubles from
    /// two seconds. The window is untouched: pacing is not a failure.
    pub(crate) fn backoff(&mut self, now: Instant, retry_after: Option<Duration>) {
        let pause = match retry_after {
            Some(wait) => wait.min(BACKOFF_CEILING),
            None => {
                let next = self
                    .step
                    .map_or(BACKOFF_START, |step| (step * 2).min(BACKOFF_CEILING));
                self.step = Some(next);
                next
            }
        };
        let until = now + pause;
        self.pause_until = Some(self.pause_until.map_or(until, |held| held.max(until)));
    }

    /// Ends a failure chain after a request answered.
    pub(crate) fn ok(&mut self) {
        self.step = None;
        self.pause_until = None;
    }

    #[cfg(test)]
    pub(crate) fn paused_until(&self) -> Option<Instant> {
        self.pause_until
    }
}

#[cfg(test)]
mod tests;
