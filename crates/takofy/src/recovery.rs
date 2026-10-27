//! When a rebuild is attempted, and what the window is told while it fails.
//!
//! The bridge does the I/O; this is the schedule alone, driven by an `Instant`
//! the caller injects, so every wait and the point where the window is told in
//! words are testable without a clock.

use std::time::{Duration, Instant};

use crate::playhead::ResumePoint;

/// How often the link is polled, how failures back off, and how long one
/// attempt may take.
#[derive(Debug, Clone, Copy)]
pub struct Pacing {
    /// The wait between two looks at whether the session is still alive.
    pub poll: Duration,
    /// The wait before each attempt, from the first failure on; the last entry
    /// repeats.
    pub delays: &'static [Duration],
    /// The failure count from which the window is told in words.
    pub stall_after: u32,
    /// The budget for one rebuild attempt.
    pub attempt: Duration,
}

impl Pacing {
    /// The schedule the window runs with.
    pub const LIVE: Self = Self {
        poll: Duration::from_secs(1),
        delays: &[
            Duration::ZERO,
            Duration::from_secs(1),
            Duration::from_secs(2),
            Duration::from_secs(4),
            Duration::from_secs(8),
            Duration::from_secs(15),
            Duration::from_secs(30),
        ],
        stall_after: 8,
        attempt: Duration::from_secs(15),
    };

    /// The same schedule with waits short enough for a test.
    #[cfg(test)]
    pub const FAST: Self = Self {
        poll: Duration::from_millis(10),
        delays: &[
            Duration::ZERO,
            Duration::from_millis(1),
            Duration::from_millis(2),
            Duration::from_millis(4),
            Duration::from_millis(8),
            Duration::from_millis(15),
            Duration::from_millis(30),
        ],
        stall_after: 8,
        attempt: Duration::from_millis(100),
    };

    /// What the attempt after `failures` failures waits.
    fn wait(&self, failures: u32) -> Duration {
        let last = self.delays.len().saturating_sub(1);
        let index = usize::try_from(failures.saturating_sub(1))
            .unwrap_or(usize::MAX)
            .min(last);
        self.delays.get(index).copied().unwrap_or_default()
    }
}

/// What one failed attempt means for the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attempt {
    /// Another attempt is scheduled, and the window is not told yet.
    Retrying,
    /// The rebuild has failed often enough that the window must say so.
    Stalled,
}

/// What a command sent while recovering does to the point a rebuild reloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// The track now moves, or now sits still. `false` for a pause.
    Play(bool),
    /// The track now sits at this many milliseconds.
    Seek(u32),
    /// There is nothing left to resume.
    Clear,
}

/// A rebuild in progress: where to resume, how many attempts have failed, and
/// when the next one falls due.
#[derive(Debug)]
pub struct Recovery {
    pacing: Pacing,
    active: Option<State>,
}

#[derive(Debug)]
struct State {
    /// `None` when the drop happened with nothing loaded. The rebuild still
    /// runs, so the link comes back, but nothing is reloaded.
    point: Option<ResumePoint>,
    failures: u32,
    next_at: Instant,
}

impl Recovery {
    pub fn new(pacing: Pacing) -> Self {
        Self {
            pacing,
            active: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Enters recovery at `point`, with the first attempt due now.
    ///
    /// A second loss while recovering is the same recovery: the attempt count
    /// and the point stay, because they are about the drop already being
    /// handled.
    pub fn lost(&mut self, now: Instant, point: Option<ResumePoint>) {
        if self.active.is_some() {
            return;
        }
        self.active = Some(State {
            point,
            failures: 0,
            next_at: now,
        });
    }

    /// Whether an attempt falls due at `now`.
    pub fn due(&self, now: Instant) -> bool {
        self.active
            .as_ref()
            .is_some_and(|state| now >= state.next_at)
    }

    /// When the next attempt falls due, or `None` while idle.
    pub fn wake_at(&self) -> Option<Instant> {
        self.active.as_ref().map(|state| state.next_at)
    }

    /// Counts a failed attempt and schedules the next one.
    ///
    /// The count is kept while recovery lasts, so the wait grows even when the
    /// failures happened in different attempts. An idle recovery has nothing to
    /// fail: the answer is `Retrying` and nothing changes.
    pub fn failed(&mut self, now: Instant) -> Attempt {
        let Some(state) = self.active.as_mut() else {
            return Attempt::Retrying;
        };
        state.failures = state.failures.saturating_add(1);
        state.next_at = now + self.pacing.wait(state.failures);
        if state.failures >= self.pacing.stall_after {
            Attempt::Stalled
        } else {
            Attempt::Retrying
        }
    }

    /// A command interrupted the attempt. That is not a failure, and the next
    /// attempt is due at once.
    pub fn interrupted(&mut self, now: Instant) {
        if let Some(state) = self.active.as_mut() {
            state.next_at = now;
        }
    }

    /// Ends recovery, answering what to resume, and goes idle.
    pub fn recovered(&mut self) -> Option<ResumePoint> {
        self.active.take().and_then(|state| state.point)
    }

    /// Retargets the point and makes the next attempt due now, so a command
    /// during recovery is not waited out.
    pub fn defer(&mut self, point: ResumePoint, now: Instant) {
        if let Some(state) = self.active.as_mut() {
            state.point = Some(point);
            state.next_at = now;
        }
    }

    /// Retargets the point without touching the schedule.
    pub fn adjust(&mut self, change: Change) {
        let Some(state) = self.active.as_mut() else {
            return;
        };
        match change {
            Change::Clear => state.point = None,
            Change::Play(play) => {
                if let Some(point) = state.point.as_mut() {
                    point.play = play;
                }
            }
            Change::Seek(position_ms) => {
                if let Some(point) = state.point.as_mut() {
                    point.position_ms = position_ms;
                }
            }
        }
    }

    /// The point a rebuild reloads, borrowed so the attempt can read it without
    /// ending recovery.
    pub fn point(&self) -> Option<&ResumePoint> {
        self.active.as_ref().and_then(|state| state.point.as_ref())
    }

    /// Whether a rebuild has a track to reload.
    pub fn has_resume(&self) -> bool {
        self.point().is_some()
    }
}

#[cfg(test)]
mod tests;
