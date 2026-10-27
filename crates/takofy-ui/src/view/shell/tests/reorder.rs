use super::super::reorder;
use super::super::*; // the module under test — what `use super::*` gave the flat module
use super::*; // the slice's shared fixtures, from tests/mod.rs

#[test]
fn the_pick_a_carry_held_before_the_toggle_is_refused_after_it() {
    let mut transport = playing(cover("abc"));
    let mut queue = Queue::seeded(9);
    let tracks: Vec<TrackEntry> = (0..5).map(|index| track(index, None)).collect();
    queue.start(&tracks, 0, None, &mut transport);
    queue.press(1, Press::Replace);
    for row in 2..5 {
        queue.press(row, Press::Toggle);
    }
    let rows = queue.lift(1).expect("the coming rows are editable");
    let carry = Carry {
        source: Source::Queue,
        rows: rows.clone(),
        entries: queue.entries_at(&rows),
        ..carried_rows(egui::Id::unique("queue"), rows)
    };
    queue.toggle_shuffle();
    assert_ne!(
        queue.entries_at(&carry.rows),
        carry.entries,
        "the premise: the shuffle moved the carried rows"
    );
    let before = entries(&queue);

    land(
        &mut queue,
        &carry,
        Target::At(Landing {
            boundary: 2,
            marker: Rect::ZERO,
        }),
    );

    assert_eq!(
        entries(&queue),
        before,
        "a pick held across a toggle must not move other tracks"
    );
}

#[test]
fn a_drag_that_starts_inside_a_pick_carries_the_whole_block() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    page_of_tracks(&mut window, &["a", "b", "c", "d"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let view = view_rect(&window);
    let at = |title: &str| row_line(&frame, view, title);

    let _ = window.click(&mut transport, &mut queue, at("b"));
    window.held = egui::Modifiers::SHIFT;
    let _ = window.click(&mut transport, &mut queue, at("d"));
    window.held = egui::Modifiers::NONE;

    // The press is inside the pick, and the pick is what travels.
    let from = at("c");
    let to = page_boundary(&frame, view, &["a", "b", "c", "d"], 0);
    assert_eq!(
        window.drag(&mut transport, &mut queue, from, to),
        Some(Command::MoveTrack {
            playlist: PLAYLIST.to_owned(),
            revision: revision("rev-1"),
            moves: vec![Move {
                from: 1,
                length: 3,
                to: 0,
            }],
        }),
        "a contiguous block is one move of its own length"
    );
    assert_eq!(page_rows(&window), ["b", "c", "d", "a"]);
}

#[test]
fn a_gapped_pick_gathered_at_the_end_is_the_measured_chain() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    page_of_tracks(&mut window, &["a", "b", "c", "d"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let view = view_rect(&window);
    let at = |title: &str| row_line(&frame, view, title);

    let _ = window.click(&mut transport, &mut queue, at("a"));
    window.held = egui::Modifiers::COMMAND;
    let _ = window.click(&mut transport, &mut queue, at("c"));
    window.held = egui::Modifiers::NONE;

    let from = at("c");
    let to = page_boundary(&frame, view, &["a", "b", "c", "d"], 4);
    assert_eq!(
        window.drag(&mut transport, &mut queue, from, to),
        Some(Command::MoveTrack {
            playlist: PLAYLIST.to_owned(),
            revision: revision("rev-1"),
            moves: vec![
                Move {
                    from: 0,
                    length: 1,
                    to: 4,
                },
                Move {
                    from: 1,
                    length: 1,
                    to: 4,
                },
            ],
        }),
        "a gapped pair is a chain in one delta"
    );
    assert_eq!(page_rows(&window), ["b", "d", "a", "c"]);
}

#[test]
fn a_block_target_is_drawn_exactly_where_a_move_would_land() {
    let len = 6;
    let list = egui::Id::unique("list");
    let rows = Rect::from_min_size(
        egui::Pos2::ZERO,
        Vec2::new(300.0, len as f32 * extent::ROW_H),
    );
    let zone = crate::landing::Zone {
        hit: rows,
        list: rows,
        row_h: extent::ROW_H,
        first: 0,
        count: len,
    };
    let offer = Offer {
        list,
        playlist: PLAYLIST.to_owned(),
        zone,
    };
    let pointer = |boundary: usize| Some(egui::pos2(150.0, boundary as f32 * extent::ROW_H));

    for picks in [vec![1], vec![1, 2, 3], vec![1, 3]] {
        let carry = carried_rows(list, picks.clone());
        for boundary in 0..=len {
            let drawn = reorder_target(&carry, Some(&offer), pointer(boundary)).is_some();
            let moves = reorder::gather(len, &picks, boundary).is_some();
            assert_eq!(drawn, moves, "{picks:?} at {boundary}");
        }
    }

    // A single row's own two gaps, and a contiguous block's inner gaps.
    let single = carried_rows(list, vec![1]);
    assert_eq!(reorder_target(&single, Some(&offer), pointer(1)), None);
    assert_eq!(reorder_target(&single, Some(&offer), pointer(2)), None);
    let block = carried_rows(list, vec![1, 2, 3]);
    for boundary in 1..=4 {
        assert_eq!(
            reorder_target(&block, Some(&offer), pointer(boundary)),
            None,
            "the block already sits at {boundary}"
        );
    }

    // A gapped block draws a marker at every boundary but its own pick.
    let gapped = carried_rows(list, vec![1, 3]);
    for boundary in 0..=len {
        assert!(
            reorder_target(&gapped, Some(&offer), pointer(boundary)).is_some(),
            "a gapped block moves at every boundary: {boundary}"
        );
    }
}

#[test]
fn dragging_a_row_to_the_top_writes_index_zero() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1", "p2"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let view = view_rect(&window);
    let from = row_line(&frame, view, "p2");
    let to = page_boundary(&frame, view, &["p0", "p1", "p2"], 0);

    assert_eq!(
        window.drag(&mut transport, &mut queue, from, to),
        Some(Command::MoveTrack {
            playlist: PLAYLIST.to_owned(),
            revision: revision("rev-1"),
            moves: vec![Move {
                from: 2,
                length: 1,
                to: 0,
            }],
        })
    );
    assert_eq!(page_rows(&window), ["list:p2", "list:p0", "list:p1"]);
}

#[test]
fn a_drop_beside_itself_moves_nothing_and_writes_nothing() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1", "p2"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let view = view_rect(&window);
    for (title, beside) in [("p1", 1), ("p1", 2), ("p2", 3)] {
        assert_eq!(
            window.drag(
                &mut transport,
                &mut queue,
                row_line(&frame, view, title),
                page_boundary(&frame, view, &["p0", "p1", "p2"], beside)
            ),
            None,
            "the gap {beside} is where {title} already is"
        );
        assert_eq!(
            page_rows(&window),
            ["list:p0", "list:p1", "list:p2"],
            "nothing moved"
        );
        assert!(window.library.movable(), "and nothing was written");
    }
}

