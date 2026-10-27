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
