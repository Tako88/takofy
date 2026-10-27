use super::super::*;
use super::test_support::*;

use crate::reorder;

#[test]
fn moving_one_row_down_puts_it_before_the_boundary_entry() {
    let mut queue = started(5, 0);
    assert_eq!(queue.move_rows(&[1], 3), Some(2..3));
    assert_eq!(order(&queue), uris(&["t0", "t2", "t1", "t3", "t4"]));
}

#[test]
fn moving_one_row_up_puts_it_before_the_boundary_entry() {
    let mut queue = started(5, 0);
    assert_eq!(queue.move_rows(&[3], 2), Some(2..3));
    assert_eq!(order(&queue), uris(&["t0", "t1", "t3", "t2", "t4"]));
}

#[test]
fn moving_a_block_leaves_the_order_a_playlist_block_leaves() {
    for len in 1..=6 {
        for rows in subsets(len) {
            for boundary in 0..=len {
                let mut queue = armed(len);
                let before = order(&queue);
                let found = reorder::block(&before, &rows, boundary);
                let moved = queue.move_rows(&rows, boundary);
                match found {
                    None => {
                        assert_eq!(moved, None, "{len} {rows:?} {boundary}");
                        assert_eq!(order(&queue), before, "{len} {rows:?} {boundary}");
                    }
                    Some(found) => {
                        assert_eq!(
                            moved,
                            Some(found.landed..found.landed + rows.len()),
                            "{len} {rows:?} {boundary}"
                        );
                        assert_eq!(order(&queue), found.order, "{len} {rows:?} {boundary}");
                    }
                }
            }
        }
    }
}

#[test]
fn moving_a_block_past_the_current_entry_agrees_where_the_queue_may_edit() {
    // A started queue clamps a boundary and drops the rows at or before the
    // current entry, so the sweep is its own editable region.
    for len in 1..=6 {
        for rows in subsets(len).into_iter().filter(|rows| rows[0] >= 1) {
            for boundary in 1..=len {
                let mut queue = started(len, 0);
                let before = order(&queue);
                let found = reorder::block(&before, &rows, boundary);
                let moved = queue.move_rows(&rows, boundary);
                match found {
                    None => assert_eq!(moved, None, "{len} {rows:?} {boundary}"),
                    Some(found) => {
                        assert_eq!(
                            moved,
                            Some(found.landed..found.landed + rows.len()),
                            "{len} {rows:?} {boundary}"
                        );
                        assert_eq!(order(&queue), found.order, "{len} {rows:?} {boundary}");
                    }
                }
            }
        }
    }
}

#[test]
fn moving_a_block_keeps_its_order() {
    let mut queue = started(6, 0);
    assert_eq!(queue.move_rows(&[2, 1], 5), Some(3..5));
    assert_eq!(order(&queue), uris(&["t0", "t3", "t4", "t1", "t2", "t5"]));
}

#[test]
fn a_gapped_selection_gathers_at_the_boundary() {
    let mut queue = started(6, 0);
    assert_eq!(queue.move_rows(&[3, 1], 5), Some(3..5));
    assert_eq!(order(&queue), uris(&["t0", "t2", "t4", "t1", "t3", "t5"]));
}

#[test]
fn a_contiguous_block_dropped_inside_or_beside_itself_changes_nothing() {
    let mut queue = started(6, 0);
    let before = order(&queue);

    for boundary in [2, 3, 4, 5] {
        assert_eq!(queue.move_rows(&[2, 3, 4], boundary), None, "{boundary}");
    }

    assert_eq!(order(&queue), before);
}

#[test]
fn a_move_selects_the_landed_block() {
    let mut queue = started(6, 0);
    queue.press(1, Press::Replace);
    queue.press(3, Press::Toggle);
    assert_eq!(picked(&queue), vec![1, 3]);

    assert_eq!(queue.move_rows(&[1, 3], 5), Some(3..5));

    assert_eq!(picked(&queue), vec![3, 4]);
}

#[test]
fn rows_at_or_before_the_current_entry_cannot_move() {
    let mut queue = started(5, 2);
    let before = order(&queue);

    assert_eq!(queue.move_rows(&[0, 1, 2], 4), None);
    assert_eq!(queue.move_rows(&[1], 4), None);

    assert_eq!(order(&queue), before);
}

