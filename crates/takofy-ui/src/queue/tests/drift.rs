//! A radio's drift: asking for rows that continue what it has played.

use super::super::*;
use super::test_support::*;

use crate::message::Failure;

/// One track row, whose address is the kind a radio continues from.
fn radio_track(id: &str) -> TrackEntry {
    track(&format!("spotify:track:{id}"))
}

fn radio_tracks(count: usize) -> Vec<TrackEntry> {
    (0..count)
        .map(|index| radio_track(&format!("t{index}")))
        .collect()
}

/// Rows no queue holds, so every one of them is fresh.
fn fresh(count: usize) -> Vec<TrackEntry> {
    (0..count)
        .map(|index| radio_track(&format!("f{index}")))
        .collect()
}

fn uris_of(entries: &[TrackEntry]) -> Vec<String> {
    entries.iter().map(|entry| entry.uri.clone()).collect()
}

/// A radio started on `count` rows and moved onto `index`.
fn tuned(count: usize, index: usize) -> (Queue, Transport) {
    let tracks = radio_tracks(count);
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    assert!(queue.start_radio(&tracks, &mut transport).is_some());
    if index > 0 {
        assert!(queue.jump(index, &mut transport).is_some());
    }
    (queue, transport)
}

/// The token, context and recent rows of the ask the queue just made.
fn wanted(queue: &mut Queue) -> (u64, String, Vec<String>) {
    match queue.drift_wants() {
        Some(Command::ContinueRadio {
            ask,
            context,
            recent,
        }) => (ask, context, recent),
        other => panic!("the radio asked for {other:?}"),
    }
}

#[test]
fn a_started_list_never_wants() {
    let tracks = radio_tracks(6);
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    assert!(queue.start(&tracks, 0, None, &mut transport).is_some());
    assert!(queue.jump(4, &mut transport).is_some());

    assert_eq!(queue.drift_wants(), None, "a plain list is not a radio");
}

#[test]
fn a_radio_wants_only_below_five_rows_remaining() {
    let (mut queue, _transport) = tuned(100, 94);
    assert_eq!(
        queue.drift_wants(),
        None,
        "five rows ahead are still enough"
    );

    let (mut queue, _transport) = tuned(100, 95);
    assert!(
        matches!(queue.drift_wants(), Some(Command::ContinueRadio { .. })),
        "four rows ahead are not enough"
    );
}

#[test]
fn the_want_names_the_current_row_as_its_context() {
    let (mut queue, _transport) = tuned(10, 8);

    let (ask, context, _recent) = wanted(&mut queue);

    assert_eq!(ask, 1);
    assert_eq!(context, "spotify:track:t8");
}

#[test]
fn recent_is_the_last_fifty_rows_nearest_first_without_the_current_one() {
    let (mut queue, _transport) = tuned(100, 95);

    let (_ask, _context, recent) = wanted(&mut queue);

    assert_eq!(recent.len(), 50, "the read takes fifty rows");
    assert_eq!(
        recent.first().map(String::as_str),
        Some("spotify:track:t94")
    );
    assert_eq!(recent.last().map(String::as_str), Some("spotify:track:t45"));
}

#[test]
fn recent_keeps_only_rows_that_name_a_track() {
    let mut tracks = radio_tracks(6);
    tracks[1] = track("spotify:episode:e0");
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    assert!(queue.start_radio(&tracks, &mut transport).is_some());
    assert!(queue.jump(4, &mut transport).is_some());

    let (_ask, _context, recent) = wanted(&mut queue);

    assert_eq!(
        recent,
        uris(&["spotify:track:t3", "spotify:track:t2", "spotify:track:t0"])
    );
}

#[test]
fn each_ask_carries_a_new_token() {
    let (mut queue, _transport) = tuned(6, 2);

    assert_eq!(wanted(&mut queue).0, 1);
    queue.land_drift(1, &fresh(1));
    assert_eq!(wanted(&mut queue).0, 2, "the answered radio asked again");
}

#[test]
fn a_current_row_that_names_no_track_does_not_ask_and_spends_no_token() {
    let mut tracks = radio_tracks(6);
    tracks[5] = track("spotify:episode:e0");
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    assert!(queue.start_radio(&tracks, &mut transport).is_some());
    assert!(queue.jump(5, &mut transport).is_some());

    assert_eq!(
        queue.drift_wants(),
        None,
        "an episode is not continued from"
    );

    assert!(queue.jump(4, &mut transport).is_some());
    assert_eq!(wanted(&mut queue).0, 1, "the refused ask spent a token");
}

#[test]
fn an_empty_radio_is_not_marked() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    assert_eq!(queue.start_radio(&[], &mut transport), None);

    queue.append(radio_tracks(3));
    assert!(queue.jump(0, &mut transport).is_some());
    assert_eq!(queue.drift_wants(), None, "an empty start marked a radio");
}

#[test]
fn a_start_clears_the_mark() {
    let (mut queue, mut transport) = tuned(6, 2);
    let tracks = radio_tracks(6);

    assert!(queue.start(&tracks, 0, None, &mut transport).is_some());
    assert!(queue.jump(2, &mut transport).is_some());

    assert_eq!(queue.drift_wants(), None);
}

