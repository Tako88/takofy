use super::super::*;
use super::test_support::*;
use takofy_model::Move;

#[test]
fn a_page_move_reply_updates_the_entrys_revision() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
    library.apply(&Update::Playlists(vec![with_access(
        playlist("a", "a"),
        0x01,
        true,
        Some(2),
    )]));
    library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

    library.apply(&Update::TrackMoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });

    library.open(Page::Album("spotify:album:b".to_owned()));
    assert_eq!(
        library.add_tracks(0, &pick(&["spotify:track:x"])),
        Some(Command::AddTracks {
            playlist: "a".to_owned(),
            revision: revision(0x02),
            from_index: 2,
            tracks: pick(&["spotify:track:x"]),
        }),
        "the add names the revision the move's reply named"
    );
}

#[test]
fn a_move_applies_at_once_and_asks_for_one_write_on_the_held_revision() {
    let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));

    let found = move_one(&mut library, "a", 0, 2, &track("t0"));

    assert_eq!(
        found,
        Some((
            Command::MoveTrack {
                playlist: "a".to_owned(),
                revision: revision(0x01),
                moves: vec![Move {
                    from: 0,
                    length: 1,
                    to: 2,
                }],
            },
            1,
        ))
    );
    assert_eq!(
        library.tracks,
        ready("a", &["t1", "t0", "t2"], access(0x01, true)),
        "the dragged order is on screen before the write is answered"
    );
    assert_eq!(
        library.pending,
        Some(crate::reorder::Pending {
            playlist: "a".to_owned(),
            before: moved(&["t0", "t1", "t2"]),
            after: moved(&["t1", "t0", "t2"]),
            query: None,
        })
    );
}

#[test]
fn a_move_selects_nothing_new_and_returns_where_the_row_landed() {
    let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
    let page = library.page().cloned();

    let found = move_one(&mut library, "a", 2, 0, &track("t2"));

    assert_eq!(found.map(|(_, landed)| landed), Some(0));
    assert_eq!(library.page(), page.as_ref());
    assert_eq!(library.selected(), Some("a"));
}

#[test]
fn a_read_only_page_refuses_a_move() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, false));

    assert!(!library.movable());
    assert_eq!(move_one(&mut library, "a", 0, 2, &track("t0")), None);
    assert_eq!(library.pending, None);
    assert_eq!(
        library.tracks,
        ready("a", &["t0", "t1"], access(0x01, false))
    );
}

#[test]
fn a_page_with_no_access_refuses_a_move() {
    let mut library = page_with("a", &["t0", "t1"], None);

    assert!(!library.movable());
    assert_eq!(move_one(&mut library, "a", 0, 2, &track("t0")), None);
    assert_eq!(library.pending, None);
}

#[test]
fn a_second_move_waits_for_the_first() {
    let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

    assert_eq!(
        move_one(&mut library, "a", 0, 2, &track("t1")),
        None,
        "one gesture is one write, so a second waits for the reply"
    );
    assert!(!library.movable());
}

#[test]
fn a_move_whose_row_changed_under_the_drag_is_refused() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));

    assert_eq!(move_one(&mut library, "a", 0, 2, &track("gone")), None);
    assert_eq!(
        move_one(&mut library, "a", 9, 2, &track("t0")),
        None,
        "a row the list does not hold"
    );
    assert_eq!(library.pending, None);
}

#[test]
fn a_block_move_applies_at_once_and_asks_for_one_write() {
    let mut library = page_with("a", &["t0", "t1", "t2", "t3"], access(0x01, true));

    let found = library.move_tracks("a", &[0, 2], 4, &moved(&["t0", "t2"]));

    assert_eq!(
        found,
        Some((
            Command::MoveTrack {
                playlist: "a".to_owned(),
                revision: revision(0x01),
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
            },
            2..4,
        )),
        "one command carries the whole chain"
    );
    assert_eq!(
        library.tracks,
        ready("a", &["t1", "t3", "t0", "t2"], access(0x01, true)),
        "the gathered block is on screen before the write is answered"
    );
    assert_eq!(
        library.pending,
        Some(crate::reorder::Pending {
            playlist: "a".to_owned(),
            before: moved(&["t0", "t1", "t2", "t3"]),
            after: moved(&["t1", "t3", "t0", "t2"]),
            query: None,
        })
    );
}

