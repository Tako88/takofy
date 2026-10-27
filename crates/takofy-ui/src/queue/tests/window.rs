use super::super::*;
use super::test_support::*;

/// One row the phone sends: a uri and nothing else.
fn word(name: &str) -> String {
    format!("spotify:track:{name}")
}

/// A window's rows, each named the way the phone names one.
fn window(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|row| word(row)).collect()
}

#[test]
fn add_next_lands_after_the_current_row_and_after_earlier_adds() {
    let mut queue = started(3, 0);

    assert_eq!(queue.add_next(vec![track("x")]), Some(1..2));
    assert_eq!(queue.add_next(vec![track("y")]), Some(2..3));

    assert_eq!(order(&queue), uris(&["t0", "x", "y", "t1", "t2"]));
    assert_eq!(queue.queued, 2);
    assert_eq!(
        upcoming(&queue),
        vec![
            (1, "x".to_owned()),
            (2, "y".to_owned()),
            (3, "t1".to_owned()),
            (4, "t2".to_owned()),
        ],
        "the context resumes after the block"
    );
}

#[test]
fn queued_shrinks_when_the_queue_advances_past_it() {
    let mut queue = started(3, 0);
    let mut transport = Transport::default();
    queue.add_next(vec![track("x")]);
    queue.add_next(vec![track("y")]);

    assert_eq!(queue.next(&mut transport), Some(play("x")));
    assert_eq!(queue.queued, 1, "x was the row the phone queued first");
    assert_eq!(queue.next(&mut transport), Some(play("y")));
    assert_eq!(queue.queued, 0);
    assert_eq!(queue.next(&mut transport), Some(play("t1")));
    assert_eq!(queue.queued, 0, "a hop past the block cannot go negative");
}

#[test]
fn a_skip_past_the_queued_block_clears_it() {
    let mut queue = started(4, 0);
    let mut transport = Transport::default();
    queue.add_next(vec![track("x")]);
    assert_eq!(queue.queued, 1);

    assert_eq!(queue.jump(2, &mut transport), Some(play("t1")));
    assert_eq!(queue.queued, 0, "the block is behind the row that plays");

    queue.add_next(vec![track("w")]);
    assert_eq!(
        order(&queue),
        uris(&["t0", "x", "t1", "w", "t2", "t3"]),
        "an add after a skip past the block sits right after the current row"
    );
}

#[test]
fn a_skip_inside_the_block_shrinks_it() {
    let mut queue = started(3, 0);
    let mut transport = Transport::default();
    queue.add_next(vec![track("x")]);
    queue.add_next(vec![track("y")]);
    assert_eq!(queue.queued, 2);

    assert_eq!(queue.jump(1, &mut transport), Some(play("x")));
    assert_eq!(queue.queued, 1, "the row that played left the block");

    queue.add_next(vec![track("w")]);
    assert_eq!(
        order(&queue),
        uris(&["t0", "x", "y", "w", "t1", "t2"]),
        "the rest of the block still plays before the context"
    );
}

#[test]
fn a_move_back_out_of_the_block_clears_it() {
    let mut queue = started(4, 1);
    let mut transport = Transport::default();
    queue.add_next(vec![track("x")]);
    assert_eq!(queue.queued, 1);

    // Back to a history row: the block is no longer counted from here.
    assert_eq!(queue.previous(0, &mut transport), Some(play("t0")));
    assert_eq!(queue.queued, 0);

    queue.add_next(vec![track("w")]);
    assert_eq!(
        order(&queue),
        uris(&["t0", "w", "t1", "x", "t2", "t3"]),
        "an add goes right after the row that plays, not into the old block"
    );
}

#[test]
fn removing_rows_clamps_the_queued_block() {
    let mut queue = started(3, 0);
    queue.add_next(vec![track("x")]);
    queue.add_next(vec![track("y")]);
    assert_eq!(queue.queued, 2);

    let gone = queue.entries_at(&[2]);
    assert_eq!(
        queue.remove_rows(&[2], &gone),
        Some(1),
        "the second block row"
    );
    assert_eq!(queue.queued, 1, "the block counts the rows that remain");

    queue.add_next(vec![track("w")]);
    assert_eq!(
        order(&queue),
        uris(&["t0", "x", "w", "t1", "t2"]),
        "an add goes after the rows that are still queued"
    );
}

