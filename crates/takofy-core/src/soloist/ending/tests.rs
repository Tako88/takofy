use super::*;
use crate::soloist::position::Anchor;
use std::time::Duration;
use tokio::time::Instant;

fn expect(seen_attached: bool) -> Expect {
    Expect {
        generation: 1,
        uri: "spotify:track:a".to_owned(),
        seen_attached,
    }
}

fn attached_state(duration: Option<u32>, position: u32, speed: f64) -> (SoloistState, Instant) {
    let now = Instant::now();
    let mut state = SoloistState {
        duration_ms: duration,
        ..SoloistState::default()
    };
    state.clock.anchor(
        Anchor {
            position_ms: position,
            timestamp_ms: 1,
            speed,
        },
        now,
    );
    (state, now)
}

#[test]
fn idle_near_the_end_is_ended() {
    let (state, now) = attached_state(Some(100_000), 99_000, 1.0);
    let mut ending = Ending::default();
    ending.begin_load(1, now);
    let verdict = ending.observe(
        &Frame::PlaybackChanged {
            status: Status::Idle,
        },
        now,
        &state,
        &expect(true),
    );
    assert_eq!(verdict.events, vec![PlaybackEvent::Ended]);
    assert!(!verdict.pause);
}

#[test]
fn idle_with_an_unknown_duration_is_ended() {
    let (state, now) = attached_state(None, 0, 1.0);
    let mut ending = Ending::default();
    ending.begin_load(1, now);
    let verdict = ending.observe(
        &Frame::PlaybackState(super::super::frames::Snapshot {
            status: Status::Idle,
            duration_ms: None,
            ..Default::default()
        }),
        now,
        &state,
        &expect(true),
    );
    assert_eq!(verdict.events, vec![PlaybackEvent::Ended]);
}

#[test]
fn idle_short_of_the_end_is_an_external_stop() {
    let (state, now) = attached_state(Some(200_000), 30_000, 1.0);
    let mut ending = Ending::default();
    ending.begin_load(1, now);
    let verdict = ending.observe(
        &Frame::PlaybackChanged {
            status: Status::Idle,
        },
        now,
        &state,
        &expect(true),
    );
    assert_eq!(verdict.events, vec![PlaybackEvent::Stopped]);
}

#[test]
fn idle_before_attach_is_stale_and_ignored() {
    let (state, now) = attached_state(Some(100_000), 99_000, 1.0);
    let mut ending = Ending::default();
    ending.begin_load(1, now);
    let verdict = ending.observe(
        &Frame::PlaybackChanged {
            status: Status::Idle,
        },
        now,
        &state,
        &expect(false),
    );
    assert_eq!(verdict, Verdict::default());
}

#[test]
fn an_unexpected_track_change_ends_and_pauses() {
    let (state, now) = attached_state(Some(100_000), 1_000, 1.0);
    let mut ending = Ending::default();
    ending.begin_load(1, now);
    let frame = Frame::TrackChanged {
        uri: "spotify:track:b".to_owned(),
        duration_ms: Some(1_000),
    };
    let first = ending.observe(&frame, now, &state, &expect(true));
    assert_eq!(first.events, vec![PlaybackEvent::Ended]);
    assert!(first.pause);
    let second = ending.observe(&frame, now, &state, &expect(true));
    assert_eq!(second, Verdict::default());
}

#[test]
fn an_unexpected_track_change_before_attach_is_stale() {
    let (state, now) = attached_state(Some(100_000), 1_000, 1.0);
    let mut ending = Ending::default();
    ending.begin_load(1, now);
    let frame = Frame::TrackChanged {
        uri: "spotify:track:b".to_owned(),
        duration_ms: None,
    };
    assert_eq!(
        ending.observe(&frame, now, &state, &expect(false)),
        Verdict::default()
    );
}