#[test]
fn a_drop_beside_itself_draws_no_marker_and_a_flat_ghost() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1", "p2"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let view = view_rect(&window);
    for (title, beside) in [("p0", 1), ("p1", 2)] {
        let from = row_line(&frame, view, title);
        let to = page_boundary(&frame, view, &["p0", "p1", "p2"], beside);
        let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);
        assert!(
            carry::carrying(&window.ctx),
            "the gap {beside} of {title} is reached by a drag"
        );
        assert!(
            marker(&frame).is_none(),
            "the gap {beside} of {title} marks no landing"
        );
        assert_eq!(
            ghost_at(&frame, to),
            Some(Tokens::DARK.palette.surface),
            "the gap {beside} of {title} draws the ghost flat"
        );
        let corner = to + Vec2::new(space::S3, space::S4);
        assert!(
            !shadows(&frame)
                .iter()
                .any(|cast| (cast.min - corner).length() < 0.5),
            "a flat ghost casts no shadow"
        );
        let (command, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);
        assert_eq!(command, None, "the gap {beside} of {title} writes nothing");
        assert_eq!(
            page_rows(&window),
            ["list:p0", "list:p1", "list:p2"],
            "and moves nothing"
        );
    }
}

#[test]
fn a_row_that_changed_under_the_drag_is_refused_on_release() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1", "p2"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let view = view_rect(&window);
    let from = row_line(&frame, view, "p1");
    let to = page_boundary(&frame, view, &["p0", "p1", "p2"], 0);
    let _ = window.hold_drag(&mut transport, &mut queue, from, to);
    assert!(carry::carrying(&window.ctx), "the row is in flight");

    // A read lands while the block is held, and another entry now sits at `from`.
    window.library.apply(&Update::Tracks {
        playlist: PLAYLIST.to_owned(),
        tracks: ["p2", "p0", "p1"]
            .iter()
            .map(|title| TrackEntry {
                uri: format!("list:{title}"),
                title: (*title).to_owned(),
                ..track(0, None)
            })
            .collect(),
        cover: None,
        access: page_access(true),
    });

    let (command, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);
    assert_eq!(command, None, "the carried entry no longer holds the row");
    assert_eq!(
        page_rows(&window),
        ["list:p2", "list:p0", "list:p1"],
        "a refused reorder moves nothing"
    );
    assert!(window.library.movable(), "and writes nothing");
}