#[test]
fn a_block_whose_rows_changed_under_the_drag_is_refused() {
    let mut library = page_with("a", &["t0", "t1", "t2", "t3"], access(0x01, true));

    assert_eq!(
        library.move_tracks("a", &[0, 2], 4, &moved(&["t0", "gone"])),
        None,
        "a row is no longer the one the drag carried"
    );
    assert_eq!(
        library.move_tracks("a", &[0, 9], 4, &moved(&["t0", "gone"])),
        None,
        "a row the list does not hold"
    );
    assert_eq!(
        library.move_tracks("a", &[0, 2], 4, &moved(&["t0"])),
        None,
        "the entries no longer answer for the rows"
    );
    assert_eq!(library.pending, None);
}

#[test]
fn a_block_that_is_not_ascending_is_refused() {
    let mut library = page_with("a", &["t0", "t1", "t2", "t3"], access(0x01, true));

    assert_eq!(
        library.move_tracks("a", &[2, 0], 4, &moved(&["t2", "t0"])),
        None
    );
    assert_eq!(
        library.move_tracks("a", &[0, 0], 4, &moved(&["t0", "t0"])),
        None,
        "one row is picked once"
    );
    assert_eq!(
        library.move_tracks("a", &[], 4, &[]),
        None,
        "nothing carried is nothing to move"
    );
    assert_eq!(library.pending, None);
}

#[test]
fn a_read_only_page_refuses_a_block() {
    let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, false));

    assert_eq!(
        library.move_tracks("a", &[0, 2], 3, &moved(&["t0", "t2"])),
        None
    );
    assert_eq!(library.pending, None);
    assert_eq!(
        library.tracks,
        ready("a", &["t0", "t1", "t2"], access(0x01, false))
    );
}

#[test]
fn a_block_that_would_not_move_writes_nothing() {
    let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));

    for boundary in 0..=2 {
        assert_eq!(
            library.move_tracks("a", &[0, 1], boundary, &moved(&["t0", "t1"])),
            None,
            "a contiguous block dropped on or inside itself at {boundary}"
        );
    }
    assert_eq!(library.pending, None);
    assert_eq!(
        library.tracks,
        ready("a", &["t0", "t1", "t2"], access(0x01, true))
    );
}

#[test]
fn a_second_block_waits_for_the_first() {
    let mut library = page_with("a", &["t0", "t1", "t2", "t3"], access(0x01, true));
    assert!(
        library
            .move_tracks("a", &[0, 2], 4, &moved(&["t0", "t2"]))
            .is_some()
    );

    assert_eq!(
        library.move_tracks("a", &[0, 1], 4, &moved(&["t1", "t3"])),
        None,
        "one gesture is one write, so a second block waits for the reply"
    );
    assert!(!library.movable());
}

#[test]
fn a_failed_block_restores_the_order_and_says_so() {
    let mut library = page_with("a", &["t0", "t1", "t2", "t3"], access(0x01, true));
    assert!(
        library
            .move_tracks("a", &[0, 2], 4, &moved(&["t0", "t2"]))
            .is_some()
    );

    library.apply(&Update::MoveFailed {
        playlist: "a".to_owned(),
        failure: Failure::Save,
    });

    assert_eq!(library.pending, None);
    assert_eq!(
        library.tracks,
        Load::Ready(TracksFor {
            playlist: "a".to_owned(),
            tracks: moved(&["t0", "t1", "t2", "t3"]),
            access: access(0x01, true),
            notice: Some(Failure::Save),
        })
    );
}