#[test]
fn start_adopt_and_a_shuffle_toggle_reset_the_queued_block() {
    let mut queue = started(3, 0);
    let mut transport = Transport::default();
    queue.add_next(vec![track("x")]);

    queue.toggle_shuffle();
    assert_eq!(queue.queued, 0, "the draw moved the rows the block named");

    queue.add_next(vec![track("y")]);
    queue.start(&tracks(3), 0, None, &mut transport);
    assert_eq!(queue.queued, 0, "a new list has no phone block");

    queue.add_next(vec![track("z")]);
    assert!(queue.adopt(&landing(&["a", "b"], 0), &mut transport));
    assert_eq!(queue.queued, 0, "a landing replaces the rows");
}

#[test]
fn replace_window_keeps_the_current_row_and_puts_the_index_after_prev() {
    let mut queue = started(2, 1);

    assert_eq!(
        queue.replace_window(&window(&["a", "b"]), &window(&["c"]), "rev-1".to_owned()),
        Ok(())
    );

    assert_eq!(
        order(&queue),
        vec![word("a"), word("b"), "t1".to_owned(), word("c"),],
        "the window sits around the row that is playing"
    );
    assert_eq!(queue.index, Some(2));
    assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("t1"));
}

#[test]
fn replace_window_replaces_everything_else() {
    let mut queue = started(6, 2);

    assert_eq!(
        queue.replace_window(&window(&["a"]), &window(&["b"]), "rev-1".to_owned()),
        Ok(())
    );

    assert_eq!(order(&queue), vec![word("a"), "t2".to_owned(), word("b")]);
    assert_eq!(
        queue.index,
        Some(1),
        "the index sits after the rows before it"
    );
    assert_eq!(
        queue.current().map(|entry| entry.uri.as_str()),
        Some("t2"),
        "the row that is playing stays current"
    );
    assert_eq!(
        queue
            .history(usize::MAX)
            .map(|entry| entry.uri.as_str())
            .collect::<Vec<_>>(),
        vec!["spotify:track:a"],
        "the rows before the current one are the phone's alone"
    );
    assert_eq!(
        queue
            .upcoming()
            .map(|(_, entry)| entry.uri.as_str())
            .collect::<Vec<_>>(),
        vec!["spotify:track:b"],
        "and the rows after it are the phone's alone too"
    );
    assert_eq!(
        queue.tracks.len(),
        3,
        "the five rows the window did not name are gone"
    );
}

#[test]
fn replace_window_drops_delimiter_rows_from_both_lists() {
    let mut queue = started(1, 0);
    let mut prev = window(&["a", "b"]);
    prev.insert(1, DELIMITER_URI.to_owned());
    let mut next = window(&["c"]);
    next.insert(0, DELIMITER_URI.to_owned());

    assert_eq!(
        queue.replace_window(&prev, &next, "rev-1".to_owned()),
        Ok(())
    );

    assert_eq!(
        order(&queue),
        vec![word("a"), word("b"), "t0".to_owned(), word("c")]
    );
    assert_eq!(queue.index, Some(2), "a separator is not a row");
}

#[test]
fn an_empty_uri_row_becomes_a_copy_of_the_current_row() {
    let mut queue = started(2, 1);

    assert_eq!(
        queue.replace_window(&[String::new()], &[String::new()], "rev-1".to_owned()),
        Ok(())
    );

    assert_eq!(order(&queue), uris(&["t1", "t1", "t1"]));
    assert_eq!(queue.index, Some(1));
}

#[test]
fn an_empty_window_leaves_only_the_current_row() {
    let mut queue = started(3, 1);

    assert_eq!(queue.replace_window(&[], &[], "rev-1".to_owned()), Ok(()));

    assert_eq!(order(&queue), uris(&["t1"]));
    assert_eq!(queue.index, Some(0));
    assert!(!queue.can_next(), "nothing follows the current row");
}

#[test]
fn a_local_row_with_colons_is_a_row() {
    for uri in [
        "spotify:track:abc",
        "spotify:episode:abc",
        "spotify:local:A:B:C:222",
    ] {
        assert!(spotify_uri(uri), "must accept {uri}");
    }
    for uri in [
        "spotify:album:a",
        "spotify:show:s",
        "spotify:track:",
        "spotify:local:",
        "spotify::x",
        "t2",
    ] {
        assert!(!spotify_uri(uri), "must refuse {uri}");
    }
}

