use super::super::*;
use super::test_support::*;

use crate::message::Failure;

#[test]
fn ended_advances() {
    let tracks = tracks(2);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.start(&tracks, 0, None, &mut transport),
        Some(play("t0"))
    );

    clear_guard(&mut queue, &mut transport);
    assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));
}

#[test]
fn ended_at_the_last_entry_is_idle_with_no_command() {
    let tracks = tracks(1);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.start(&tracks, 0, None, &mut transport),
        Some(play("t0"))
    );

    clear_guard(&mut queue, &mut transport);
    assert_eq!(end(&mut queue, &mut transport), None);
    assert!(!queue.can_next());
}

#[test]
fn unavailable_advances_and_the_chain_stops_at_the_cap() {
    let tracks = tracks(12);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.start(&tracks, 0, None, &mut transport),
        Some(play("t0"))
    );

    for i in 1..=MAX_CONSECUTIVE_SKIPS {
        assert_eq!(
            skip(&mut queue, &mut transport),
            Some(play(&format!("t{i}"))),
            "skip {i}"
        );
    }
    assert_eq!(
        skip(&mut queue, &mut transport),
        None,
        "the run past the cap must stop"
    );
}

#[test]
fn a_non_paused_position_resets_the_skip_count() {
    let tracks = tracks(16);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.start(&tracks, 0, None, &mut transport),
        Some(play("t0"))
    );

    for _ in 0..3 {
        skip(&mut queue, &mut transport);
    }
    feed(
        &mut queue,
        &mut transport,
        TransportUpdate::Position {
            position_ms: 1_000,
            paused: false,
        },
    );

    for i in 4..=8 {
        assert_eq!(
            skip(&mut queue, &mut transport),
            Some(play(&format!("t{i}"))),
            "a playing position must reset the streak"
        );
    }
    assert_eq!(skip(&mut queue, &mut transport), None);
}

#[test]
fn a_stale_ended_before_loading_is_ignored() {
    let tracks = tracks(3);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.start(&tracks, 0, None, &mut transport),
        Some(play("t0"))
    );

    assert_eq!(
        end(&mut queue, &mut transport),
        None,
        "an end before the new load's first event is the old track's"
    );
    clear_guard(&mut queue, &mut transport);
    assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));
}

#[test]
fn failed_clears_the_guard_and_never_advances() {
    let tracks = tracks(3);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.start(&tracks, 0, None, &mut transport),
        Some(play("t0"))
    );

    let failed = transport_update(TransportUpdate::Failed(Failure::Other));
    assert_eq!(queue.apply(&failed, &mut transport), None);
    assert_eq!(
        end(&mut queue, &mut transport),
        Some(play("t1")),
        "the failure must have cleared the guard"
    );
}

#[test]
fn previous_goes_back_within_the_threshold() {
    let tracks = tracks(3);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.start(&tracks, 0, None, &mut transport),
        Some(play("t0"))
    );
    clear_guard(&mut queue, &mut transport);
    assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));

    assert_eq!(queue.previous(4_000, &mut transport), Some(play("t0")));
}

#[test]
fn previous_restarts_past_the_threshold() {
    let tracks = tracks(3);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.start(&tracks, 1, None, &mut transport),
        Some(play("t1"))
    );
    seekable(&mut queue, &mut transport, 4_001);

    assert_eq!(
        queue.previous(4_001, &mut transport),
        Some(Command::Seek { position_ms: 0 })
    );
}

#[test]
fn previous_on_the_first_entry_restarts() {
    let tracks = tracks(3);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.start(&tracks, 0, None, &mut transport),
        Some(play("t0"))
    );
    seekable(&mut queue, &mut transport, 1_000);

    assert_eq!(
        queue.previous(1_000, &mut transport),
        Some(Command::Seek { position_ms: 0 })
    );
}

#[test]
fn previous_replays_the_first_entry_when_not_seekable() {
    let tracks = tracks(3);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.start(&tracks, 0, None, &mut transport),
        Some(play("t0"))
    );
    clear_guard(&mut queue, &mut transport);

    assert_eq!(queue.previous(1_000, &mut transport), Some(play("t0")));
}

#[test]
fn a_seek_restart_leaves_the_guard_unset() {
    let tracks = tracks(3);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.start(&tracks, 0, None, &mut transport),
        Some(play("t0"))
    );
    seekable(&mut queue, &mut transport, 4_001);

    assert_eq!(
        queue.previous(4_001, &mut transport),
        Some(Command::Seek { position_ms: 0 })
    );
    assert_eq!(
        end(&mut queue, &mut transport),
        Some(play("t1")),
        "a restart emits no load, so no guard may be left behind"
    );
}

#[test]
fn a_position_clears_the_guard() {
    let tracks = tracks(3);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.start(&tracks, 0, None, &mut transport),
        Some(play("t0"))
    );

    feed(
        &mut queue,
        &mut transport,
        TransportUpdate::Position {
            position_ms: 1,
            paused: false,
        },
    );
    assert_eq!(
        end(&mut queue, &mut transport),
        Some(play("t1")),
        "the load that emits no other event must clear the guard too"
    );
}

#[test]
fn a_second_loading_does_not_count_as_a_second_load() {
    let tracks = tracks(3);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.start(&tracks, 0, None, &mut transport),
        Some(play("t0"))
    );

    clear_guard(&mut queue, &mut transport);
    clear_guard(&mut queue, &mut transport);
    assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));
    assert_eq!(end(&mut queue, &mut transport), None, "exactly one advance");
}

#[test]
fn next_at_the_end_is_none() {
    let tracks = tracks(2);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.start(&tracks, 1, None, &mut transport),
        Some(play("t1"))
    );

    assert!(!queue.can_next());
    assert_eq!(queue.next(&mut transport), None);
    assert_eq!(
        queue.next(&mut transport),
        None,
        "a refused next must not move the index"
    );
}

#[test]
fn a_manual_next_after_a_cap_stop_gets_a_full_fresh_streak() {
    let tracks = tracks(16);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.start(&tracks, 0, None, &mut transport),
        Some(play("t0"))
    );

    for _ in 0..=MAX_CONSECUTIVE_SKIPS {
        skip(&mut queue, &mut transport);
    }
    assert_eq!(skip(&mut queue, &mut transport), None, "capped");

    assert_eq!(queue.next(&mut transport), Some(play("t6")));
    for i in 7..=11 {
        assert_eq!(
            skip(&mut queue, &mut transport),
            Some(play(&format!("t{i}"))),
            "the user's next must have reset the streak"
        );
    }
    assert_eq!(skip(&mut queue, &mut transport), None);
}