#[test]
fn a_move_for_a_playlist_not_on_screen_is_refused() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));

    assert_eq!(move_one(&mut library, "b", 0, 2, &track("t0")), None);

    library.to_playlists();
    assert_eq!(
        move_one(&mut library, "a", 0, 2, &track("t0")),
        None,
        "the ready rows are kept for Back, not for a drop"
    );
    assert_eq!(library.pending, None);
}

#[test]
fn a_drop_beside_itself_sends_nothing() {
    let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));

    assert_eq!(move_one(&mut library, "a", 1, 1, &track("t1")), None);
    assert_eq!(move_one(&mut library, "a", 1, 2, &track("t1")), None);
    assert_eq!(library.pending, None);
    assert_eq!(
        library.tracks,
        ready("a", &["t0", "t1", "t2"], access(0x01, true))
    );
}

#[test]
fn the_reply_revision_replaces_the_held_one_and_ends_the_write() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

    library.apply(&Update::TrackMoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });

    assert_eq!(library.pending, None);
    assert_eq!(
        library.tracks,
        ready("a", &["t1", "t0"], access(0x02, true))
    );
}

#[test]
fn the_next_move_is_based_on_the_reply_revision() {
    let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
    library.apply(&Update::TrackMoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });

    let found = move_one(&mut library, "a", 2, 0, &track("t2"));

    assert_eq!(
        found.map(|(command, _)| command),
        Some(Command::MoveTrack {
            playlist: "a".to_owned(),
            revision: revision(0x02),
            moves: vec![Move {
                from: 2,
                length: 1,
                to: 0,
            }],
        })
    );
}

#[test]
fn a_read_while_a_write_is_pending_is_not_adopted_even_with_an_equal_revision() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: moved(&["t0", "t1"]),
        cover: None,
        access: access(0x01, true),
    });

    assert_eq!(
        library.tracks,
        ready("a", &["t1", "t0"], access(0x01, true)),
        "the server echoes the old order under the new revision"
    );
    assert!(library.pending.is_some(), "the write is still in flight");
}

#[test]
fn a_read_while_a_write_is_pending_keeps_the_held_revision() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: moved(&["t0", "t1"]),
        cover: None,
        access: access(0x02, true),
    });

    assert_eq!(
        library.tracks,
        ready("a", &["t1", "t0"], access(0x01, true)),
        "the held access is the one the write was sent on"
    );
}

#[test]
fn a_read_onto_a_loading_page_while_a_write_is_pending_draws_the_moved_order() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
    library.select("a");
    assert_eq!(library.tracks, Load::Loading);

    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: moved(&["t0", "t1"]),
        cover: None,
        access: access(0x01, true),
    });

    assert_eq!(
        library.tracks,
        ready("a", &["t1", "t0"], access(0x01, true)),
        "the reload does not undo the drag it was asked for before"
    );
}

#[test]
fn a_read_with_nothing_pending_is_adopted_even_with_an_equal_revision() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));

    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: moved(&["t1", "t0"]),
        cover: None,
        access: access(0x01, true),
    });

    assert_eq!(
        library.tracks,
        ready("a", &["t1", "t0"], access(0x01, true))
    );
}

#[test]
fn a_read_after_the_reply_is_adopted() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
    library.apply(&Update::TrackMoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });

    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: moved(&["t0", "t1"]),
        cover: None,
        access: access(0x02, true),
    });

    assert_eq!(
        library.tracks,
        ready("a", &["t0", "t1"], access(0x02, true)),
        "nothing is pending, so every read is adopted"
    );
}

#[test]
fn a_failed_write_reverts_to_the_order_before_it_and_says_save() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

    library.apply(&Update::MoveFailed {
        playlist: "a".to_owned(),
        failure: Failure::Save,
    });

    assert_eq!(library.pending, None);
    assert_eq!(
        library.tracks,
        Load::Ready(TracksFor {
            playlist: "a".to_owned(),
            tracks: moved(&["t0", "t1"]),
            access: access(0x01, true),
            notice: Some(Failure::Save),
        })
    );
}