#[test]
fn a_shuffled_start_clears_the_mark() {
    let (mut queue, mut transport) = tuned(6, 2);
    let tracks = radio_tracks(6);

    assert!(
        queue
            .start_with_shuffle(&tracks, 0, None, &mut transport)
            .is_some()
    );
    assert!(queue.jump(2, &mut transport).is_some());

    assert_eq!(queue.drift_wants(), None);
}

#[test]
fn a_hand_over_clears_the_mark() {
    let (mut queue, mut transport) = tuned(6, 2);
    let landing = landing(
        &["spotify:track:a", "spotify:track:b", "spotify:track:c"],
        0,
    );

    assert!(queue.adopt(&landing, &mut transport));

    assert_eq!(queue.drift_wants(), None);
}

#[test]
fn a_phone_window_clears_the_mark() {
    let (mut queue, _transport) = tuned(10, 5);

    assert!(
        queue
            .replace_window(&[], &["spotify:track:x".to_owned()], "rev".to_owned())
            .is_ok()
    );

    assert_eq!(queue.drift_wants(), None);
}

#[test]
fn an_ask_in_flight_blocks_a_second_one() {
    let (mut queue, _transport) = tuned(6, 2);

    assert_eq!(wanted(&mut queue).0, 1);
    assert_eq!(queue.drift_wants(), None, "an ask is already out");
}

#[test]
fn an_answer_to_a_token_the_queue_never_issued_is_dropped() {
    let (mut queue, _transport) = tuned(6, 2);
    assert_eq!(wanted(&mut queue).0, 1);
    let before = order(&queue);

    queue.land_drift(0, &fresh(1));

    assert_eq!(order(&queue), before);
}

#[test]
fn an_answer_from_another_radios_token_is_dropped() {
    let (mut queue, mut transport) = tuned(6, 2);
    assert_eq!(wanted(&mut queue).0, 1);

    let tracks = radio_tracks(6);
    assert!(queue.start_radio(&tracks, &mut transport).is_some());
    assert!(queue.jump(2, &mut transport).is_some());
    assert_eq!(wanted(&mut queue).0, 2);
    let before = order(&queue);

    queue.land_drift(1, &fresh(1));

    assert_eq!(order(&queue), before, "the old radio's answer landed");
}

#[test]
fn a_failed_transport_lets_a_stranded_ask_go_again() {
    for update in [
        TransportUpdate::Failed(Failure::Load),
        TransportUpdate::Reconnecting,
    ] {
        let (mut queue, mut transport) = tuned(6, 2);
        assert_eq!(wanted(&mut queue).0, 1);
        let before = (queue.index, order(&queue));

        assert_eq!(feed(&mut queue, &mut transport, update.clone()), None);

        assert_eq!(
            (queue.index, order(&queue)),
            before,
            "{update:?} moved the queue"
        );
        assert_eq!(wanted(&mut queue).0, 2, "{update:?} stranded the ask");
    }
}

#[test]
fn a_late_answer_to_an_ask_a_rearm_cancelled_is_dropped() {
    let (mut queue, mut transport) = tuned(6, 2);
    assert_eq!(wanted(&mut queue).0, 1);
    feed(
        &mut queue,
        &mut transport,
        TransportUpdate::Failed(Failure::Load),
    );
    assert_eq!(wanted(&mut queue).0, 2);
    let before = order(&queue);

    queue.land_drift(1, &fresh(1));

    assert_eq!(order(&queue), before);
}

#[test]
fn a_failed_transport_leaves_a_ready_drift_ready() {
    let (mut queue, mut transport) = tuned(6, 2);

    feed(
        &mut queue,
        &mut transport,
        TransportUpdate::Failed(Failure::Load),
    );

    assert_eq!(wanted(&mut queue).0, 1, "a failure spent a token");
}

#[test]
fn a_failed_transport_never_revives_a_dry_radio() {
    let (mut queue, mut transport) = tuned(6, 2);
    assert_eq!(wanted(&mut queue).0, 1);
    queue.land_drift(1, &[]);

    for update in [
        TransportUpdate::Failed(Failure::Load),
        TransportUpdate::Reconnecting,
    ] {
        feed(&mut queue, &mut transport, update);
        assert_eq!(queue.drift_wants(), None, "a dry radio came back to life");
    }
}

#[test]
fn a_failed_transport_leaves_a_plain_list_alone() {
    let tracks = radio_tracks(6);
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    assert!(queue.start(&tracks, 0, None, &mut transport).is_some());
    assert!(queue.jump(4, &mut transport).is_some());

    feed(
        &mut queue,
        &mut transport,
        TransportUpdate::Failed(Failure::Load),
    );

    assert_eq!(queue.drift_wants(), None);
}

