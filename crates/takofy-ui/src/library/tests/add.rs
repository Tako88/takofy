use super::super::*;
use super::test_support::*;

#[test]
fn an_add_asks_for_one_write_on_the_entrys_revision_and_length() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));

    assert_eq!(
        library.add_tracks(1, &pick(&["spotify:track:x"])),
        Some(Command::AddTracks {
            playlist: "b".to_owned(),
            revision: revision(0x07),
            from_index: 3,
            tracks: pick(&["spotify:track:x"]),
        })
    );
}

#[test]
fn an_add_names_the_pick_in_the_order_the_drag_took_it() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
    let tracks = pick(&["spotify:track:a", "spotify:track:b", "spotify:track:c"]);

    assert_eq!(
        library.add_tracks(1, &tracks),
        Some(Command::AddTracks {
            playlist: "b".to_owned(),
            revision: revision(0x07),
            from_index: 3,
            tracks: tracks.clone(),
        })
    );
}

#[test]
fn an_add_of_three_counts_three() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
    let tracks = pick(&["spotify:track:a", "spotify:track:b", "spotify:track:c"]);
    assert!(library.add_tracks(1, &tracks).is_some());

    library.apply(&Update::TrackAdded {
        playlist: "b".to_owned(),
        revision: revision(0x08),
    });

    let entry = sidebar_entry(&library, "b").expect("still listed");
    assert_eq!(entry.tracks, Some(6), "three travelled, three were held");
}

#[test]
fn a_failed_add_of_three_leaves_the_count_alone() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
    let tracks = pick(&["spotify:track:a", "spotify:track:b", "spotify:track:c"]);
    assert!(library.add_tracks(1, &tracks).is_some());

    library.apply(&Update::AddFailed {
        playlist: "b".to_owned(),
    });

    let entry = sidebar_entry(&library, "b").expect("still listed");
    assert_eq!(entry.tracks, Some(3));
    assert_eq!(library.sidebar_notice, Some(SidebarWrite::Add));
    assert!(library.addable(1), "the refusal is spent with the gesture");
}

#[test]
fn an_add_changes_nothing_until_it_is_confirmed() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
    let before = sidebar_order(&library);

    assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());

    assert_eq!(sidebar_order(&library), before);
    assert_eq!(
        sidebar_entry(&library, "b").and_then(|entry| entry.tracks),
        Some(3)
    );
    assert!(!library.addable(0), "the write is in flight");
}

#[test]
fn a_confirmed_add_replaces_the_revision_and_counts_one_more() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
    assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());

    library.apply(&Update::TrackAdded {
        playlist: "b".to_owned(),
        revision: revision(0x08),
    });

    let entry = sidebar_entry(&library, "b").expect("still listed");
    assert_eq!(entry.tracks, Some(4));
    assert_eq!(
        entry.access.map(|access| access.revision),
        Some(revision(0x08))
    );
    assert_eq!(library.sidebar_notice, None);
    assert!(library.addable(1), "the write is answered");
}

#[test]
fn a_confirmed_add_drops_the_page_held_for_that_playlist() {
    // The add is made from an album page, so the playlist's own rows were
    // read before it and are still held behind it.
    let mut library = page_with("b", &["t0", "t1"], access(0x01, true));
    library.playlists = Load::Ready(vec![
        with_access(playlist("a", "a"), 0x07, true, Some(1)),
        with_access(playlist("b", "b"), 0x07, true, Some(2)),
    ]);
    library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));
    library.open(album("spotify:album:x"));
    assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());

    library.apply(&Update::TrackAdded {
        playlist: "b".to_owned(),
        revision: revision(0x08),
    });

    assert_eq!(library.tracks, Load::Idle, "the page is one track short");
    assert_eq!(
        library.back(),
        Some(Command::LoadTracks {
            playlist: "b".to_owned()
        }),
        "Back asks for the page again"
    );
}

#[test]
fn a_confirmed_add_keeps_the_page_held_for_another_playlist() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
    library.playlists = Load::Ready(vec![
        with_access(playlist("a", "a"), 0x07, true, Some(1)),
        with_access(playlist("b", "b"), 0x07, true, Some(2)),
    ]);
    library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));
    library.open(album("spotify:album:x"));
    assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());

    library.apply(&Update::TrackAdded {
        playlist: "b".to_owned(),
        revision: revision(0x08),
    });

    assert!(
        matches!(&library.tracks, Load::Ready(tracks) if tracks.playlist == "a"),
        "the add names another list"
    );
    assert_eq!(library.back(), None, "and that page is still ready");
}