#[test]
fn the_marker_shows_where_the_row_lands_in_its_own_list() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1", "p2"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let view = view_rect(&window);
    let from = row_line(&frame, view, "p0");
    let to = page_boundary(&frame, view, &["p0", "p1", "p2"], 2);

    let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);
    let marker = marker(&frame).expect("the page draws where the row lands");
    assert!(
        view.contains_rect(marker),
        "the marker is the page's own, not the queue's"
    );
    let first = row_line(&frame, view, "p1");
    let second = row_line(&frame, view, "p2");
    assert!(
        marker.top() > first.y && marker.top() < second.y,
        "it sits in the gap the drop takes, between p1 and p2: {marker:?}"
    );
}

#[test]
fn a_read_only_playlist_row_still_queues_but_never_reorders() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    read_only_playlist_page(&mut window, &["p0", "p1", "p2"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let pane = pane_rect_when_shown(&window);
    let view = view_rect(&window);
    let from = row_line(&frame, view, "p1");
    let to = below(row_line(&frame, pane, "t3"), -0.25);

    assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
    assert_eq!(
        entries(&queue),
        ["t0", "t1", "t2", "list:p1", "t3"],
        "a drop into the queue is not a write to the page"
    );

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view, "p0");
    let to = row_half(&frame, view, "p2", false);
    assert_eq!(
        window.drag(&mut transport, &mut queue, from, to),
        None,
        "the page offers no reorder"
    );
    assert_eq!(
        page_rows(&window),
        ["list:p0", "list:p1", "list:p2"],
        "so its rows do not move"
    );
}

#[test]
fn a_page_without_access_never_reorders() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page_without_access(&mut window, &["p0", "p1", "p2"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let view = view_rect(&window);
    let from = row_line(&frame, view, "p0");
    let to = row_half(&frame, view, "p2", false);

    assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
    assert_eq!(page_rows(&window), ["list:p0", "list:p1", "list:p2"]);
}

#[test]
fn while_a_write_is_pending_no_row_lifts() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1", "p2"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let view = view_rect(&window);
    let from = row_line(&frame, view, "p0");
    let to = page_boundary(&frame, view, &["p0", "p1", "p2"], 2);
    assert!(
        window.drag(&mut transport, &mut queue, from, to).is_some(),
        "the first move is written"
    );
    assert!(!window.library.movable());

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view, "p2");
    let (_, _) = window.hold_drag(
        &mut transport,
        &mut queue,
        from,
        row_half(&frame, view, "p2", true),
    );
    assert!(
        carry::carried(&window.ctx).is_none(),
        "no row lifts while the revision it would write is already spent"
    );
}

#[test]
fn a_failed_write_puts_the_row_back_and_says_so() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1", "p2"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let view = view_rect(&window);
    let from = row_line(&frame, view, "p0");
    let to = page_boundary(&frame, view, &["p0", "p1", "p2"], 2);
    assert!(window.drag(&mut transport, &mut queue, from, to).is_some());
    assert_eq!(page_rows(&window), ["list:p1", "list:p0", "list:p2"]);

    window.library.apply(&Update::MoveFailed {
        playlist: PLAYLIST.to_owned(),
        failure: Failure::Save,
    });
    assert_eq!(
        page_rows(&window),
        ["list:p0", "list:p1", "list:p2"],
        "the row went back"
    );

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let notice = crate::signin::failure_message(Failure::Save);
    let at = painted_at(&frame, notice).expect("the failure is drawn");
    let first = painted_at(&frame, "p0").expect("the first row is drawn");
    assert!(at.y < first.y, "the failure heads the rows it put back");
}