#[test]
fn a_window_of_local_rows_is_accepted() {
    let mut queue = started(2, 1);
    let local = "spotify:local:A:B:C:222".to_owned();
    let rows = vec![local.clone()];

    assert_eq!(
        queue.replace_window(&rows, &rows, "rev-1".to_owned()),
        Ok(())
    );

    assert_eq!(
        order(&queue),
        vec![local.clone(), "t1".to_owned(), local],
        "a local file is a row the queue can be handed"
    );
    assert_eq!(queue.index, Some(1));
}

#[test]
fn an_unparsable_row_refuses_and_changes_nothing() {
    let mut queue = started(3, 1);
    let before = order(&queue);
    let good = word("a");
    let bad = [
        "https://example.test/row".to_owned(),
        "t2".to_owned(),
        "spotify:track:".to_owned(),
        "spotify::x".to_owned(),
        "spotify:".to_owned(),
        "spotify:foo:bar".to_owned(),
    ];

    for uri in bad {
        // A bad row in either list refuses the whole window, so a caller
        // that applies one list before validating the other cannot keep
        // the half it already wrote.
        for (prev, next) in [
            (vec![uri.clone()], vec![good.clone()]),
            (vec![good.clone()], vec![uri.clone()]),
        ] {
            assert_eq!(
                queue.replace_window(&prev, &next, "rev-1".to_owned()),
                Err(WindowRefusal::BadRow),
                "{uri}"
            );
            assert_eq!(order(&queue), before, "{uri}");
            assert_eq!(queue.index, Some(1), "{uri}");
            assert!(queue.queue_revision.is_empty(), "{uri} stored a revision");
        }
    }
}

#[test]
fn an_oversized_window_refuses_and_changes_nothing() {
    let mut queue = started(2, 0);
    let before = order(&queue);
    // One row past the bound, split so neither list alone is too large.
    let half = MAX_WINDOW_ROWS / 2;
    let rows = |count: usize, prefix: &str| -> Vec<String> {
        (0..count)
            .map(|row| word(&format!("{prefix}{row}")))
            .collect()
    };
    let prev = rows(half, "p");
    let next = rows(half + 1, "n");

    assert_eq!(
        queue.replace_window(&prev, &next, "rev-1".to_owned()),
        Err(WindowRefusal::TooLarge)
    );
    assert_eq!(order(&queue), before);
    assert_eq!(queue.index, Some(0));
    assert!(queue.queue_revision.is_empty());

    assert_eq!(
        queue.replace_window(&prev, &next[..half], "rev-1".to_owned()),
        Ok(()),
        "the bound itself is a window"
    );
}

#[test]
fn replace_window_with_no_current_entry_is_refused() {
    let mut queue = armed(3);
    let before = order(&queue);

    assert_eq!(
        queue.replace_window(&window(&["a"]), &window(&["b"]), "rev-1".to_owned()),
        Err(WindowRefusal::NoCurrent)
    );

    assert_eq!(order(&queue), before);
    assert!(queue.armed(), "the rows still wait for a first play");
}

#[test]
fn replace_window_stores_the_newest_revision() {
    let mut queue = started(2, 0);

    assert_eq!(
        queue.replace_window(&[], &window(&["a"]), "rev-1".to_owned()),
        Ok(())
    );
    assert_eq!(queue.queue_revision, "rev-1");
    assert_eq!(
        queue.replace_window(&[], &window(&["b"]), "rev-2".to_owned()),
        Ok(())
    );
    assert_eq!(queue.queue_revision, "rev-2", "the newest one is kept");

    assert_eq!(
        queue.replace_window(&["nonsense".to_owned()], &[], "rev-3".to_owned()),
        Err(WindowRefusal::BadRow)
    );
    assert_eq!(queue.queue_revision, "rev-2", "a refusal stored nothing");
}

#[test]
fn replace_window_clears_selection_and_shuffle_snapshot() {
    let mut queue = started(5, 1);
    queue.toggle_shuffle();
    queue.press(2, Press::Replace);
    queue.press(3, Press::Toggle);
    queue.refocus_at(2);
    assert_eq!(picked(&queue), vec![2, 3]);

    assert_eq!(
        queue.replace_window(&window(&["a"]), &window(&["b", "c"]), "rev-1".to_owned()),
        Ok(())
    );

    assert!(
        picked(&queue).is_empty(),
        "a pick cannot survive other rows"
    );
    assert_eq!(queue.refocus(), None);
    assert!(
        queue.snapshot.is_empty(),
        "the drawn order belonged to the rows that went"
    );
}

