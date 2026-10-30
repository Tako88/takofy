//! The startup timings: one line per stage, once per run.
//!
//! A stage line says how long it took, how much it carried, and when it ended
//! relative to the run's start. Every field is a number or a fixed word, so a
//! line can never name a playlist or a track.

use std::collections::HashSet;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// The target the stage lines are logged under, so the binary can show them
/// without widening the log for anything else.
pub(crate) const STARTUP_LOG_TARGET: &str = "takofy::startup";

/// One step of a start, from the resume read to Home's first answer.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Stage {
    SignIn,
    Connect,
    Walk,
    Labels,
    Likes,
    Albums,
    Artists,
    Shows,
    HomeFirst,
}

impl Stage {
    /// The stage's name in the log. Total, so a new stage is a build failure
    /// until it has a word.
    fn word(self) -> &'static str {
        match self {
            Self::SignIn => "sign-in",
            Self::Connect => "connect",
            Self::Walk => "walk",
            Self::Labels => "labels",
            Self::Likes => "likes",
            Self::Albums => "albums",
            Self::Artists => "artists",
            Self::Shows => "shows",
            Self::HomeFirst => "home first",
        }
    }
}

/// One line: the stage's word, the time it took, what it carried, and when it
/// ended.
pub(super) fn line(stage: Stage, took: Duration, at: Duration, count: usize) -> String {
    format!(
        "{}: took {} ms, count {count}, at {} ms",
        stage.word(),
        took.as_millis(),
        at.as_millis()
    )
}

/// Which stages have logged in this run.
#[derive(Default)]
pub(super) struct Ledger {
    seen: HashSet<Stage>,
}

impl Ledger {
    /// Says whether this call is the stage's first, and records it when it is.
    pub(super) fn admit(&mut self, stage: Stage) -> bool {
        self.seen.insert(stage)
    }
}

/// A run's stages, and when the run began.
pub(crate) struct Startup {
    origin: Instant,
    ledger: Mutex<Ledger>,
}

impl Startup {
    pub(crate) fn new() -> Self {
        Self {
            origin: Instant::now(),
            ledger: Mutex::new(Ledger::default()),
        }
    }

    /// Logs one stage that ended now, if this is its first time in this run.
    ///
    /// `began` is when the stage started, so its start is `at - took`.
    pub(crate) fn record(&self, stage: Stage, began: Instant, count: usize) {
        {
            let mut ledger = self.ledger.lock().unwrap_or_else(PoisonError::into_inner);
            if !ledger.admit(stage) {
                return;
            }
        }
        let now = Instant::now();
        let took = now.saturating_duration_since(began);
        let at = now.saturating_duration_since(self.origin);
        log::info!(target: STARTUP_LOG_TARGET, "{}", line(stage, took, at, count));
    }
}

#[cfg(test)]
mod tests;