#[test]
fn fresh_rows_are_appended_after_the_last_row_in_the_answers_order() {
    let (mut queue, _transport) = tuned(8, 5);
    assert_eq!(wanted(&mut queue).0, 1);

    queue.land_drift(1, &fresh(2));

    let mut expected = uris_of(&radio_tracks(8));
    expected.extend(uris_of(&fresh(2)));
    assert_eq!(order(&queue), expected);
}

#[test]
fn rows_the_queue_already_holds_are_dropped() {
    let (mut queue, _transport) = tuned(8, 5);
    assert_eq!(wanted(&mut queue).0, 1);
    let before = order(&queue);

    queue.land_drift(
        1,
        &[radio_track("t0"), radio_track("t5"), radio_track("t6")],
    );

    assert_eq!(order(&queue), before, "a row already held landed twice");
}

#[test]
fn a_repeat_inside_the_answer_is_dropped() {
    let (mut queue, _transport) = tuned(8, 5);
    assert_eq!(wanted(&mut queue).0, 1);

    queue.land_drift(1, &[radio_track("f0"), radio_track("f0")]);

    let mut expected = uris_of(&radio_tracks(8));
    expected.push("spotify:track:f0".to_owned());
    assert_eq!(order(&queue), expected);
}

#[test]
fn a_row_the_user_removed_can_come_back() {
    let (mut queue, _transport) = tuned(8, 5);
    let removed = queue.entries_at(&[6]);
    assert!(queue.remove_rows(&[6], &removed).is_some());
    assert_eq!(wanted(&mut queue).0, 1);

    queue.land_drift(1, &[radio_track("t6")]);

    assert_eq!(
        order(&queue).last().map(String::as_str),
        Some("spotify:track:t6"),
        "dedupe only knows the rows still in the queue"
    );
}

#[test]
fn an_answer_of_known_rows_leaves_the_radio_dry() {
    let (mut queue, _transport) = tuned(8, 5);
    assert_eq!(wanted(&mut queue).0, 1);
    let before = order(&queue);

    queue.land_drift(1, &[radio_track("t0")]);

    assert_eq!(order(&queue), before);
    assert_eq!(queue.drift_wants(), None, "a dry radio asked again");
}

#[test]
fn an_empty_answer_leaves_the_radio_dry() {
    let (mut queue, _transport) = tuned(8, 5);
    assert_eq!(wanted(&mut queue).0, 1);
    let before = order(&queue);

    queue.land_drift(1, &[]);

    assert_eq!(order(&queue), before);
    assert_eq!(queue.drift_wants(), None);
}

#[test]
fn landed_rows_leave_the_current_row_where_it_was() {
    let (mut queue, _transport) = tuned(8, 5);
    assert_eq!(wanted(&mut queue).0, 1);
    let current = queue.current().cloned();

    queue.land_drift(1, &fresh(3));

    assert_eq!(queue.current().cloned(), current);
    assert_eq!(queue.index, Some(5));
}

#[test]
fn with_shuffle_on_the_rows_land_at_the_tail_and_turning_it_off_still_restores_the_list() {
    let (mut queue, _transport) = tuned(10, 5);
    queue.toggle_shuffle();
    assert!(queue.shuffle());
    assert_eq!(wanted(&mut queue).0, 1);

    queue.land_drift(1, &fresh(2));
    queue.toggle_shuffle();

    let mut expected = uris_of(&radio_tracks(10));
    expected.extend(uris_of(&fresh(2)));
    assert_eq!(order(&queue), expected);
    assert_eq!(
        queue.current().map(|entry| entry.uri.as_str()),
        Some("spotify:track:t5")
    );
}

#[test]
fn a_phone_queued_block_keeps_its_place() {
    let (mut queue, _transport) = tuned(8, 5);
    assert!(queue.add_next(vec![radio_track("p0")]).is_some());
    assert_eq!(wanted(&mut queue).0, 1);

    queue.land_drift(1, &fresh(1));

    let mut expected = uris_of(&radio_tracks(8));
    expected.insert(6, "spotify:track:p0".to_owned());
    expected.extend(uris_of(&fresh(1)));
    assert_eq!(order(&queue), expected);
    assert_eq!(queue.queued, 1, "the landing moved the queued block");
}

#[test]
fn an_answer_applied_as_an_update_is_folded_in() {
    let (mut queue, mut transport) = tuned(8, 5);
    assert_eq!(wanted(&mut queue).0, 1);

    let update = Update::RadioMore {
        ask: 1,
        rows: fresh(1),
    };

    assert_eq!(queue.apply(&update, &mut transport), None);
    assert_eq!(
        order(&queue).last().map(String::as_str),
        Some("spotify:track:f0")
    );
}

#[test]
fn a_repeat_one_radio_asks_once_and_stops_when_the_rows_land() {
    let (mut queue, _transport) = tuned(5, 0);
    while queue.repeat() != Repeat::One {
        queue.cycle_repeat();
    }

    assert_eq!(wanted(&mut queue).0, 1);

    queue.land_drift(1, &fresh(50));

    assert_eq!(
        queue.drift_wants(),
        None,
        "the queue grew past the threshold"
    );
    assert_eq!(queue.repeat(), Repeat::One, "the landing changed the mode");
}
