use super::super::*;
use super::test_support::*;

/// A queue of `count` tracks started at `index`, shuffled from `seed`.
fn shuffled_at(count: usize, index: usize, seed: u64) -> Queue {
    let tracks = tracks(count);
    let mut queue = Queue::seeded(seed);
    let mut transport = Transport::default();
    queue.start(&tracks, index, None, &mut transport);
    queue.toggle_shuffle();
    queue
}

/// Whether the queue holds rows `t0..count`, in some order.
fn holds_exactly(queue: &Queue, count: usize) -> bool {
    let mut held = order(queue);
    held.sort();
    let mut expected: Vec<String> = (0..count).map(|row| format!("t{row}")).collect();
    expected.sort();
    held == expected
}

#[test]
fn shuffle_on_permutes_only_the_coming_rows() {
    let mut queue = started(5, 2);
    let before = order(&queue);
    queue.toggle_shuffle();

    assert_eq!(
        &order(&queue)[..2],
        &uris(&["t0", "t1"]),
        "the rows before the current one are history"
    );
    assert_eq!(order(&queue)[2], "t2", "the current entry keeps its row");
    assert_ne!(order(&queue), before);
    assert_eq!(queue.index, Some(2));
    assert!(holds_exactly(&queue, 5));
}

#[test]
fn shuffle_on_an_armed_queue_permutes_every_row_and_stays_armed() {
    let mut queue = Queue::seeded(7);
    queue.append(tracks(5));
    let before = order(&queue);

    queue.toggle_shuffle();

    assert!(queue.armed(), "a permutation is not a play");
    assert_eq!(queue.first_upcoming(), 0);
    assert_ne!(order(&queue), before);
    assert!(holds_exactly(&queue, 5));
}

#[test]
fn shuffle_on_and_off_never_alter_awaiting_start_or_skips() {
    let mut queue = started(6, 1);
    let mut transport = Transport::default();
    clear_guard(&mut queue, &mut transport);
    assert_eq!(
        feed(&mut queue, &mut transport, TransportUpdate::Unavailable),
        Some(play("t2"))
    );
    let before = (queue.index, queue.awaiting_start, queue.skips);

    queue.toggle_shuffle();
    assert_eq!((queue.index, queue.awaiting_start, queue.skips), before);
    queue.toggle_shuffle();
    assert_eq!(
        (queue.index, queue.awaiting_start, queue.skips),
        before,
        "the entry is the same one, so the guard is still its load's"
    );
}

#[test]
fn a_stale_ended_after_the_toggle_is_still_ignored() {
    let mut queue = started(5, 0);
    let mut transport = Transport::default();
    queue.toggle_shuffle();

    assert_eq!(
        end(&mut queue, &mut transport),
        None,
        "a toggle is no play, so the load in flight is still in flight"
    );
}

#[test]
fn what_plays_next_is_what_the_pane_lists_first() {
    let mut queue = shuffled_at(6, 1, 4);
    let mut transport = Transport::default();
    let listed: Vec<String> = upcoming(&queue).into_iter().map(|(_, uri)| uri).collect();
    clear_guard(&mut queue, &mut transport);

    assert_eq!(end(&mut queue, &mut transport), Some(play(&listed[0])));
    assert_eq!(queue.index, Some(2));
    let rest: Vec<String> = upcoming(&queue).into_iter().map(|(_, uri)| uri).collect();
    assert_eq!(rest, listed[1..], "the pane still reads in play order");
}

#[test]
fn a_shuffled_run_plays_every_row_once_and_is_not_the_lists_order() {
    for count in 3..=6 {
        for seed in 0..50 {
            let list = tracks(count);
            let mut queue = Queue::seeded(seed);
            let mut transport = Transport::default();
            queue.start(&list, 0, None, &mut transport);
            queue.toggle_shuffle();

            let mut played = Vec::new();
            loop {
                played.push(
                    queue
                        .current()
                        .map(|entry| entry.uri.clone())
                        .expect("a current row"),
                );
                if queue.next(&mut transport).is_none() {
                    break;
                }
            }

            let mut once = played.clone();
            once.sort();
            let mut listed: Vec<String> = list.iter().map(|entry| entry.uri.clone()).collect();
            listed.sort();
            assert_eq!(once, listed, "{count} rows, seed {seed}");
            assert_ne!(
                played, listed,
                "{count} rows, seed {seed}: the run repeated the list's order"
            );
        }
    }
}

