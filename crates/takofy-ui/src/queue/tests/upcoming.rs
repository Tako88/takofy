use super::super::*;
use super::test_support::*;

#[test]
fn an_empty_queue_has_nothing_upcoming() {
    let queue = Queue::default();
    assert_eq!(upcoming(&queue), Vec::new());
    assert_eq!(queue.upcoming().len(), 0);
}

#[test]
fn the_last_entry_has_nothing_upcoming() {
    let tracks = tracks(3);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&tracks, 2, None, &mut transport);

    assert_eq!(upcoming(&queue), Vec::new());
    assert!(!queue.can_next(), "nothing upcoming is exactly `!can_next`");
}

#[test]
fn upcoming_lists_what_follows_the_current_entry_with_absolute_indices() {
    let tracks = tracks(5);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&tracks, 1, None, &mut transport);

    assert_eq!(
        upcoming(&queue),
        vec![
            (2, "t2".to_owned()),
            (3, "t3".to_owned()),
            (4, "t4".to_owned()),
        ],
        "the current entry is never listed, and the indices are absolute"
    );
}

#[test]
fn upcoming_shrinks_as_the_queue_advances() {
    let tracks = tracks(4);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&tracks, 0, None, &mut transport);

    assert_eq!(queue.upcoming().len(), 3);
    for (step, remaining) in [(1, 2), (2, 1), (3, 0)] {
        assert_eq!(queue.next(&mut transport), Some(play(&format!("t{step}"))));
        assert_eq!(
            queue.upcoming().len(),
            remaining,
            "after advancing to t{step}"
        );
    }
}

#[test]
fn upcoming_reports_an_exact_length() {
    let tracks = tracks(6);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&tracks, 2, None, &mut transport);

    let mut iter = queue.upcoming();
    assert_eq!(iter.len(), 3);
    let _ = iter.next();
    assert_eq!(iter.len(), 2, "`ExactSizeIterator` counts what is left");
    let _ = iter.next();
    let _ = iter.next();
    assert_eq!(iter.len(), 0);
    assert!(iter.next().is_none());
}

#[test]
fn duplicate_entries_get_distinct_upcoming_indices() {
    let duplicates = vec![track("same"), track("same"), track("same")];
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&duplicates, 0, None, &mut transport);

    assert_eq!(
        upcoming(&queue),
        vec![(1, "same".to_owned()), (2, "same".to_owned())],
        "identical uris are two entries, not one"
    );
}

#[test]
fn jump_plays_that_entry_and_keeps_the_rest_of_the_list() {
    let tracks = tracks(4);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&tracks, 0, None, &mut transport);

    assert_eq!(queue.jump(2, &mut transport), Some(play("t2")));
    assert_eq!(
        transport.now_playing.as_ref().map(|now| now.uri.as_str()),
        Some("t2")
    );
    assert_eq!(
        upcoming(&queue),
        vec![(3, "t3".to_owned())],
        "the entries after the target stay reachable"
    );
    assert_eq!(
        queue.previous(1_000, &mut transport),
        Some(play("t1")),
        "the entries before the target stay reachable"
    );
}

#[test]
fn jump_sets_the_guard_so_a_stale_end_is_ignored() {
    // The guard covers an event arriving before *any* event of the new load,
    // not only its first: `Loading`, `Duration`, `Position` and `Failed` all
    // clear it, so after one of them a trailing event is counted as the new
    // load's own.
    let tracks = tracks(4);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&tracks, 0, None, &mut transport);
    clear_guard(&mut queue, &mut transport);

    assert_eq!(queue.jump(2, &mut transport), Some(play("t2")));
    assert_eq!(
        end(&mut queue, &mut transport),
        None,
        "an end arriving before any event of the new load is the old track's"
    );
    clear_guard(&mut queue, &mut transport);
    assert_eq!(
        end(&mut queue, &mut transport),
        Some(play("t3")),
        "a later end advances from the jumped-to entry"
    );
}

