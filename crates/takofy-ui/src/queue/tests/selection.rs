use super::super::*;
use super::test_support::*;

#[test]
fn step_moves_one_row_up_and_down() {
    let mut queue = started(5, 0);
    assert_eq!(queue.step_rows(&[3], Step::Up), Some(vec![(3, 2)]));
    assert_eq!(order(&queue), uris(&["t0", "t1", "t3", "t2", "t4"]));
    assert_eq!(queue.step_rows(&[2], Step::Down), Some(vec![(2, 3)]));
    assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "t3", "t4"]));
}

#[test]
fn step_stops_at_the_first_upcoming_row_and_the_last_entry() {
    let mut queue = started(4, 1);

    assert_eq!(queue.step_rows(&[2], Step::Up), None);
    assert_eq!(queue.step_rows(&[3], Step::Down), None);

    assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "t3"]));
}

#[test]
fn a_block_steps_as_a_unit() {
    let mut queue = started(6, 0);

    assert_eq!(
        queue.step_rows(&[2, 3, 4], Step::Up),
        Some(vec![(2, 1), (3, 2), (4, 3)])
    );
    assert_eq!(order(&queue), uris(&["t0", "t2", "t3", "t4", "t1", "t5"]));
    assert_eq!(picked(&queue), vec![1, 2, 3]);

    assert_eq!(
        queue.step_rows(&[1, 2, 3], Step::Down),
        Some(vec![(1, 2), (2, 3), (3, 4)])
    );
    assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "t3", "t4", "t5"]));
}

#[test]
fn a_blocked_row_blocks_the_ones_behind_it() {
    let mut queue = started(5, 0);
    let before = order(&queue);

    assert_eq!(queue.step_rows(&[1, 2], Step::Up), None);
    assert_eq!(order(&queue), before);

    assert_eq!(queue.step_rows(&[3, 4], Step::Down), None);
    assert_eq!(order(&queue), before);
}

#[test]
fn a_pick_blocked_at_the_top_keeps_its_pick() {
    let mut queue = started(5, 0);
    queue.press(1, Press::Replace);
    queue.press(3, Press::Toggle);

    assert_eq!(queue.step_rows(&[1, 3], Step::Up), Some(vec![(3, 2)]));

    assert_eq!(picked(&queue), vec![1, 2]);
    assert_eq!(queue.acting_rows(None), vec![1, 2]);
    assert_eq!(order(&queue), uris(&["t0", "t1", "t3", "t2", "t4"]));
}

#[test]
fn a_pick_blocked_at_the_bottom_keeps_its_pick() {
    let mut queue = started(5, 0);
    queue.press(2, Press::Replace);
    queue.press(4, Press::Toggle);

    assert_eq!(queue.step_rows(&[2, 4], Step::Down), Some(vec![(2, 3)]));

    assert_eq!(picked(&queue), vec![3, 4]);
    assert_eq!(queue.acting_rows(None), vec![3, 4]);
    assert_eq!(order(&queue), uris(&["t0", "t1", "t3", "t2", "t4"]));
}

#[test]
fn step_reports_old_and_new_positions() {
    let mut queue = started(5, 0);

    assert_eq!(
        queue.step_rows(&[3, 1], Step::Down),
        Some(vec![(1, 2), (3, 4)])
    );

    assert_eq!(order(&queue), uris(&["t0", "t2", "t1", "t4", "t3"]));
}

#[test]
fn press_outside_the_region_is_ignored() {
    let mut queue = started(4, 1);

    queue.press(0, Press::Replace);
    queue.press(1, Press::Replace);
    queue.press(4, Press::Replace);

    assert!(picked(&queue).is_empty());
}

#[test]
fn lift_keeps_a_selection_it_is_in() {
    let mut queue = started(8, 0);
    queue.press(2, Press::Replace);
    queue.press(3, Press::Toggle);
    queue.press(6, Press::Toggle);

    assert_eq!(queue.lift(3), Some(vec![2, 3, 6]));
    assert_eq!(picked(&queue), vec![2, 3, 6]);
}