#[test]
fn a_boundary_before_the_first_upcoming_row_lands_there() {
    let mut queue = started(5, 1);
    assert_eq!(queue.move_rows(&[3], 0), Some(2..3));
    assert_eq!(order(&queue), uris(&["t0", "t1", "t3", "t2", "t4"]));
}

#[test]
fn a_boundary_past_the_end_appends() {
    let mut queue = started(5, 0);
    assert_eq!(queue.move_rows(&[1], 99), Some(4..5));
    assert_eq!(order(&queue), uris(&["t0", "t2", "t3", "t4", "t1"]));
}

#[test]
fn an_edit_never_touches_the_index_the_guard_or_the_skips() {
    let mut queue = started(6, 1);
    let mut transport = Transport::default();
    clear_guard(&mut queue, &mut transport);
    // A failed load leaves the index one further on, the guard armed for the
    // new load and one skip counted.
    assert_eq!(
        feed(&mut queue, &mut transport, TransportUpdate::Unavailable),
        Some(play("t2"))
    );
    assert_eq!(
        (queue.index, queue.awaiting_start, queue.skips),
        (Some(2), true, 1)
    );

    assert!(queue.move_rows(&[3], 5).is_some());
    assert!(queue.insert_at(4, vec![track("x")]).is_some());
    assert!(queue.step_rows(&[3], Step::Down).is_some());

    assert_eq!(
        (queue.index, queue.awaiting_start, queue.skips),
        (Some(2), true, 1),
        "no edit may move the index, disarm a load or reset a skip"
    );
}

#[test]
fn duplicates_keep_their_own_identity() {
    let entries = vec![
        track_by("same", "A"),
        track_by("same", "B"),
        track_by("same", "C"),
    ];
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&entries, 0, None, &mut transport);

    assert_eq!(queue.move_rows(&[2], 1), Some(1..2));
    assert_eq!(artists(&queue), uris(&["A", "C", "B"]));
}

#[test]
fn insert_in_the_middle() {
    let mut queue = started(3, 0);
    assert_eq!(queue.insert_at(2, vec![track("x")]), Some(2..3));
    assert_eq!(order(&queue), uris(&["t0", "t1", "x", "t2"]));
}

#[test]
fn insert_at_the_front_of_the_upcoming_rows() {
    let mut queue = started(4, 1);
    assert_eq!(queue.insert_at(0, vec![track("x")]), Some(2..3));
    assert_eq!(order(&queue), uris(&["t0", "t1", "x", "t2", "t3"]));
}

#[test]
fn insert_at_the_end() {
    let mut queue = started(3, 0);
    assert_eq!(queue.insert_at(99, vec![track("x")]), Some(3..4));
    assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "x"]));
}

#[test]
fn an_insert_shifts_selected_rows_after_it() {
    let mut queue = started(5, 0);
    queue.press(2, Press::Replace);
    queue.press(4, Press::Toggle);

    assert_eq!(queue.insert_at(3, vec![track("x"), track("y")]), Some(3..5));

    assert_eq!(picked(&queue), vec![2, 6]);
}

#[test]
fn an_empty_queue_takes_nothing_to_play() {
    let mut queue = Queue::default();

    assert_eq!(
        queue.toggle(&mut Transport::default()),
        None,
        "an empty queue has no first row to start"
    );
}

#[test]
fn an_insert_lands_in_an_empty_queue_and_leaves_it_armed() {
    let mut queue = Queue::default();

    assert_eq!(queue.insert_at(0, vec![track("x")]), Some(0..1));

    assert_eq!(order(&queue), uris(&["x"]));
    assert_eq!(queue.first_upcoming(), 0, "nothing is current yet");
    assert_eq!(queue.upcoming().len(), 1);
    assert!(queue.armed(), "the row waits for a first play");
}