#[test]
fn a_reply_lets_the_next_move_build_on_its_revision() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1", "p2"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let view = view_rect(&window);
    let from = row_line(&frame, view, "p0");
    let to = page_boundary(&frame, view, &["p0", "p1", "p2"], 2);
    assert!(window.drag(&mut transport, &mut queue, from, to).is_some());

    window.library.apply(&Update::TrackMoved {
        playlist: PLAYLIST.to_owned(),
        revision: revision("rev-2"),
    });
    assert!(window.library.movable(), "the reply ends the write");

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view, "p2");
    let to = page_boundary(&frame, view, &["p1", "p0", "p2"], 0);
    assert_eq!(
        window.drag(&mut transport, &mut queue, from, to),
        Some(Command::MoveTrack {
            playlist: PLAYLIST.to_owned(),
            revision: revision("rev-2"),
            moves: vec![Move {
                from: 2,
                length: 1,
                to: 0,
            }],
        }),
        "the second move is based on the reply's revision"
    );
    assert_eq!(page_rows(&window), ["list:p2", "list:p1", "list:p0"]);
}

#[test]
fn the_selection_after_a_move_is_the_landed_index_holding_the_moved_track() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1", "p2"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let view = view_rect(&window);
    let from = row_line(&frame, view, "p0");
    let to = page_boundary(&frame, view, &["p0", "p1", "p2"], 2);
    assert!(window.drag(&mut transport, &mut queue, from, to).is_some());

    let id = egui::Id::unique(("playlist-tracks", PLAYLIST));
    let list = carry::List::open(&window.ctx, id, 3);
    assert!(list.look(1).selected, "the row that landed is picked");
    assert!(
        !list.look(0).selected && !list.look(2).selected,
        "and nothing else is"
    );
    assert_eq!(
        page_rows(&window)[1],
        "list:p0",
        "the pick is on the track that moved"
    );
}

#[test]
fn dragging_a_playlist_below_the_next_moves_it_and_sends_one_write() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_playlists(
        &mut window,
        &[("A", None), ("B", None), ("C", None)],
        Some(rootlist_access(true)),
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, sidebar_rect(&window), "A");
    let to = sidebar_boundary(&window, &frame, &["A", "B", "C"], "Playlist", 2);

    assert_eq!(
        window.drag(&mut transport, &mut queue, from, to),
        Some(Command::MovePlaylist {
            revision: revision("root-1"),
            from: 0,
            to: 2,
        }),
        "the boundary is the gap, which is what the wire reads"
    );
    assert_eq!(sidebar_order(&window), ["B", "A", "C"]);
    assert!(!window.library.rootlist_movable(), "the write is in flight");
    assert_eq!(
        window.frame(&mut transport, &mut queue, Vec::new()).0,
        None,
        "the gesture sends one write, not one a frame"
    );
}

#[test]
fn a_query_that_matches_every_row_still_reorders() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_playlists(
        &mut window,
        &[("Midnight City", None), ("Midnight Sun", None)],
        Some(rootlist_access(true)),
    );
    // The query matches every row, so the drawn set is the whole list: its
    // slots are true indices and the lift must not be withheld.
    window.library.rail_filter.edit().push_str("midnight");

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, sidebar_rect(&window), "Midnight City");
    let to = sidebar_boundary(
        &window,
        &frame,
        &["Midnight City", "Midnight Sun"],
        "Playlist",
        2,
    );

    let (_, held) = window.hold_drag(&mut transport, &mut queue, from, to);
    assert!(
        carry::carried(&window.ctx).is_some(),
        "a query that matches every row still lets the row lift"
    );
    assert!(
        marker(&held).is_some(),
        "a reorder that can land draws its rule"
    );

    let (command, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);

    assert_eq!(
        command,
        Some(Command::MovePlaylist {
            revision: revision("root-1"),
            from: 0,
            to: 2,
        }),
        "the whole-list query blocked the reorder"
    );
    assert_eq!(
        sidebar_order(&window),
        ["Midnight Sun", "Midnight City"],
        "the playlist did not move"
    );
}

#[test]
fn dragging_a_playlist_to_the_top_writes_gap_zero() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_playlists(
        &mut window,
        &[("A", None), ("B", None), ("C", None)],
        Some(rootlist_access(true)),
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, sidebar_rect(&window), "C");
    let to = sidebar_boundary(&window, &frame, &["A", "B", "C"], "Playlist", 0);

    assert_eq!(
        window.drag(&mut transport, &mut queue, from, to),
        Some(Command::MovePlaylist {
            revision: revision("root-1"),
            from: 2,
            to: 0,
        })
    );
    assert_eq!(sidebar_order(&window), ["C", "A", "B"]);
}