#[test]
fn the_next_add_builds_on_the_reply_and_the_new_count() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
    assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());
    library.apply(&Update::TrackAdded {
        playlist: "b".to_owned(),
        revision: revision(0x08),
    });

    assert_eq!(
        library.add_tracks(1, &pick(&["spotify:track:y"])),
        Some(Command::AddTracks {
            playlist: "b".to_owned(),
            revision: revision(0x08),
            from_index: 4,
            tracks: pick(&["spotify:track:y"]),
        })
    );
}

#[test]
fn a_failed_add_leaves_the_count_and_the_revision_and_says_so() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
    assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());

    library.apply(&Update::AddFailed {
        playlist: "b".to_owned(),
    });

    let entry = sidebar_entry(&library, "b").expect("still listed");
    assert_eq!(entry.tracks, Some(3));
    assert_eq!(
        entry.access.map(|access| access.revision),
        Some(revision(0x07))
    );
    assert_eq!(library.sidebar_notice, Some(SidebarWrite::Add));
    assert!(library.addable(1));
}

#[test]
fn a_list_that_can_not_be_edited_is_not_addable() {
    let library = sidebar_library(&["a", "b"], 0x07, false, Some(3));

    assert!(!library.addable(0));
    let mut library = library;
    assert_eq!(library.add_tracks(0, &pick(&["spotify:track:x"])), None);
}

#[test]
fn a_playlist_with_no_revision_or_no_count_is_not_addable() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![
        with_access(playlist("a", "a"), 0x07, true, None),
        playlist("b", "b"),
    ]));
    library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));

    assert!(!library.addable(0), "no count");
    assert!(!library.addable(1), "no revision");
    assert_eq!(library.add_tracks(0, &pick(&["spotify:track:x"])), None);
}

#[test]
fn the_open_playlist_is_not_addable() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
    library.select("a");
    assert!(!library.addable(0));
    assert!(library.addable(1));

    library.open(Page::Album("spotify:album:b".to_owned()));
    assert!(
        library.addable(0),
        "an album page leaves every playlist a target"
    );

    library.open(Page::Search("never gonna".to_owned()));
    assert!(library.addable(0));
}

#[test]
fn an_empty_pick_is_not_a_write() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));

    assert_eq!(library.add_tracks(0, &[]), None);
    assert!(library.addable(0), "a refused pick is no write at all");
}

#[test]
fn an_add_waits_while_any_write_is_pending() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
    library.playlists = Load::Ready(vec![
        with_access(playlist("p0", "p0"), 0x07, true, Some(1)),
        with_access(playlist("p1", "p1"), 0x07, true, Some(1)),
    ]);
    library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

    assert_eq!(library.add_tracks(1, &pick(&["spotify:track:x"])), None);

    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(1));
    assert!(library.move_playlist(0, 2, "a").is_some());
    assert_eq!(library.add_tracks(1, &pick(&["spotify:track:x"])), None);
}

#[test]
fn the_add_targets_are_the_sidebars_order_without_the_open_page() {
    let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, Some(2));
    library.select("b");

    let targets = library.add_targets();

    assert_eq!(
        targets
            .iter()
            .map(|row| row.uri.as_str())
            .collect::<Vec<_>>(),
        ["a", "c"],
        "the open page is no target, and the order is the sidebar's"
    );
    assert_eq!(targets[0].name, "a");
    assert_eq!(targets[0].revision, revision(0x07));
    assert_eq!(targets[0].length, 2);
}

#[test]
fn an_add_target_needs_an_editable_list_with_a_revision_and_a_count() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![
        with_access(playlist("a", "a"), 0x07, true, Some(2)),
        with_access(playlist("b", "b"), 0x07, false, Some(2)),
        with_access(playlist("c", "c"), 0x07, true, None),
        playlist("d", "d"),
    ]));
    library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));

    assert_eq!(
        library
            .add_targets()
            .iter()
            .map(|row| row.uri.as_str())
            .collect::<Vec<_>>(),
        ["a"],
        "a foreign list is absent, never greyed"
    );
}

#[test]
fn the_add_targets_are_empty_while_a_write_is_pending() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(2));
    assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());
    assert!(library.add_targets().is_empty(), "an add is in flight");

    let mut creating = sidebar_library(&["a", "b"], 0x07, true, Some(2));
    creating.naming.open();
    creating.naming.edit().push_str("Mix");
    assert!(creating.creatable());
    assert!(creating.create_playlist().is_some());
    assert!(creating.add_targets().is_empty(), "a create is in flight");
}

#[test]
fn an_add_pick_names_the_held_list() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
    let tracks = pick(&["spotify:track:x", "spotify:track:y"]);

    assert_eq!(
        library.add_pick("b", &tracks),
        Some(Command::AddTracks {
            playlist: "b".to_owned(),
            revision: revision(0x07),
            from_index: 3,
            tracks: tracks.clone(),
        })
    );
}