#[test]
fn shuffle_on_clears_the_pick_and_the_refocus() {
    let mut queue = started(5, 0);
    queue.press(1, Press::Replace);
    queue.press(3, Press::Toggle);
    queue.refocus_at(3);
    assert_eq!(picked(&queue), vec![1, 3]);

    queue.toggle_shuffle();
    assert!(
        picked(&queue).is_empty(),
        "a pick cannot survive other rows"
    );
    assert_eq!(queue.refocus(), None);

    queue.press(1, Press::Replace);
    queue.toggle_shuffle();
    assert!(
        picked(&queue).is_empty(),
        "and turning it off clears it too"
    );
}

#[test]
fn shuffle_off_restores_the_original_order_and_keeps_the_current_entry() {
    for count in 1..=7 {
        for index in 0..count {
            for seed in 0..20 {
                let mut queue = shuffled_at(count, index, seed);
                assert!(holds_exactly(&queue, count), "{count} {index} {seed}");
                let current = queue.current().map(|entry| entry.uri.clone());
                assert!(current.is_some(), "{count} {index} {seed}");

                queue.toggle_shuffle();

                let expected: Vec<String> = (0..count).map(|row| format!("t{row}")).collect();
                assert_eq!(order(&queue), expected, "{count} {index} {seed}");
                assert_eq!(
                    queue.current().map(|entry| entry.uri.clone()),
                    current,
                    "{count} {index} {seed}: the current entry changed"
                );
                assert_eq!(
                    queue.index,
                    Some(index),
                    "{count} {index} {seed}: the entry is back on its own row"
                );
            }
        }
    }

    let mut empty = Queue::default();
    empty.toggle_shuffle();
    empty.toggle_shuffle();
    assert_eq!(order(&empty), Vec::<String>::new());
    assert!(!empty.armed(), "an empty queue is not armed");
}

#[test]
fn shuffle_off_after_advancing_keeps_the_current_entry() {
    let mut transport = Transport::default();
    for seed in 0..20 {
        for advances in 0..6 {
            let mut queue = shuffled_at(6, 0, seed);
            for _ in 0..advances {
                assert!(queue.next(&mut transport).is_some());
            }
            let current = queue.current().map(|entry| entry.uri.clone());

            queue.toggle_shuffle();

            assert_eq!(
                queue.current().map(|entry| entry.uri.clone()),
                current,
                "seed {seed} advances {advances}"
            );
            assert_eq!(
                order(&queue),
                (0..6).map(|row| format!("t{row}")).collect::<Vec<_>>(),
                "seed {seed} advances {advances}"
            );
        }
    }
}

#[test]
fn shuffle_off_restores_history_too() {
    let mut queue = started(5, 0);
    let mut transport = Transport::default();
    queue.toggle_shuffle();

    let held = order(&queue);
    let t3 = held
        .iter()
        .position(|uri| uri == "t3")
        .expect("t3 is queued");
    let t1 = held
        .iter()
        .position(|uri| uri == "t1")
        .expect("t1 is queued");
    queue.jump(t3, &mut transport);
    queue.jump(t1, &mut transport);
    assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("t1"));

    queue.toggle_shuffle();

    assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "t3", "t4"]));
    assert_eq!(queue.index, Some(1), "the entry is back on its own row");
    assert_eq!(
        order(&queue)[3],
        "t3",
        "a row that already played is ahead of the current one"
    );
    assert_eq!(queue.previous(0, &mut transport), Some(play("t0")));
}

#[test]
fn shuffle_off_with_duplicate_uris_keeps_every_row_and_the_entry() {
    let list = vec![
        track_by("a", "1"),
        track_by("b", "2"),
        track_by("a", "3"),
        track_by("b", "4"),
        track_by("a", "5"),
    ];
    let mut transport = Transport::default();
    let mut expected: Vec<String> = list.iter().map(|entry| entry.artists.clone()).collect();
    expected.sort();
    for seed in 0..20 {
        for index in 0..list.len() {
            let mut queue = Queue::seeded(seed);
            queue.start(&list, index, None, &mut transport);
            let current = queue.current().map(|entry| entry.artists.clone());

            queue.toggle_shuffle();
            queue.toggle_shuffle();

            assert_eq!(
                order(&queue),
                uris(&["a", "b", "a", "b", "a"]),
                "seed {seed} index {index}"
            );
            let mut held = artists(&queue);
            held.sort();
            assert_eq!(
                held, expected,
                "seed {seed} index {index}: a row went missing"
            );
            assert_eq!(
                queue.current().map(|entry| entry.artists.clone()),
                current,
                "seed {seed} index {index}: the current entry changed"
            );
        }
    }
}

