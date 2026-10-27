use super::super::*;
use super::test_support::*;

/// The number of repeat clicks that reach `repeat` from Off.
fn clicks(repeat: Repeat) -> usize {
    match repeat {
        Repeat::Off => 0,
        Repeat::All => 1,
        Repeat::One => 2,
    }
}

/// A queue started on `count` tracks at `index`, in `repeat` mode.
fn started_in(count: usize, index: usize, repeat: Repeat) -> Queue {
    let mut queue = started(count, index);
    for _ in 0..clicks(repeat) {
        queue.cycle_repeat();
    }
    queue
}

#[test]
fn repeat_all_wraps_to_row_zero_on_a_natural_end() {
    let mut queue = started_in(3, 2, Repeat::All);
    let mut transport = Transport::default();
    clear_guard(&mut queue, &mut transport);

    assert_eq!(end(&mut queue, &mut transport), Some(play("t0")));
    assert_eq!(
        queue.index,
        Some(0),
        "a wrap leaves the current row at zero"
    );
}

#[test]
fn repeat_all_manual_next_wraps_and_can_next_is_true_on_the_last_row() {
    let mut queue = started_in(3, 2, Repeat::All);
    let mut transport = Transport::default();

    assert!(queue.can_next(), "All always has somewhere to go");
    assert_eq!(queue.next(&mut transport), Some(play("t0")));
}

#[test]
fn can_next_agrees_with_next_for_every_state() {
    let mut transport = Transport::default();
    for repeat in [Repeat::Off, Repeat::All, Repeat::One] {
        for count in 0..=4 {
            for index in 0..count {
                let mut queue = started_in(count, index, repeat);
                let can = queue.can_next();
                assert_eq!(
                    queue.next(&mut transport).is_some(),
                    can,
                    "{repeat:?} on {count} rows at {index}"
                );
            }
        }
        let mut armed = armed(3);
        for _ in 0..clicks(repeat) {
            armed.cycle_repeat();
        }
        assert!(!armed.can_next(), "{repeat:?} on an armed queue");
        assert_eq!(armed.next(&mut transport), None, "{repeat:?} armed");
    }
}

#[test]
fn repeat_one_replays_the_current_row_and_arms_the_guard() {
    let mut queue = started_in(3, 1, Repeat::One);
    let mut transport = Transport::default();
    clear_guard(&mut queue, &mut transport);

    assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));
    assert_eq!(queue.index, Some(1));
    assert!(queue.awaiting_start, "a replay is a fresh play");
}

#[test]
fn a_stale_ended_after_a_replay_is_ignored() {
    let mut queue = started_in(3, 1, Repeat::One);
    let mut transport = Transport::default();
    clear_guard(&mut queue, &mut transport);

    assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));
    assert_eq!(
        end(&mut queue, &mut transport),
        None,
        "the end that follows a replay belongs to the load before it"
    );
    clear_guard(&mut queue, &mut transport);
    assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));
}

#[test]
fn repeat_one_manual_next_advances_and_stops_at_the_end() {
    let mut queue = started_in(2, 0, Repeat::One);
    let mut transport = Transport::default();

    assert_eq!(queue.next(&mut transport), Some(play("t1")));
    assert!(!queue.can_next(), "One governs the end, not the button");
    assert_eq!(queue.next(&mut transport), None);
}

#[test]
fn repeat_one_unavailable_moves_on_like_off() {
    let mut queue = started_in(3, 0, Repeat::One);
    let mut transport = Transport::default();
    assert_eq!(skip(&mut queue, &mut transport), Some(play("t1")));

    let mut last = started_in(2, 1, Repeat::One);
    assert_eq!(
        skip(&mut last, &mut transport),
        None,
        "an unplayable last row stops even under One"
    );
}