#[test]
fn lift_selects_an_unpicked_row() {
    let mut queue = started(8, 0);
    queue.press(2, Press::Replace);
    queue.press(3, Press::Toggle);

    assert_eq!(queue.lift(1), Some(vec![1]));
}

#[test]
fn lifting_outside_the_region_carries_nothing() {
    let mut queue = started(3, 1);

    assert_eq!(queue.lift(1), None);
    assert_eq!(queue.lift(3), None);
}

#[test]
fn start_clears_the_selection() {
    let mut queue = started(4, 0);
    queue.press(2, Press::Replace);
    queue.refocus_at(3);
    assert_eq!(picked(&queue), vec![2]);
    assert_eq!(queue.refocus(), Some(3));

    let tracks = tracks(3);
    let mut transport = Transport::default();
    queue.start(&tracks, 0, None, &mut transport);

    assert!(picked(&queue).is_empty());
    assert_eq!(queue.refocus(), None);
}

#[test]
fn advancing_leaves_the_selection_and_the_refocus_alone() {
    let mut queue = started(4, 0);
    let mut transport = Transport::default();
    queue.press(2, Press::Replace);
    queue.refocus_at(2);

    assert_eq!(queue.next(&mut transport), Some(play("t1")));

    assert!(queue.selection.contains(2));
    assert_eq!(queue.refocus(), Some(2));
}

#[test]
fn a_selected_row_that_became_current_is_not_selected() {
    let mut queue = started(4, 0);
    let mut transport = Transport::default();
    queue.press(1, Press::Replace);
    queue.press(2, Press::Toggle);
    assert_eq!(picked(&queue), vec![1, 2]);

    clear_guard(&mut queue, &mut transport);
    assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));

    assert!(!queue.selected(1), "the current entry is never picked");
    assert!(queue.selected(2));
    assert_eq!(queue.acting_rows(None), vec![2]);
}

#[test]
fn acting_rows_prefers_the_selection_then_the_focused_row() {
    let mut queue = started(6, 1);

    assert_eq!(queue.acting_rows(Some(4)), vec![4]);
    assert!(queue.acting_rows(Some(1)).is_empty());
    assert!(queue.acting_rows(None).is_empty());

    queue.press(2, Press::Replace);
    queue.press(3, Press::Toggle);
    assert_eq!(queue.acting_rows(Some(5)), vec![2, 3]);
}

#[test]
fn refocus_lives_until_settled() {
    let mut queue = started(4, 0);

    assert_eq!(queue.refocus(), None);
    queue.refocus_at(3);
    assert_eq!(queue.refocus(), Some(3));
    queue.settle_refocus();
    assert_eq!(queue.refocus(), None);
}

#[test]
fn current_is_the_entry_at_index_and_none_when_armed_or_empty() {
    assert_eq!(Queue::default().current(), None);
    assert_eq!(armed(3).current(), None, "an armed queue has no current");
    let queue = started(4, 2);
    assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("t2"));
}

#[test]
fn act_on_inside_the_pick_returns_it_ascending_and_keeps_it() {
    let mut queue = started(8, 0);
    queue.press(5, Press::Replace);
    queue.press(2, Press::Toggle);

    assert_eq!(queue.act_on(5), vec![2, 5]);
    assert_eq!(picked(&queue), vec![2, 5], "act_on collapsed the pick");
}

#[test]
fn act_on_outside_replaces_the_pick() {
    let mut queue = started(8, 0);
    queue.press(5, Press::Replace);
    queue.press(2, Press::Toggle);

    assert_eq!(queue.act_on(4), vec![4]);
    assert_eq!(picked(&queue), vec![4]);
}