#[test]
fn shuffle_off_honours_a_removal_puts_an_append_last_and_discards_a_move() {
    let mut queue = shuffled_at(5, 0, 3);
    let row = order(&queue)
        .iter()
        .position(|uri| uri == "t3")
        .expect("t3 is queued");
    assert_ne!(row, 0, "the current row is never editable");
    assert_eq!(
        queue.remove_rows(&[row], &queue.entries_at(&[row])),
        Some(1)
    );
    assert!(queue.append(vec![track("x")]).is_some());
    let last = order(&queue).len();
    assert!(
        queue.move_rows(&[1], last).is_some(),
        "a move the restore has to discard"
    );

    queue.toggle_shuffle();

    assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "t4", "x"]));
    assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("t0"));
}

#[test]
fn shuffle_off_on_an_armed_queue() {
    let mut queue = Queue::seeded(5);
    queue.append(tracks(5));
    let before = order(&queue);

    queue.toggle_shuffle();
    assert!(queue.armed());
    queue.toggle_shuffle();

    assert_eq!(order(&queue), before);
    assert!(
        queue.armed(),
        "nothing was playing, so nothing became current"
    );
    assert_eq!(queue.first_upcoming(), 0);
}

#[test]
fn restore_modes_on_an_empty_queue_sets_both_and_returns_true() {
    let mut queue = Queue::seeded(9);

    assert!(
        queue.restore_modes(Repeat::All, true),
        "an empty queue takes both saved modes"
    );
    assert_eq!(queue.repeat(), Repeat::All);
    assert!(queue.shuffle());
}

#[test]
fn restore_modes_with_rows_sets_only_repeat_and_returns_false() {
    let mut queue = Queue::seeded(9);
    queue.append(tracks(4));
    let before = order(&queue);

    assert!(
        !queue.restore_modes(Repeat::One, true),
        "rows already hold an order, so the shuffle flag is not applied"
    );
    assert_eq!(queue.repeat(), Repeat::One, "repeat is only a mode");
    assert!(!queue.shuffle(), "the loaded order was not drawn");
    assert!(queue.armed());
    assert_eq!(order(&queue), before);
}

#[test]
fn restore_modes_off_false_on_a_default_queue_changes_nothing() {
    let mut queue = Queue::default();

    assert!(
        queue.restore_modes(Repeat::Off, false),
        "an empty queue takes both saved modes"
    );
    assert_eq!(queue.repeat(), Repeat::Off);
    assert!(!queue.shuffle());
    assert!(queue.tracks.is_empty());
    assert!(queue.snapshot.is_empty());
}

#[test]
fn the_first_play_of_an_armed_restored_shuffle_queue_draws_the_rest() {
    let list = tracks(4);
    let mut queue = Queue::seeded(9);
    queue.restore_modes(Repeat::Off, true);
    queue.append(list.clone());
    let mut transport = Transport::default();

    assert_eq!(
        queue.toggle(&mut transport),
        Some(play("t0")),
        "the first row is the user's choice"
    );

    let mut drawn = Queue::seeded(9);
    drawn.start_with_shuffle(&list, 0, None, &mut Transport::default());
    assert_eq!(
        order(&queue),
        order(&drawn),
        "the rest is drawn as a started list draws it"
    );
    assert!(holds_exactly(&queue, 4), "every row is still queued");
    assert!(
        !queue.snapshot.is_empty(),
        "shuffle off needs the list's order back"
    );
}

#[test]
fn shuffle_off_after_an_armed_first_play_restores_list_order() {
    let mut queue = Queue::seeded(9);
    queue.restore_modes(Repeat::Off, true);
    queue.append(tracks(4));
    queue.toggle(&mut Transport::default());

    queue.toggle_shuffle();

    assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "t3"]));
    assert_eq!(
        queue.current().map(|entry| entry.uri.as_str()),
        Some("t0"),
        "the playing row keeps its entry"
    );
    assert_eq!(queue.index, Some(0));
}

#[test]
fn a_jump_on_an_armed_restored_shuffle_queue_draws_only_the_rows_after_it() {
    let mut queue = Queue::seeded(9);
    queue.restore_modes(Repeat::Off, true);
    queue.append(tracks(4));
    let mut transport = Transport::default();

    assert_eq!(queue.jump(1, &mut transport), Some(play("t1")));

    let order = order(&queue);
    assert_eq!(
        order[..2],
        uris(&["t0", "t1"]),
        "the row jumped to and the ones before it keep their place"
    );
    let mut tail = order[2..].to_vec();
    tail.sort();
    assert_eq!(tail, uris(&["t2", "t3"]), "the rest is a permutation");
    assert_ne!(
        &order[2..],
        &uris(&["t2", "t3"]),
        "a drawn order never repeats the one the rows had"
    );
    assert_eq!(
        queue.snapshot,
        uris(&["t0", "t1", "t2", "t3"]),
        "the snapshot holds the list's order"
    );
    assert_eq!(queue.index, Some(1));
}