#[test]
fn repeat_one_that_ends_without_a_position_stops_at_the_cap() {
    let mut queue = started_in(1, 0, Repeat::One);
    let mut transport = Transport::default();

    for lap in 1..=MAX_CONSECUTIVE_SKIPS {
        clear_guard(&mut queue, &mut transport);
        assert_eq!(
            end(&mut queue, &mut transport),
            Some(play("t0")),
            "replay {lap}"
        );
    }
    clear_guard(&mut queue, &mut transport);
    assert_eq!(
        end(&mut queue, &mut transport),
        None,
        "a loop that never reports a position must stop at the cap"
    );
}

#[test]
fn a_healthy_repeat_one_loop_never_hits_the_cap() {
    let mut queue = started_in(1, 0, Repeat::One);
    let mut transport = Transport::default();

    for lap in 0..30 {
        clear_guard(&mut queue, &mut transport);
        feed(
            &mut queue,
            &mut transport,
            TransportUpdate::Position {
                position_ms: 1_000,
                paused: false,
            },
        );
        assert_eq!(
            end(&mut queue, &mut transport),
            Some(play("t0")),
            "lap {lap}"
        );
    }
}

#[test]
fn a_duration_resets_the_skip_run() {
    let mut queue = started_in(1, 0, Repeat::One);
    let mut transport = Transport::default();

    for lap in 0..30 {
        clear_guard(&mut queue, &mut transport);
        feed(
            &mut queue,
            &mut transport,
            TransportUpdate::Duration {
                duration_ms: 222_000,
            },
        );
        assert_eq!(
            end(&mut queue, &mut transport),
            Some(play("t0")),
            "lap {lap}"
        );
        assert_eq!(queue.skips, 1, "a known duration must reset the streak");
    }
}

#[test]
fn repeat_all_over_a_failing_queue_stops_at_the_cap() {
    let mut queue = started_in(3, 0, Repeat::All);
    let mut transport = Transport::default();

    for (lap, uri) in ["t1", "t2", "t0", "t1", "t2"].into_iter().enumerate() {
        assert_eq!(
            skip(&mut queue, &mut transport),
            Some(play(uri)),
            "lap {lap}"
        );
    }
    assert_eq!(
        skip(&mut queue, &mut transport),
        None,
        "a failing queue that wraps must still stop at the cap"
    );
}

#[test]
fn a_silent_wrap_on_one_row_counts_toward_the_cap() {
    let mut queue = started_in(1, 0, Repeat::All);
    let mut transport = Transport::default();

    for lap in 1..=MAX_CONSECUTIVE_SKIPS {
        clear_guard(&mut queue, &mut transport);
        assert_eq!(
            end(&mut queue, &mut transport),
            Some(play("t0")),
            "wrap {lap}"
        );
    }
    clear_guard(&mut queue, &mut transport);
    assert_eq!(end(&mut queue, &mut transport), None, "capped");
}

#[test]
fn repeat_one_on_an_armed_queue_ignores_ended_and_toggle_starts_row_zero() {
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(queue.append(tracks(3)), Some(0..3));
    queue.cycle_repeat();
    queue.cycle_repeat();
    assert_eq!(queue.repeat(), Repeat::One);

    assert_eq!(end(&mut queue, &mut transport), None);
    assert!(queue.armed(), "nothing was playing, so nothing repeats");
    assert_eq!(queue.toggle(&mut transport), Some(play("t0")));
}

#[test]
fn changing_repeat_never_touches_index_guard_or_skips() {
    let mut queue = started(6, 1);
    let mut transport = Transport::default();
    clear_guard(&mut queue, &mut transport);
    assert_eq!(
        feed(&mut queue, &mut transport, TransportUpdate::Unavailable),
        Some(play("t2"))
    );
    let before = (queue.index, queue.awaiting_start, queue.skips);

    for _ in 0..3 {
        queue.cycle_repeat();
        assert_eq!((queue.index, queue.awaiting_start, queue.skips), before);
    }
    assert_eq!(queue.repeat(), Repeat::Off);
}