#[test]
fn a_failed_write_for_a_page_no_longer_shown_only_ends_the_write() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
    library.to_playlists();

    library.apply(&Update::MoveFailed {
        playlist: "a".to_owned(),
        failure: Failure::Save,
    });

    assert_eq!(library.pending, None);
    assert_eq!(
        library.tracks,
        Load::Ready(TracksFor {
            playlist: "a".to_owned(),
            tracks: moved(&["t1", "t0"]),
            access: access(0x01, true),
            notice: None,
        }),
        "nothing is on screen to put back, and Back reloads the list"
    );
}

#[test]
fn an_adopted_read_clears_the_notice() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
    library.apply(&Update::MoveFailed {
        playlist: "a".to_owned(),
        failure: Failure::Save,
    });

    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: moved(&["t0", "t1"]),
        cover: None,
        access: access(0x01, true),
    });

    assert_eq!(
        library.tracks,
        ready("a", &["t0", "t1"], access(0x01, true))
    );
}

#[test]
fn a_new_gesture_clears_the_notice_at_once() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
    library.apply(&Update::MoveFailed {
        playlist: "a".to_owned(),
        failure: Failure::Save,
    });

    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

    assert_eq!(
        library.tracks,
        ready("a", &["t1", "t0"], access(0x01, true)),
        "the sentence goes when the user tries again, not when the reply lands"
    );
}

#[test]
fn a_successful_write_after_a_failed_one_clears_the_notice() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
    library.apply(&Update::MoveFailed {
        playlist: "a".to_owned(),
        failure: Failure::Save,
    });
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

    library.apply(&Update::TrackMoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });

    assert_eq!(
        library.tracks,
        ready("a", &["t1", "t0"], access(0x02, true)),
        "a saved order must not still claim it could not be saved"
    );
}

#[test]
fn a_reply_for_another_playlist_does_not_touch_the_page() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

    library.apply(&Update::TrackMoved {
        playlist: "b".to_owned(),
        revision: revision(0x09),
    });
    library.apply(&Update::MoveFailed {
        playlist: "b".to_owned(),
        failure: Failure::Save,
    });

    assert!(
        library.pending.is_some(),
        "the write for a is still in flight"
    );
    assert_eq!(
        library.tracks,
        ready("a", &["t1", "t0"], access(0x01, true))
    );
}

#[test]
fn a_reply_while_the_page_is_loading_only_ends_the_write() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
    library.select("a");

    library.apply(&Update::TrackMoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });

    assert_eq!(library.pending, None);
    assert_eq!(library.tracks, Load::Loading);
}

#[test]
fn a_reply_while_the_page_is_off_screen_only_ends_the_write() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
    library.to_playlists();

    library.apply(&Update::TrackMoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });

    assert_eq!(library.pending, None);
    assert_eq!(
        library.tracks,
        ready("a", &["t1", "t0"], access(0x02, true)),
        "the rows are kept for Back, which reloads them anyway"
    );
}

#[test]
fn a_failed_write_while_the_page_is_loading_only_ends_the_write() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
    library.select("a");

    library.apply(&Update::MoveFailed {
        playlist: "a".to_owned(),
        failure: Failure::Save,
    });

    assert_eq!(library.pending, None);
    assert_eq!(library.tracks, Load::Loading);
}

#[test]
fn a_tracks_failure_while_a_write_is_pending_keeps_the_write() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

    library.apply(&Update::TracksFailed {
        playlist: "a".to_owned(),
        failure: Failure::Load,
    });

    assert_eq!(library.tracks, Load::Failed(Failure::Load));
    assert!(
        library.pending.is_some(),
        "only the write's own reply ends it"
    );
}
