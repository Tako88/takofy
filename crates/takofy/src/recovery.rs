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
mod tests {
    use std::time::{Duration, Instant};

    use super::{Attempt, Change, Pacing, Recovery};
    use crate::playhead::ResumePoint;

    const URI: &str = "spotify:track:a";

    /// What the attempt after each failure waits, from the first one on.
    const WAITS_SECS: [u64; 9] = [0, 1, 2, 4, 8, 15, 30, 30, 30];

    fn secs(secs: u64) -> Duration {
        Duration::from_secs(secs)
    }

    fn point(position_ms: u32, play: bool) -> ResumePoint {
        ResumePoint {
            uri: URI.to_owned(),
            position_ms,
            play,
        }
    }

    /// A window that has just lost the link, holding `point`.
    fn recovering(base: Instant, point: Option<ResumePoint>) -> Recovery {
        let mut recovery = Recovery::new(Pacing::LIVE);
        recovery.lost(base, point);
        recovery
    }

    #[test]
    fn a_loss_is_due_at_once() {
        let base = Instant::now();
        let recovery = recovering(base, Some(point(1_000, true)));

        assert!(recovery.is_active(), "a loss starts recovery");
        assert!(recovery.due(base), "the first attempt is not waited for");
        assert_eq!(recovery.wake_at(), Some(base), "a poll finds it due now");
        assert!(recovery.has_resume(), "the point is there to reload");
    }

    #[test]
    fn a_drop_with_nothing_loaded_still_rebuilds_the_link() {
        let base = Instant::now();
        let mut recovery = recovering(base, None);

        assert!(recovery.is_active(), "there is a session to rebuild");
        assert!(!recovery.has_resume(), "there is nothing to reload");
        assert_eq!(recovery.recovered(), None, "recovery answers no track");
    }

    #[test]
    fn the_waits_follow_the_table_from_the_first_failure_on() {
        let base = Instant::now();
        let mut recovery = recovering(base, Some(point(0, true)));

        for (index, wait) in WAITS_SECS.iter().enumerate() {
            let now = base + secs(index as u64);
            recovery.failed(now);
            assert_eq!(
                recovery.wake_at(),
                Some(now + secs(*wait)),
                "the attempt after failure {}",
                index + 1
            );
        }
    }

    #[test]
    fn the_window_is_told_from_the_eighth_failure_on() {
        let base = Instant::now();
        let mut recovery = recovering(base, Some(point(0, true)));

        for failure in 1..=7 {
            assert_eq!(
                recovery.failed(base),
                Attempt::Retrying,
                "failure {failure} keeps trying quietly"
            );
        }
        assert_eq!(
            recovery.failed(base),
            Attempt::Stalled,
            "the eighth failure is said in words"
        );
        assert_eq!(
            recovery.failed(base),
            Attempt::Stalled,
            "and every failure after it"
        );
    }

    #[test]
    fn a_second_loss_while_recovering_changes_nothing() {
        let base = Instant::now();
        let mut recovery = recovering(base, Some(point(1_000, true)));
        let now = base + secs(5);
        recovery.failed(now);
        // One failure waits nothing; a second waits a second.
        recovery.failed(now);

        recovery.lost(now, Some(point(9_000, false)));

        assert_eq!(
            recovery.wake_at(),
            Some(now + secs(1)),
            "the schedule the first loss set stands"
        );
        recovery.failed(now);
        assert_eq!(
            recovery.wake_at(),
            Some(now + secs(2)),
            "the failures so far still count"
        );
        assert_eq!(
            recovery.recovered(),
            Some(point(1_000, true)),
            "the point a rebuild reloads is the one the first loss gave"
        );
    }

    #[test]
    fn an_interrupted_attempt_is_due_at_once_and_not_counted() {
        let base = Instant::now();
        let mut recovery = recovering(base, Some(point(0, true)));
        recovery.failed(base);
        let now = base + secs(1);
        assert!(recovery.due(now), "the wait after one failure is over");

        recovery.failed(now);
        assert!(!recovery.due(now), "the second wait is still running");
        recovery.interrupted(now);

        assert!(recovery.due(now), "an interrupted attempt retries now");
        assert_eq!(recovery.wake_at(), Some(now));
        recovery.failed(now);
        assert_eq!(
            recovery.wake_at(),
            Some(now + secs(2)),
            "the interruption was not a failure, so the third wait is the second one"
        );
    }