#[test]
fn a_jump_during_a_failure_run_is_carried_past_its_own_entry() {
    // An accepted race, shared with `next`: once the clicked entry's
    // `Loading` has cleared the guard, a trailing `Unavailable` from the
    // entry the run left behind is indistinguishable from the clicked
    // entry's own, so the queue advances once more and skips the click.
    let tracks = tracks(8);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&tracks, 0, None, &mut transport);

    // One failed load has already carried the queue to t1.
    assert_eq!(skip(&mut queue, &mut transport), Some(play("t1")));

    // The click lands while that failure's load is still in flight.
    assert_eq!(queue.jump(4, &mut transport), Some(play("t4")));
    clear_guard(&mut queue, &mut transport);
    assert_eq!(
        feed(&mut queue, &mut transport, TransportUpdate::Unavailable),
        Some(play("t5")),
        "the old load's trailing failure skips the clicked entry"
    );
    assert_eq!(
        upcoming(&queue),
        vec![(6, "t6".to_owned()), (7, "t7".to_owned())],
        "the queue sits one past the entry the click picked"
    );
}

#[test]
fn jump_resets_the_skip_streak() {
    let tracks = tracks(16);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&tracks, 0, None, &mut transport);

    for _ in 0..=MAX_CONSECUTIVE_SKIPS {
        skip(&mut queue, &mut transport);
    }
    assert_eq!(skip(&mut queue, &mut transport), None, "capped");

    assert_eq!(queue.jump(7, &mut transport), Some(play("t7")));
    for i in 8..=12 {
        assert_eq!(
            skip(&mut queue, &mut transport),
            Some(play(&format!("t{i}"))),
            "the jump must have reset the streak"
        );
    }
    assert_eq!(skip(&mut queue, &mut transport), None);
}

#[test]
fn an_out_of_range_jump_changes_nothing_at_all() {
    let tracks = tracks(3);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&tracks, 1, None, &mut transport);
    // A load is in flight: the guard must survive a refused click.
    let before = transport.clone();

    for index in [3, 4, usize::MAX] {
        assert_eq!(queue.jump(index, &mut transport), None, "{index}");
    }

    assert_eq!(
        transport, before,
        "a refused jump must not touch the transport"
    );
    assert_eq!(
        queue.previous(1_000, &mut transport),
        Some(play("t0")),
        "the index never moved"
    );
}

#[test]
fn an_out_of_range_jump_leaves_the_guard_armed() {
    let tracks = tracks(3);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&tracks, 0, None, &mut transport);

    assert_eq!(queue.jump(9, &mut transport), None);
    assert_eq!(
        end(&mut queue, &mut transport),
        None,
        "a refused click must not disarm an in-flight load"
    );
    clear_guard(&mut queue, &mut transport);
    assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));
}

#[test]
fn jump_to_the_last_entry_leaves_nothing_upcoming() {
    let tracks = tracks(3);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&tracks, 0, None, &mut transport);

    assert_eq!(queue.jump(2, &mut transport), Some(play("t2")));
    assert!(queue.upcoming().next().is_none());
    assert!(!queue.can_next());
}

#[test]
fn jump_back_to_the_first_entry_still_restarts_it() {
    let tracks = tracks(3);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&tracks, 2, None, &mut transport);

    assert_eq!(queue.jump(0, &mut transport), Some(play("t0")));
    seekable(&mut queue, &mut transport, 1_000);
    assert_eq!(
        queue.previous(1_000, &mut transport),
        Some(Command::Seek { position_ms: 0 }),
        "the first entry has nothing before it, so Previous restarts it"
    );
}

#[test]
fn previous_after_a_jump_steps_to_the_entry_before_the_target() {
    let tracks = tracks(5);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&tracks, 0, None, &mut transport);

    assert_eq!(queue.jump(3, &mut transport), Some(play("t3")));
    // A position inside the restart threshold is a step back, not a restart.
    assert_eq!(
        queue.previous(RESTART_AFTER_MS, &mut transport),
        Some(play("t2")),
        "the threshold itself is a step back"
    );
}

#[test]
fn jump_to_the_current_index_replays_it() {
    let tracks = tracks(3);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&tracks, 1, None, &mut transport);
    clear_guard(&mut queue, &mut transport);

    assert_eq!(queue.jump(1, &mut transport), Some(play("t1")));
    assert_eq!(
        upcoming(&queue),
        vec![(2, "t2".to_owned())],
        "replaying leaves what follows it alone"
    );
}

#[test]
fn jump_passes_the_entrys_artists_into_now_playing() {
    let tracks = vec![track_by("a", "A Band"), track_by("b", "B Band")];
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&tracks, 0, None, &mut transport);

    assert_eq!(queue.jump(1, &mut transport), Some(play("b")));
    assert_eq!(
        transport.now_playing.expect("a named track").artists,
        "B Band"
    );
}