#[test]
fn dragging_a_playlist_to_the_very_end_writes_the_length() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_playlists(
        &mut window,
        &[("A", None), ("B", None), ("C", None)],
        Some(rootlist_access(true)),
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, sidebar_rect(&window), "A");
    let to = sidebar_boundary(&window, &frame, &["A", "B", "C"], "Playlist", 3);

    assert_eq!(
        window.drag(&mut transport, &mut queue, from, to),
        Some(Command::MovePlaylist {
            revision: revision("root-1"),
            from: 0,
            to: 3,
        })
    );
    assert_eq!(sidebar_order(&window), ["B", "C", "A"]);
}

#[test]
fn a_sidebar_drop_beside_itself_moves_nothing_and_writes_nothing() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_playlists(
        &mut window,
        &[("A", None), ("B", None), ("C", None)],
        Some(rootlist_access(true)),
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    for boundary in [1, 2] {
        let from = row_line(&frame, sidebar_rect(&window), "B");
        let to = sidebar_boundary(&window, &frame, &["A", "B", "C"], "Playlist", boundary);
        assert_eq!(
            window.drag(&mut transport, &mut queue, from, to),
            None,
            "the gap {boundary} is where B already is"
        );
        assert_eq!(sidebar_order(&window), ["A", "B", "C"], "nothing moved");
        assert!(window.library.rootlist_movable(), "and nothing was written");
    }
}

#[test]
fn a_failed_rootlist_write_puts_the_row_back_and_says_so() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_playlists(
        &mut window,
        &[("A", None), ("B", None), ("C", None)],
        Some(rootlist_access(true)),
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, sidebar_rect(&window), "A");
    let to = sidebar_boundary(&window, &frame, &["A", "B", "C"], "Playlist", 2);
    assert!(window.drag(&mut transport, &mut queue, from, to).is_some());
    assert_eq!(sidebar_order(&window), ["B", "A", "C"]);

    window.library.apply(&Update::PlaylistMoveFailed);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

    assert_eq!(sidebar_order(&window), ["A", "B", "C"]);
    assert!(
        text_in(
            &frame,
            "Could not move that playlist. Try again.",
            sidebar_rect(&window)
        )
        .is_some(),
        "the list says what could not be done"
    );
    assert!(
        window.library.rootlist_movable(),
        "another drag may try again"
    );
}

#[test]
fn a_rootlist_reply_lets_the_next_move_build_on_its_revision() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_playlists(
        &mut window,
        &[("A", None), ("B", None), ("C", None)],
        Some(rootlist_access(true)),
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, sidebar_rect(&window), "A");
    let to = sidebar_boundary(&window, &frame, &["A", "B", "C"], "Playlist", 2);
    assert!(window.drag(&mut transport, &mut queue, from, to).is_some());
    window.library.apply(&Update::PlaylistMoved {
        revision: revision("root-2"),
    });

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, sidebar_rect(&window), "B");
    let to = sidebar_boundary(&window, &frame, &["B", "A", "C"], "Playlist", 3);

    assert_eq!(
        window.drag(&mut transport, &mut queue, from, to),
        Some(Command::MovePlaylist {
            revision: revision("root-2"),
            from: 0,
            to: 3,
        }),
        "the next move names the reply's revision"
    );
}

#[test]
fn a_playlist_carried_between_two_rows_draws_the_rule_and_no_fill() {
    let tokens = Tokens::DARK;
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_playlists(
        &mut window,
        &[("A", None), ("B", None), ("C", None)],
        Some(rootlist_access(true)),
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, sidebar_rect(&window), "A");
    let to = sidebar_boundary(&window, &frame, &["A", "B", "C"], "Playlist", 2);

    let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);

    assert!(marker(&frame).is_some(), "a reorder draws the rule");
    assert!(
        !frame
            .shapes
            .iter()
            .any(|clipped| matches!(&clipped.shape, egui::Shape::Rect(shape)
                if shape.fill == tokens.palette.row_selected
                    && sidebar_rect(&window).contains_rect(shape.rect))),
        "a reorder draws no fill"
    );
}