    #[test]
    fn recovering_answers_the_point_and_goes_idle() {
        let base = Instant::now();
        let mut recovery = recovering(base, Some(point(500, false)));

        assert_eq!(recovery.recovered(), Some(point(500, false)));
        assert!(!recovery.is_active(), "a rebuilt session is not recovering");
        assert_eq!(
            recovery.wake_at(),
            None,
            "an idle recovery wakes for nothing"
        );
        assert!(!recovery.due(base + secs(1)), "and no attempt is due");
        assert!(!recovery.has_resume());
    }

    #[test]
    fn a_deferred_point_replaces_the_one_and_pulls_the_attempt_forward() {
        let base = Instant::now();
        let mut recovery = recovering(base, Some(point(0, true)));
        recovery.failed(base);
        recovery.failed(base);
        let now = base + secs(1);
        assert!(recovery.due(now), "the second wait ends here");

        recovery.defer(point(42_000, false), now);

        assert_eq!(
            recovery.wake_at(),
            Some(now),
            "a command retries at once instead of after the wait"
        );
        assert!(
            recovery.has_resume(),
            "the new point is what a rebuild reloads"
        );
        assert_eq!(recovery.recovered(), Some(point(42_000, false)));
    }

    #[test]
    fn a_command_retargets_the_point() {
        let base = Instant::now();

        let mut paused = recovering(base, Some(point(30_000, true)));
        paused.adjust(Change::Play(false));
        assert_eq!(
            paused.recovered(),
            Some(point(30_000, false)),
            "a pause keeps the position and comes back paused"
        );

        let mut resumed = recovering(base, Some(point(30_000, false)));
        resumed.adjust(Change::Play(true));
        assert_eq!(
            resumed.recovered(),
            Some(point(30_000, true)),
            "a resume comes back playing"
        );

        let mut seeked = recovering(base, Some(point(0, true)));
        seeked.adjust(Change::Seek(12_000));
        assert_eq!(
            seeked.recovered(),
            Some(point(12_000, true)),
            "a seek moves the point and keeps the play flag"
        );

        let mut stopped = recovering(base, Some(point(0, true)));
        stopped.adjust(Change::Clear);
        assert!(!stopped.has_resume(), "a stop leaves nothing to reload");
        assert!(stopped.is_active(), "the link is still being rebuilt");
        assert_eq!(stopped.recovered(), None);
    }

    #[test]
    fn a_command_with_nothing_held_has_nothing_to_retarget() {
        let base = Instant::now();
        let mut idle = Recovery::new(Pacing::LIVE);
        idle.adjust(Change::Play(false));
        idle.adjust(Change::Seek(1_000));
        idle.adjust(Change::Clear);
        idle.defer(point(0, true), base);
        idle.interrupted(base);

        assert!(!idle.is_active(), "no command starts a recovery");
        assert!(!idle.has_resume());
        assert_eq!(idle.failed(base), Attempt::Retrying);
        assert_eq!(idle.wake_at(), None);
    }

    #[test]
    fn the_fast_pacing_is_the_live_schedule_compressed() {
        assert_eq!(
            Pacing::FAST.delays.len(),
            Pacing::LIVE.delays.len(),
            "a test sees the same number of waits as the window"
        );
        assert_eq!(
            Pacing::FAST.stall_after,
            Pacing::LIVE.stall_after,
            "and the same stall point"
        );
        for (fast, live) in Pacing::FAST.delays.iter().zip(Pacing::LIVE.delays) {
            assert!(fast <= live, "each wait is no longer than the live one");
        }
        assert!(Pacing::FAST.poll < Pacing::LIVE.poll);
        assert!(Pacing::FAST.attempt < Pacing::LIVE.attempt);
    }
}