#[test]
fn act_on_the_current_entry_is_empty() {
    let mut queue = started(8, 3);
    queue.press(5, Press::Replace);

    assert!(queue.act_on(3).is_empty());
    assert!(queue.act_on(0).is_empty());
    assert_eq!(picked(&queue), vec![5], "a refused row touched the pick");
}

#[test]
fn act_on_drops_a_picked_row_the_queue_has_passed() {
    let mut queue = started(6, 1);
    let mut transport = Transport::default();
    queue.press(2, Press::Replace);
    queue.press(3, Press::Toggle);
    clear_guard(&mut queue, &mut transport);
    assert_eq!(end(&mut queue, &mut transport), Some(play("t2")));

    assert_eq!(
        queue.act_on(3),
        vec![3],
        "the row that became current went into the cut"
    );
}

#[test]
fn act_on_a_pick_the_queue_has_passed_is_empty() {
    let mut queue = started(6, 1);
    let mut transport = Transport::default();
    queue.press(2, Press::Replace);
    clear_guard(&mut queue, &mut transport);
    assert_eq!(end(&mut queue, &mut transport), Some(play("t2")));

    assert!(
        queue.act_on(2).is_empty(),
        "a pick the queue has passed was offered"
    );
}

#[test]
fn remove_rows_takes_the_rows_and_keeps_the_rest_in_order() {
    let mut queue = started(6, 0);
    let expect = queue.entries_at(&[1, 3, 4]);

    assert_eq!(queue.remove_rows(&[1, 3, 4], &expect), Some(3));
    assert_eq!(order(&queue), uris(&["t0", "t2", "t5"]));
}

#[test]
fn a_queue_holding_the_same_entry_twice_removes_the_one_picked() {
    let tracks = vec![track("t0"), track("t1"), track("t1"), track("t2")];
    let mut queue = Queue::default();
    queue.start(&tracks, 0, None, &mut Transport::default());
    queue.press(2, Press::Replace);

    let rows = queue.act_on(2);
    let expect = queue.entries_at(&rows);

    assert_eq!(rows, vec![2], "the pick did not name the second copy");
    assert_eq!(
        queue.remove_rows(&rows, &expect),
        Some(1),
        "the identical neighbour was taken too"
    );
    assert_eq!(order(&queue), uris(&["t0", "t1", "t2"]));
}

#[test]
fn remove_rows_never_moves_index_awaiting_start_or_skips() {
    for len in 1..=6 {
        for index in 0..len {
            for rows in subsets(len).into_iter().filter(|rows| rows[0] > index) {
                let mut queue = started(len, index);
                let expect = queue.entries_at(&rows);
                let before = (queue.index, queue.awaiting_start, queue.skips);

                assert_eq!(
                    queue.remove_rows(&rows, &expect),
                    Some(rows.len()),
                    "len {len} at {index}, rows {rows:?}"
                );
                assert_eq!(
                    (queue.index, queue.awaiting_start, queue.skips),
                    before,
                    "len {len} at {index}, rows {rows:?}"
                );
            }
        }
    }
}

#[test]
fn an_armed_queue_stays_armed_or_empties() {
    let mut queue = armed(4);
    let expect = queue.entries_at(&[1]);
    assert_eq!(queue.remove_rows(&[1], &expect), Some(1));
    assert!(queue.armed());
    assert_eq!(order(&queue), uris(&["t0", "t2", "t3"]));

    let expect = queue.entries_at(&[0, 1, 2]);
    assert_eq!(queue.remove_rows(&[0, 1, 2], &expect), Some(3));
    assert!(!queue.armed(), "an emptied queue is not armed");
    assert!(order(&queue).is_empty());
}

#[test]
fn emptying_the_upcoming_region_leaves_the_current_entry_and_no_next() {
    let mut queue = started(4, 1);
    let expect = queue.entries_at(&[2, 3]);

    assert_eq!(queue.remove_rows(&[2, 3], &expect), Some(2));
    assert_eq!(order(&queue), uris(&["t0", "t1"]));
    assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("t1"));
    assert!(!queue.can_next());
    assert_eq!(queue.refocus(), None);
}