#[test]
fn a_one_row_armed_shuffle_queue_plays_its_only_row() {
    let mut queue = Queue::seeded(9);
    queue.restore_modes(Repeat::Off, true);
    queue.append(tracks(1));
    let mut transport = Transport::default();

    assert_eq!(queue.toggle(&mut transport), Some(play("t0")));

    assert_eq!(order(&queue), uris(&["t0"]));
    assert_eq!(queue.index, Some(0));
}

#[test]
fn a_jump_to_the_last_row_of_an_armed_shuffle_queue_plays_it() {
    let mut queue = Queue::seeded(9);
    queue.restore_modes(Repeat::Off, true);
    queue.append(tracks(4));
    let mut transport = Transport::default();

    assert_eq!(queue.jump(3, &mut transport), Some(play("t3")));

    assert_eq!(queue.index, Some(3));
    assert_eq!(
        queue.current().map(|entry| entry.uri.as_str()),
        Some("t3"),
        "the row jumped to is the one that plays"
    );
    assert_eq!(
        order(&queue),
        uris(&["t0", "t1", "t2", "t3"]),
        "there is no tail to draw, so every row keeps its place"
    );
}

#[test]
fn the_first_play_of_a_drawn_queue_clears_the_picks() {
    let mut queue = Queue::seeded(9);
    queue.restore_modes(Repeat::Off, true);
    queue.append(tracks(4));
    queue.press(2, Press::Replace);
    queue.refocus_at(3);
    assert_eq!(picked(&queue), vec![2]);
    let mut transport = Transport::default();

    queue.toggle(&mut transport);

    assert!(
        picked(&queue).is_empty(),
        "the picks name rows that now hold other tracks"
    );
    assert!(queue.refocus().is_none());
}

#[test]
fn an_armed_queue_with_shuffle_off_is_not_drawn_on_first_play() {
    let mut queue = Queue::seeded(9);
    queue.append(tracks(4));
    let mut transport = Transport::default();

    assert_eq!(queue.toggle(&mut transport), Some(play("t0")));

    assert_eq!(order(&queue), uris(&["t0", "t1", "t2", "t3"]));
    assert!(queue.snapshot.is_empty(), "nothing was drawn");
}

#[test]
fn a_second_toggle_or_jump_after_the_first_play_does_not_redraw() {
    let mut queue = Queue::seeded(9);
    queue.restore_modes(Repeat::Off, true);
    queue.append(tracks(4));
    let mut transport = Transport::default();
    queue.toggle(&mut transport);
    let before = order(&queue);
    let rng = queue.rng;

    queue.toggle(&mut transport);
    queue.jump(2, &mut transport);

    assert_eq!(order(&queue), before, "the draw happens once");
    assert_eq!(queue.rng, rng, "and the generator is not advanced again");
}

#[test]
fn shuffle_toggled_on_an_armed_queue_is_not_drawn_twice() {
    let mut queue = Queue::seeded(9);
    queue.append(tracks(5));
    queue.toggle_shuffle();
    let before = order(&queue);
    let rng = queue.rng;
    let mut transport = Transport::default();

    queue.toggle(&mut transport);

    assert_eq!(order(&queue), before, "the first play kept the drawn order");
    assert_eq!(queue.rng, rng, "and drew nothing itself");
}

#[test]
fn shuffle_off_with_a_load_in_flight_keeps_the_guard_and_the_entry() {
    let mut queue = started(5, 1);
    let mut transport = Transport::default();

    queue.toggle_shuffle();
    queue.toggle_shuffle();

    assert!(queue.awaiting_start, "a mode switch is no play");
    assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("t1"));
    assert_eq!(
        end(&mut queue, &mut transport),
        None,
        "the load in flight is still ignored"
    );
}

#[test]
fn the_same_seed_gives_the_same_queue_and_two_seeds_differ() {
    let first = shuffled_at(6, 0, 1);
    let second = shuffled_at(6, 0, 1);
    let other = shuffled_at(6, 0, 2);

    assert_eq!(order(&first), order(&second));
    assert_ne!(order(&first), order(&other));
}