#[test]
fn an_add_pick_refuses_an_unknown_uri_the_open_page_and_a_pending_write() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
    let tracks = pick(&["spotify:track:x"]);

    assert_eq!(library.add_pick("missing", &tracks), None);
    assert!(library.addable(1), "an unknown URI is no write at all");

    library.select("b");
    assert_eq!(library.add_pick("b", &tracks), None, "the open page");
    library.open(Page::Search("query".to_owned()));

    assert!(library.add_pick("b", &tracks).is_some());
    assert_eq!(library.add_pick("a", &tracks), None, "a write is pending");
}

#[test]
fn a_stale_reply_with_nothing_pending_is_ignored() {
    let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, Some(2));
    library.apply(&Update::PlaylistMoved {
        revision: revision(0x08),
    });
    library.apply(&Update::PlaylistMoveFailed);
    library.apply(&Update::TrackAdded {
        playlist: "a".to_owned(),
        revision: revision(0x08),
    });
    library.apply(&Update::AddFailed {
        playlist: "a".to_owned(),
    });

    assert_eq!(sidebar_order(&library), names(&["a", "b", "c"]));
    assert_eq!(
        sidebar_entry(&library, "a").and_then(|entry| entry.tracks),
        Some(2)
    );
    assert_eq!(library.sidebar_notice, None);
    assert!(library.rootlist_movable());
}

#[test]
fn a_reply_for_another_playlist_does_not_end_the_write() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(2));
    assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());

    library.apply(&Update::TrackAdded {
        playlist: "a".to_owned(),
        revision: revision(0x08),
    });
    library.apply(&Update::AddFailed {
        playlist: "a".to_owned(),
    });

    assert!(!library.addable(0), "the write for b is still in flight");
}

#[test]
fn an_access_that_arrives_mid_add_waits_for_a_fresh_read() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
    // A read that lands just before the add drops the held access with the
    // order it brought, so the column is only movable again once its own
    // access arrives.
    library.apply(&Update::Playlists(vec![
        with_access(playlist("a", "a"), 0x07, true, Some(3)),
        with_access(playlist("b", "b"), 0x07, true, Some(3)),
    ]));
    assert!(!library.rootlist_movable(), "no access yet");
    assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());

    library.apply(&Update::RootlistAccess(rootlist_access(0x09, true)));

    assert!(
        library.rootlist.is_none(),
        "an access adopted while a write is pending names a revision that write may replace"
    );
    assert_eq!(
        library.rootlist_request(),
        None,
        "a read asked for while the write it waits on is still in flight"
    );

    library.apply(&Update::TrackAdded {
        playlist: "b".to_owned(),
        revision: revision(0x08),
    });

    assert_eq!(
        library.rootlist_request(),
        Some(Command::ReloadLibrary),
        "the refused access asks for a read that can be adopted"
    );
    assert_eq!(library.rootlist_request(), None, "a second read");
}

#[test]
fn an_access_whose_read_was_refused_is_refused_with_it() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
    // A read that drops the held access waits for its own to arrive.
    library.apply(&Update::Playlists(vec![
        with_access(playlist("a", "a"), 0x07, true, Some(3)),
        with_access(playlist("b", "b"), 0x07, true, Some(3)),
    ]));
    assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());

    library.apply(&Update::Playlists(vec![
        with_access(playlist("b", "b"), 0x0a, true, Some(3)),
        with_access(playlist("a", "a"), 0x0a, true, Some(3)),
    ]));
    library.apply(&Update::RootlistAccess(rootlist_access(0x0a, true)));
    library.apply(&Update::TrackAdded {
        playlist: "b".to_owned(),
        revision: revision(0x08),
    });

    assert_eq!(
        sidebar_order(&library),
        names(&["a", "b"]),
        "the read's order was refused"
    );
    assert!(
        !library.rootlist_movable(),
        "and its revision is no use beside this one"
    );
}

#[test]
fn a_read_while_an_add_is_pending_is_not_adopted() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
    assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());

    library.apply(&Update::Playlists(vec![
        with_access(playlist("a", "a"), 0x07, true, Some(3)),
        with_access(playlist("b", "b"), 0x08, true, Some(4)),
    ]));

    assert_eq!(
        sidebar_entry(&library, "b").and_then(|entry| entry.tracks),
        Some(3),
        "a read taken after the server applied the add would count twice"
    );
    library.apply(&Update::TrackAdded {
        playlist: "b".to_owned(),
        revision: revision(0x09),
    });
    assert_eq!(
        sidebar_entry(&library, "b").and_then(|entry| entry.tracks),
        Some(4),
        "the confirmation counts from the count the drop named"
    );
}