#[test]
fn a_playlist_carried_over_the_queue_is_never_queued() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_playlists(
        &mut window,
        &[("A", None), ("B", None)],
        Some(rootlist_access(true)),
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, sidebar_rect(&window), "A");
    let to = pane_rect_when_shown(&window).center();
    let before = entries(&queue);

    assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);

    assert_eq!(entries(&queue), before, "a playlist is no queue entry");
    assert_eq!(sidebar_order(&window), ["A", "B"]);
    assert!(window.library.rootlist_movable(), "and nothing was written");
}

#[test]
fn a_playlist_carried_over_the_queue_button_is_refused() {
    // The queue column is hidden below this width, so the playbar's button
    // is the only queue target there is.
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1100.0, 900.0));
    sidebar_playlists(
        &mut window,
        &[("A", None), ("B", None)],
        Some(rootlist_access(true)),
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, sidebar_rect(&window), "A");
    let to = queue_button_at(&window).center();
    let before = entries(&queue);

    assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);

    assert_eq!(entries(&queue), before);
    assert!(window.library.rootlist_movable());
}

#[test]
fn a_drop_while_a_write_is_pending_is_refused() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_playlists(
        &mut window,
        &[("A", None), ("B", None), ("C", None)],
        Some(rootlist_access(true)),
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, sidebar_rect(&window), "A");
    let to = sidebar_boundary(&window, &frame, &["A", "B", "C"], "Playlist", 2);
    assert!(window.drag(&mut transport, &mut queue, from, to).is_some());

    // A second drop would name a revision the first is already replacing.
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, sidebar_rect(&window), "B");
    let to = sidebar_boundary(&window, &frame, &["B", "A", "C"], "Playlist", 3);

    assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
    assert_eq!(sidebar_order(&window), ["B", "A", "C"]);
}

#[test]
fn a_queue_drop_still_works_while_a_sidebar_write_is_pending() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(5, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1", "p2"]);
    sidebar_playlists(
        &mut window,
        &[("A", None), ("B", None), ("C", None)],
        Some(rootlist_access(true)),
    );
    assert!(
        window.library.move_playlist(0, 2, "playlist:A").is_some(),
        "the sidebar write is in flight"
    );

    // A queue insert names no revision, so the gate leaves it alone.
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let pane = pane_rect_when_shown(&window);
    let from = row_line(&frame, view_rect(&window), "p1");
    let to = below(row_line(&frame, pane, "t3"), -0.25);
    assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
    assert_eq!(entries(&queue), ["t0", "t1", "t2", "list:p1", "t3", "t4"]);

    // A page move is the other write, and it waits.
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let view = view_rect(&window);
    let from = row_line(&frame, view, "p0");
    let to = page_boundary(&frame, view, &["p0", "p1", "p2"], 2);
    assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
    assert_eq!(page_rows(&window), ["list:p0", "list:p1", "list:p2"]);
}

#[test]
fn a_sidebar_drop_on_a_frame_another_region_took_moves_nothing() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1", "p2"]);
    // A page ahead of the one the drop returns to, so Back has a load to ask
    // for and takes the frame.
    let _ = window.library.select("spotify:playlist:other");
    sidebar_playlists(
        &mut window,
        &[("A", None), ("B", None), ("C", None)],
        Some(rootlist_access(true)),
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, sidebar_rect(&window), "A");
    let to = sidebar_boundary(&window, &frame, &["A", "B", "C"], "Playlist", 2);
    let _ = window.hold_drag(&mut transport, &mut queue, from, to);

    let (command, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![window.release(to), key(egui::Key::BrowserBack, true)],
    );

    assert!(
        matches!(command, Some(Command::LoadTracks { .. })),
        "another region took the frame: {command:?}"
    );
    assert_eq!(sidebar_order(&window), ["A", "B", "C"]);
    assert!(window.library.rootlist_movable(), "and writes nothing");
}

/// The page a read that named no state leaves: nothing may be written.
fn playlist_page_without_access(window: &mut Window, titles: &[&str]) {
    playlist_page_with(window, titles, None);
}

/// A point inside the row painted as `title` in `area`, in the half that
/// decides the boundary it takes: `after` is the gap below it.
fn row_half(output: &egui::FullOutput, area: Rect, title: &str, after: bool) -> egui::Pos2 {
    let line = row_line(output, area, title);
    egui::pos2(line.x, line.y + if after { 20.0 } else { -4.0 })
}