#[test]
fn double_terminal_triggers_emit_one_ended() {
    let (state, now) = attached_state(Some(100_000), 99_000, 1.0);
    let mut ending = Ending::default();
    ending.begin_load(1, now);
    let idle = Frame::PlaybackChanged {
        status: Status::Idle,
    };
    assert_eq!(
        ending.observe(&idle, now, &state, &expect(true)).events,
        vec![PlaybackEvent::Ended]
    );
    assert_eq!(
        ending.observe(&idle, now, &state, &expect(true)),
        Verdict::default()
    );
}

#[test]
fn an_error_while_play_is_in_flight_is_unavailable() {
    let (state, now) = attached_state(Some(100_000), 0, 1.0);
    let mut ending = Ending::default();
    ending.begin_load(1, now);
    ending.play_sent(now);
    let verdict = ending.observe(
        &Frame::Error {
            message: "command requires authentication".to_owned(),
        },
        now,
        &state,
        &expect(false),
    );
    assert_eq!(verdict.events, vec![PlaybackEvent::Unavailable]);
}

#[test]
fn an_error_outside_a_play_is_not_attributed() {
    let (state, now) = attached_state(Some(100_000), 0, 1.0);
    let mut ending = Ending::default();
    ending.begin_load(1, now);
    let verdict = ending.observe(
        &Frame::Error {
            message: "unknown command".to_owned(),
        },
        now,
        &state,
        &expect(false),
    );
    assert_eq!(verdict, Verdict::default());
}

#[test]
fn no_play_result_within_the_command_budget_is_unavailable() {
    let (state, now) = attached_state(Some(100_000), 0, 1.0);
    let mut ending = Ending::default();
    ending.begin_load(1, now);
    ending.play_sent(now);
    let soon = ending.tick(
        now + COMMAND_BUDGET - Duration::from_millis(1),
        &state,
        &expect(false),
    );
    assert_eq!(soon, Verdict::default());
    let late = ending.tick(now + COMMAND_BUDGET, &state, &expect(false));
    assert_eq!(late.events, vec![PlaybackEvent::Unavailable]);
}

#[test]
fn a_silent_load_times_out_at_the_load_budget() {
    let (state, now) = attached_state(Some(100_000), 0, 1.0);
    let mut ending = Ending::default();
    ending.begin_load(1, now);
    ending.observe(
        &Frame::CommandResult {
            command: "play".to_owned(),
        },
        now,
        &state,
        &expect(false),
    );
    let soon = ending.tick(
        now + LOAD_BUDGET - Duration::from_millis(1),
        &state,
        &expect(false),
    );
    assert_eq!(soon, Verdict::default());
    let late = ending.tick(now + LOAD_BUDGET, &state, &expect(false));
    assert_eq!(late.events, vec![PlaybackEvent::Unavailable]);
}

#[test]
fn a_position_running_past_the_end_is_ended() {
    let (state, now) = attached_state(Some(10_000), 0, 1.0);
    let mut ending = Ending::default();
    ending.begin_load(1, now);
    let soon = ending.tick(now + Duration::from_millis(13_000), &state, &expect(true));
    assert_eq!(soon, Verdict::default());
    let late = ending.tick(now + Duration::from_millis(13_001), &state, &expect(true));
    assert_eq!(late.events, vec![PlaybackEvent::Ended]);
}

#[test]
fn a_paused_position_never_runs_past_the_end() {
    let (state, now) = attached_state(Some(10_000), 10_000, 0.0);
    let mut ending = Ending::default();
    ending.begin_load(1, now);
    assert_eq!(
        ending.tick(now + Duration::from_secs(60), &state, &expect(true)),
        Verdict::default()
    );
}

#[test]
fn a_new_generation_forgets_the_latch() {
    let (state, now) = attached_state(Some(100_000), 99_000, 1.0);
    let mut ending = Ending::default();
    ending.begin_load(1, now);
    let idle = Frame::PlaybackChanged {
        status: Status::Idle,
    };
    ending.observe(&idle, now, &state, &expect(true));

    ending.begin_load(2, now);
    let mut next = expect(true);
    next.generation = 2;
    assert_eq!(
        ending.observe(&idle, now, &state, &next).events,
        vec![PlaybackEvent::Ended]
    );
}