#[test]
fn a_non_upcoming_row_refuses_the_whole_removal() {
    let mut queue = started(6, 2);
    let expect = queue.entries_at(&[2, 3]);
    assert_eq!(queue.remove_rows(&[2, 3], &expect), None, "the current row");

    let expect = queue.entries_at(&[1, 3]);
    assert_eq!(queue.remove_rows(&[1, 3], &expect), None, "a row before it");

    let expect = queue.entries_at(&[3, 9]);
    assert_eq!(
        queue.remove_rows(&[3, 9], &expect),
        None,
        "a row past the end"
    );

    assert_eq!(
        order(&queue),
        uris(&["t0", "t1", "t2", "t3", "t4", "t5"]),
        "a refused removal changed the list"
    );
}

#[test]
fn a_stale_entry_refuses_it() {
    let mut queue = started(6, 0);
    let mut other = track("t1");
    other.artists = "another band".to_owned();

    assert_eq!(
        queue.remove_rows(&[1, 2], &[other, track("t2")]),
        None,
        "identical neighbours would pass a comparison of uris alone"
    );
    assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "t3", "t4", "t5"]));
    assert!(picked(&queue).is_empty());
}

#[test]
fn the_selection_shifts_and_is_empty_after_a_whole_pick_goes() {
    let mut queue = started(8, 0);
    queue.press(2, Press::Replace);
    queue.press(5, Press::Toggle);
    queue.press(6, Press::Toggle);

    let expect = queue.entries_at(&[2, 3]);
    assert_eq!(queue.remove_rows(&[2, 3], &expect), Some(2));
    assert_eq!(picked(&queue), vec![3, 4]);

    let rest = queue.acting_rows(None);
    let expect = queue.entries_at(&rest);
    assert_eq!(queue.remove_rows(&rest, &expect), Some(2));
    assert!(picked(&queue).is_empty());
}

#[test]
fn refocus_lands_on_the_row_now_at_the_first_removed_index_or_is_none() {
    let mut queue = started(6, 0);
    let expect = queue.entries_at(&[2, 3]);
    queue.remove_rows(&[2, 3], &expect);

    assert_eq!(queue.refocus(), Some(2));
    assert_eq!(queue.tracks[2].uri, "t4");

    let mut queue = started(4, 0);
    let expect = queue.entries_at(&[3]);
    queue.remove_rows(&[3], &expect);
    assert_eq!(queue.refocus(), None, "the last row left nothing in place");
}

#[test]
fn refocus_is_none_when_nothing_upcoming_remains() {
    let mut queue = started(4, 1);
    let expect = queue.entries_at(&[2, 3]);
    queue.remove_rows(&[2, 3], &expect);

    assert_eq!(queue.refocus(), None);
    assert_ne!(
        queue.refocus(),
        queue.index,
        "clamped onto the current entry"
    );
}

#[test]
fn a_stale_refocus_is_overwritten() {
    let mut queue = started(6, 0);
    queue.refocus_at(5);
    let expect = queue.entries_at(&[1]);
    queue.remove_rows(&[1], &expect);
    assert_eq!(queue.refocus(), Some(1));

    let mut queue = started(4, 1);
    queue.refocus_at(2);
    let expect = queue.entries_at(&[2, 3]);
    queue.remove_rows(&[2, 3], &expect);
    assert_eq!(queue.refocus(), None);
}

#[test]
fn remove_rows_returns_the_count() {
    let mut queue = started(8, 0);
    let expect = queue.entries_at(&[5, 1, 2]);

    assert_eq!(queue.remove_rows(&[5, 1, 2], &expect), Some(3));
    assert_eq!(order(&queue), uris(&["t0", "t3", "t4", "t6", "t7"]));
    assert_eq!(queue.refocus(), Some(1));
}