#[test]
fn the_first_play_of_an_armed_queue_starts_its_first_row() {
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    let armed = queue.append(vec![track("x"), track("y")]);
    assert_eq!(armed, Some(0..2));

    assert!(!queue.can_previous(), "nothing has played yet");
    assert!(!queue.can_next(), "nothing is current to step from");
    assert_eq!(queue.next(&mut transport), None);
    assert_eq!(queue.previous(0, &mut transport), None);

    assert_eq!(queue.toggle(&mut transport), Some(play("x")));

    assert_eq!(queue.first_upcoming(), 1);
    assert!(queue.can_next());
    assert!(!queue.armed(), "it is playing the row that was waiting");
}

#[test]
fn an_armed_queue_ignores_the_events_of_a_track_it_never_started() {
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(queue.append(vec![track("x"), track("y")]), Some(0..2));

    for update in [TransportUpdate::Ended, TransportUpdate::Unavailable] {
        assert_eq!(
            feed(&mut queue, &mut transport, update.clone()),
            None,
            "nothing is playing, so {update:?} is nobody's"
        );
    }
    assert!(
        queue.armed(),
        "an armed queue must not walk itself on a stray event"
    );
    assert_eq!(order(&queue), uris(&["x", "y"]));
}

#[test]
fn a_jump_starts_an_armed_row_and_leaves_the_ones_before_it() {
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.append(vec![track("x"), track("y"), track("z")]),
        Some(0..3)
    );

    assert_eq!(queue.jump(2, &mut transport), Some(play("z")));

    assert_eq!(queue.first_upcoming(), 3);
    assert_eq!(order(&queue), uris(&["x", "y", "z"]));
    assert!(!queue.armed());
}

#[test]
fn an_armed_queue_edits_every_row() {
    let mut queue = Queue::default();
    let armed = queue.append(vec![track("x"), track("y"), track("z")]);
    assert_eq!(armed, Some(0..3));

    assert_eq!(queue.lift(0), Some(vec![0]), "the first row is editable");
    queue.press(0, Press::Replace);
    assert_eq!(picked(&queue), vec![0]);
    assert_eq!(queue.step_rows(&[0], Step::Down), Some(vec![(0, 1)]));
    assert_eq!(order(&queue), uris(&["y", "x", "z"]));
    assert_eq!(picked(&queue), vec![1], "the pick follows the row");
}

#[test]
fn an_empty_queue_moves_nothing() {
    let mut queue = Queue::default();
    assert_eq!(queue.first_upcoming(), 0);
    assert_eq!(queue.move_rows(&[0], 0), None);
    assert_eq!(queue.step_rows(&[0], Step::Up), None);
    assert_eq!(queue.lift(0), None);
}

#[test]
fn no_entries_insert_nothing() {
    let mut queue = started(3, 0);
    let before = order(&queue);

    assert_eq!(queue.insert_at(1, Vec::new()), None);

    assert_eq!(order(&queue), before);
}

#[test]
fn append_adds_after_the_last_entry_unpicked() {
    let mut queue = started(3, 0);

    assert_eq!(queue.append(vec![track("x"), track("y")]), Some(3..5));

    assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "x", "y"]));
    assert!(picked(&queue).is_empty(), "an add must not pick its rows");
}

#[test]
fn append_while_the_last_entry_plays() {
    let mut queue = started(3, 2);

    assert_eq!(queue.append(vec![track("x")]), Some(3..4));

    assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "x"]));
    assert!(queue.can_next(), "the added entry is reachable");
    assert!(picked(&queue).is_empty());
}

#[test]
fn append_keeps_an_existing_pick() {
    let mut queue = started(5, 0);
    queue.press(2, Press::Replace);
    queue.press(4, Press::Toggle);

    assert_eq!(queue.append(vec![track("x")]), Some(5..6));

    assert_eq!(picked(&queue), vec![2, 4]);
}

#[test]
fn an_empty_queue_takes_an_append_at_its_first_row() {
    let mut queue = Queue::default();

    assert_eq!(queue.append(vec![track("x")]), Some(0..1));

    assert_eq!(order(&queue), uris(&["x"]));
    assert_eq!(queue.first_upcoming(), 0);
}

#[test]
fn appending_nothing_changes_nothing() {
    let mut queue = started(3, 0);
    let before = order(&queue);

    assert_eq!(queue.append(Vec::new()), None);

    assert_eq!(order(&queue), before);
}