#[test]
fn replace_window_forgets_the_skip_run_the_queued_block_and_the_label_asks() {
    let mut queue = started(3, 0);
    assert!(queue.add_next_uri(&word("x")).is_some());
    queue.skips = MAX_CONSECUTIVE_SKIPS;
    assert!(
        queue.label_wants().is_some(),
        "the first ask covers the rows as they were"
    );

    assert_eq!(
        queue.replace_window(&[], &window(&["x"]), "rev-1".to_owned()),
        Ok(())
    );

    assert_eq!(queue.queued, 0, "a window with no separator names no block");
    assert_eq!(queue.skips, 0, "a window is not a run of failed loads");
    assert_eq!(
        queue.label_wants(),
        Some(Command::LabelRows {
            uris: window(&["x"])
        }),
        "a row the window replaced is asked about again"
    );
}

#[test]
fn a_pushed_window_seeds_the_queued_block_from_its_delimiter() {
    let mut queue = started(2, 0);
    let mut next = window(&["x", "y"]);
    next.push(DELIMITER_URI.to_owned());
    next.push(word("z"));

    assert_eq!(queue.replace_window(&[], &next, "rev-1".to_owned()), Ok(()));
    assert_eq!(
        queue.queued, 2,
        "the rows before the separator are the phone's own block"
    );

    queue.add_next(vec![track("w")]);
    assert_eq!(
        order(&queue),
        vec![
            "t0".to_owned(),
            word("x"),
            word("y"),
            "w".to_owned(),
            word("z"),
        ],
        "the block plays before the context resumes"
    );
}

#[test]
fn a_pushed_window_without_a_delimiter_queues_nothing() {
    let mut queue = started(2, 0);
    queue.add_next(vec![track("x")]);
    assert_eq!(queue.queued, 1);

    assert_eq!(
        queue.replace_window(&[], &window(&["y", "z"]), "rev-1".to_owned()),
        Ok(())
    );
    assert_eq!(queue.queued, 0, "a window with no separator names no block");

    queue.add_next(vec![track("w")]);
    assert_eq!(
        order(&queue),
        vec!["t0".to_owned(), "w".to_owned(), word("y"), word("z")],
        "an add goes right after the current row"
    );
}

#[test]
fn a_local_start_forgets_the_pushed_revision() {
    let mut queue = started(2, 0);
    let mut transport = Transport::default();
    assert_eq!(
        queue.replace_window(&[], &window(&["a"]), "rev-1".to_owned()),
        Ok(())
    );
    assert_eq!(queue.queue_revision(), "rev-1");

    queue.start(&tracks(2), 0, None, &mut transport);

    assert_eq!(
        queue.queue_revision(),
        "",
        "a list this window started is not the phone's window"
    );
}

#[test]
fn adopting_a_new_landing_forgets_the_pushed_revision() {
    let mut queue = started(2, 0);
    let mut transport = Transport::default();
    assert_eq!(
        queue.replace_window(&[], &window(&["a"]), "rev-1".to_owned()),
        Ok(())
    );

    assert!(queue.adopt(&landing(&["a", "b"], 0), &mut transport));

    assert_eq!(
        queue.queue_revision(),
        "",
        "a landing names the rows again, without the phone's revision"
    );
}

#[test]
fn a_local_edit_forgets_the_pushed_revision() {
    let mut queue = started(4, 1);
    let pushed = window(&["a", "b", "c"]);

    assert_eq!(
        queue.replace_window(&[], &pushed, "rev-1".to_owned()),
        Ok(())
    );
    assert!(queue.add_next_uri(&word("x")).is_some());
    assert_eq!(
        queue.queue_revision(),
        "",
        "an add is not the window the phone sent"
    );

    assert_eq!(
        queue.replace_window(&[], &pushed, "rev-1".to_owned()),
        Ok(())
    );
    let gone = queue.entries_at(&[2]);
    assert_eq!(queue.remove_rows(&[2], &gone), Some(1));
    assert_eq!(
        queue.queue_revision(),
        "",
        "a removal is not that window either"
    );

    assert_eq!(
        queue.replace_window(&[], &pushed, "rev-1".to_owned()),
        Ok(())
    );
    assert!(queue.move_rows(&[1], 4).is_some());
    assert_eq!(queue.queue_revision(), "", "a move is an edit too");

    assert_eq!(
        queue.replace_window(&[], &pushed, "rev-1".to_owned()),
        Ok(())
    );
    assert!(queue.step_rows(&[3], Step::Up).is_some());
    assert_eq!(queue.queue_revision(), "", "a step is an edit too");
}