/// The band's shuffle is the mode switch, not a one-off: it starts the list
/// in a new order and leaves shuffle on for whatever plays next.
#[test]
fn start_with_shuffle_turns_the_mode_on_and_keeps_it_on() {
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    let list = tracks(8);
    assert!(!queue.shuffle());

    assert_eq!(
        queue.start_with_shuffle(&list, 0, None, &mut transport),
        Some(play("t0"))
    );

    assert!(queue.shuffle(), "the switch stayed on");
    assert_eq!(queue.index, Some(0), "the first row is what plays");
    assert_ne!(
        &order(&queue)[1..],
        &uris(&["t1", "t2", "t3", "t4", "t5", "t6", "t7"]),
        "the coming rows are not the list's order"
    );
    assert!(holds_exactly(&queue, 8));
}

#[test]
fn start_with_shuffle_on_plays_the_chosen_row_first_and_shuffles_the_rest() {
    let mut queue = Queue::seeded(13);
    queue.append(tracks(2));
    queue.toggle_shuffle();
    let mut transport = Transport::default();
    let list = tracks(6);

    assert_eq!(
        queue.start(&list, 3, None, &mut transport),
        Some(play("t3"))
    );

    assert_eq!(queue.index, Some(0), "the clicked row is what plays");
    assert_eq!(order(&queue)[0], "t3");
    assert!(holds_exactly(&queue, 6));
    assert_ne!(
        &order(&queue)[1..],
        &uris(&["t0", "t1", "t2", "t4", "t5"]),
        "the rest kept the list's order"
    );
}

#[test]
fn shuffle_and_repeat_survive_a_new_start() {
    let mut queue = started(3, 0);
    let mut transport = Transport::default();
    queue.cycle_repeat();
    queue.toggle_shuffle();

    let list = tracks(2);
    assert_eq!(
        queue.start(&list, 1, None, &mut transport),
        Some(play("t1"))
    );

    assert!(queue.shuffle());
    assert_eq!(queue.repeat(), Repeat::All);
}

#[test]
fn a_second_start_replaces_the_snapshot() {
    let mut queue = Queue::seeded(3);
    let mut transport = Transport::default();
    queue.toggle_shuffle();
    queue.start(&tracks(4), 0, None, &mut transport);

    let second = vec![track("x"), track("y"), track("z")];
    assert_eq!(
        queue.start(&second, 1, None, &mut transport),
        Some(play("y"))
    );
    queue.toggle_shuffle();

    assert_eq!(
        order(&queue),
        uris(&["x", "y", "z"]),
        "the second list's order, not the first list's"
    );
    assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("y"));
}

#[test]
fn a_bad_index_start_changes_nothing_with_shuffle_on() {
    let mut queue = shuffled_at(4, 1, 6);
    let mut transport = Transport::default();
    let before = (
        order(&queue),
        queue.index,
        queue.awaiting_start,
        queue.skips,
        queue.snapshot.clone(),
    );

    assert_eq!(queue.start(&tracks(2), 5, None, &mut transport), None);
    assert_eq!(queue.start(&[], 0, None, &mut transport), None);

    assert_eq!(
        (
            order(&queue),
            queue.index,
            queue.awaiting_start,
            queue.skips,
            queue.snapshot.clone(),
        ),
        before,
        "a refused start must not draw, disarm or reseed anything"
    );
}

#[test]
fn wrapping_under_shuffle_reshuffles_the_tail_and_keeps_row_zero() {
    let mut queue = seeded_started(6, 0, 21);
    let mut transport = Transport::default();
    queue.cycle_repeat();
    queue.toggle_shuffle();
    for _ in 0..5 {
        assert!(queue.next(&mut transport).is_some());
    }
    assert_eq!(queue.index, Some(5), "the lap is at its last row");
    let row_zero = order(&queue)[0].clone();
    let tail = order(&queue)[1..].to_vec();

    assert_eq!(queue.next(&mut transport), Some(play(&row_zero)));

    assert_eq!(queue.index, Some(0));
    assert_eq!(order(&queue)[0], row_zero, "a wrap must not move row zero");
    assert_ne!(
        order(&queue)[1..],
        tail[..],
        "lap two replayed lap one's order"
    );
    assert!(holds_exactly(&queue, 6));
}

/// A queue of `count` tracks started at `index` from `seed`, not yet shuffled.
fn seeded_started(count: usize, index: usize, seed: u64) -> Queue {
    let list = tracks(count);
    let mut queue = Queue::seeded(seed);
    let mut transport = Transport::default();
    queue.start(&list, index, None, &mut transport);
    queue
}
