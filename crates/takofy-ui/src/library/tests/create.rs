use super::super::*;
use super::test_support::*;

#[test]
fn an_emptied_playlist_stays_ready_and_empty() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));

    assert!(
        library
            .remove_tracks("a", &[0, 1], &moved(&["t0", "t1"]))
            .is_some()
    );
    assert_eq!(library.tracks, ready("a", &[], access(0x01, true)));

    library.apply(&Update::TracksRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });
    assert_eq!(library.tracks, ready("a", &[], access(0x02, true)));
    assert_eq!(library.selected(), Some("a"));
    assert!(library.removable(), "an empty editable list still rewrites");
}

#[test]
fn creatable_needs_ready_playlists_a_rootlist_revision_and_no_write() {
    assert!(sidebar_library(&["a"], 0x07, true, None).creatable());

    for playlists in [Load::Idle, Load::Loading, Load::Failed(Failure::Load)] {
        let mut library = sidebar_library(&["a"], 0x07, true, None);
        library.playlists = playlists;
        assert!(
            !library.creatable(),
            "a list that is not ready to insert into"
        );
    }

    let mut no_rootlist = sidebar_library(&["a"], 0x07, true, None);
    no_rootlist.rootlist = None;
    assert!(!no_rootlist.creatable(), "no revision to link against");

    let mut writing = sidebar_library(&["a", "b"], 0x07, true, None);
    assert!(writing.move_playlist(0, 2, "a").is_some());
    assert!(!writing.creatable(), "a second write for one frame");
}

#[test]
fn creatable_does_not_need_editable() {
    // The walk skips a folder marker and leaves the access uneditable, which
    // says nothing about whether index 0 of the rootlist can take an add.
    let library = sidebar_library(&["a"], 0x07, false, None);
    assert!(!library.rootlist_movable());
    assert!(library.creatable());
}

#[test]
fn create_issues_one_command_naming_the_held_revision_and_the_trimmed_name() {
    let mut library = naming_library(&["a"], "  Night  Drive ");

    assert_eq!(
        library.create_playlist(),
        Some(Command::CreatePlaylist {
            name: "Night  Drive".to_owned(),
            revision: revision(0x07),
            tracks: Vec::new(),
        })
    );
    assert!(library.naming.writing());
    assert!(library.write_in_flight());
}

#[test]
fn create_is_refused_when_naming_is_closed_blank_or_already_writing() {
    let mut closed = sidebar_library(&["a"], 0x07, true, None);
    assert_eq!(closed.create_playlist(), None, "a closed field wrote");

    let mut blank = naming_library(&["a"], "   ");
    assert_eq!(blank.create_playlist(), None);
    assert!(blank.naming.is_open(), "a blank name closed the field");
    assert!(!blank.write_in_flight());

    let mut writing = naming_library(&["a"], "Mix");
    assert!(writing.create_playlist().is_some());
    assert_eq!(writing.create_playlist(), None, "a second write");
}

#[test]
fn create_waits_while_any_write_is_pending() {
    let mut library = naming_library(&["a", "b"], "Mix");
    assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());
    assert_eq!(library.create_playlist(), None, "an add was in flight");
    assert!(!library.naming.writing());
}

#[test]
fn a_create_names_the_pick_the_field_holds() {
    let mut library = naming_library(&["a"], "Mix");
    let tracks = pick(&["spotify:track:x", "spotify:track:y"]);
    library.naming.open_with(tracks.clone());

    assert_eq!(
        library.create_playlist(),
        Some(Command::CreatePlaylist {
            name: "Mix".to_owned(),
            revision: revision(0x07),
            tracks,
        })
    );
}

#[test]
fn a_created_list_that_took_the_pick_counts_it() {
    let mut library = naming_library(&["a"], "Mix");
    library.naming.open_with(pick(&[
        "spotify:track:x",
        "spotify:track:y",
        "spotify:track:z",
    ]));
    assert!(library.create_playlist().is_some());
    let landed = PlaylistEntry {
        composed: Vec::new(),
        tracks: Some(3),
        ..created("spotify:playlist:new", "Mix", access(0x09, true))
    };

    library.apply(&Update::PlaylistCreated {
        entry: landed,
        revision: revision(0x08),
    });

    let entry = sidebar_entry(&library, "spotify:playlist:new").expect("the new row");
    assert_eq!(entry.tracks, Some(3), "the row counts what landed");
    assert_eq!(library.sidebar_notice, None);
}

#[test]
fn a_created_list_that_lost_the_pick_says_so() {
    let mut library = naming_library(&["a"], "Mix");
    library
        .naming
        .open_with(pick(&["spotify:track:x", "spotify:track:y"]));
    assert!(library.create_playlist().is_some());
    library.apply(&Update::PlaylistCreated {
        entry: created("spotify:playlist:new", "Mix", access(0x09, true)),
        revision: revision(0x08),
    });

    library.apply(&Update::PlaylistUnfilled {
        playlist: "spotify:playlist:new".to_owned(),
    });

    let entry = sidebar_entry(&library, "spotify:playlist:new").expect("the new row");
    assert_eq!(entry.tracks, Some(0), "the row counts what landed");
    assert_eq!(library.sidebar_notice, Some(SidebarWrite::Unfilled));
    assert!(!library.naming.is_open(), "the field is spent");
    assert!(!library.write_in_flight());
}

#[test]
fn a_created_list_that_lost_its_name_says_so() {
    let mut library = naming_library(&["a"], "Mix");
    assert!(library.create_playlist().is_some());
    library.apply(&Update::PlaylistCreated {
        entry: PlaylistEntry {
            composed: Vec::new(),
            // The rename never landed, so the row does not claim a name.
            named: false,
            ..created("spotify:playlist:new", "Mix", access(0x09, true))
        },
        revision: revision(0x08),
    });

    library.apply(&Update::PlaylistUnnamed {
        playlist: "spotify:playlist:new".to_owned(),
    });

    assert_eq!(library.sidebar_notice, Some(SidebarWrite::Unnamed));
    let entry = sidebar_entry(&library, "spotify:playlist:new").expect("the new row");
    assert_eq!(entry.name, "Mix", "the typed name stays for this session");
    assert!(!library.write_in_flight());
}

#[test]
fn an_unnamed_notice_for_a_list_not_held_is_ignored() {
    let mut library = sidebar_library(&["a"], 0x07, true, Some(3));
    library.apply(&Update::PlaylistUnnamed {
        playlist: "spotify:playlist:absent".to_owned(),
    });
    assert_eq!(library.sidebar_notice, None);
}

#[test]
fn an_unfilled_notice_for_a_list_not_held_is_ignored() {
    let mut library = sidebar_library(&["a"], 0x07, true, Some(3));

    library.apply(&Update::PlaylistUnfilled {
        playlist: "spotify:playlist:missing".to_owned(),
    });

    assert_eq!(library.sidebar_notice, None);
    assert_eq!(
        sidebar_entry(&library, "a").and_then(|entry| entry.tracks),
        Some(3)
    );
}

#[test]
fn a_create_failure_keeps_the_pick_and_the_text() {
    let mut library = naming_library(&["a"], "Mix");
    let tracks = pick(&["spotify:track:x"]);
    library.naming.open_with(tracks.clone());
    assert!(library.create_playlist().is_some());

    library.apply(&Update::PlaylistCreateFailed);

    assert_eq!(library.naming.text(), "Mix");
    assert_eq!(library.naming.pick(), tracks, "a retry lands the same pick");
    assert!(library.naming.is_open());
    assert!(!library.naming.writing());
}

#[test]
fn a_pending_create_blocks_moves_adds_and_menus() {
    let mut library = naming_library(&["a", "b"], "Mix");
    assert!(library.create_playlist().is_some());

    assert!(library.write_in_flight(), "the menu's gate stayed open");
    assert!(!library.rootlist_movable());
    assert_eq!(library.move_playlist(0, 1, "a"), None);
    assert_eq!(library.add_tracks(0, &pick(&["spotify:track:x"])), None);
    assert_eq!(sidebar_order(&library), names(&["a", "b"]), "a row moved");
}

#[test]
fn a_confirmed_create_inserts_at_index_zero_and_keeps_the_rest_in_order() {
    let mut library = naming_library(&["a", "b"], "Mix");
    assert!(library.create_playlist().is_some());

    library.apply(&Update::PlaylistCreated {
        entry: created("spotify:playlist:new", "Mix", access(0x09, true)),
        revision: revision(0x08),
    });

    assert_eq!(
        sidebar_order(&library),
        names(&["spotify:playlist:new", "a", "b"])
    );
}

#[test]
fn a_confirmed_create_on_an_empty_ready_list_is_the_only_row() {
    let mut library = naming_library(&[], "Mix");
    assert!(library.create_playlist().is_some());

    library.apply(&Update::PlaylistCreated {
        entry: created("spotify:playlist:new", "Mix", None),
        revision: revision(0x08),
    });

    assert_eq!(sidebar_order(&library), names(&["spotify:playlist:new"]));
}

#[test]
fn a_confirmed_create_does_not_duplicate_a_uri_already_listed() {
    let mut library = naming_library(&["a"], "Mix");
    assert!(library.create_playlist().is_some());

    library.apply(&Update::PlaylistCreated {
        entry: created("a", "Mix", access(0x09, true)),
        revision: revision(0x08),
    });

    assert_eq!(sidebar_order(&library), names(&["a"]));
}

#[test]
fn a_confirmed_create_holds_the_link_replys_revision_and_the_next_create_names_it() {
    let mut library = naming_library(&["a"], "Mix");
    assert!(library.create_playlist().is_some());
    library.apply(&Update::PlaylistCreated {
        entry: created("spotify:playlist:new", "Mix", None),
        revision: revision(0x08),
    });

    library.naming.open();
    library.naming.edit().push_str("Again");
    assert_eq!(
        library.create_playlist(),
        Some(Command::CreatePlaylist {
            name: "Again".to_owned(),
            revision: revision(0x08),
            tracks: Vec::new(),
        }),
        "the second create named the revision the first reply held"
    );
}

#[test]
fn a_confirmed_create_closes_naming_clears_the_notice_and_sets_the_reveal() {
    let mut library = naming_library(&["a"], "Mix");
    assert!(library.create_playlist().is_some());
    library.sidebar_notice = Some(SidebarWrite::Reorder);

    library.apply(&Update::PlaylistCreated {
        entry: created("spotify:playlist:new", "Mix", None),
        revision: revision(0x08),
    });

    assert!(!library.naming.is_open());
    assert!(!library.naming.writing());
    assert_eq!(library.naming.text(), "");
    assert!(
        library.naming.wants_button(),
        "the keyboard did not go back"
    );
    assert_eq!(library.sidebar_notice, None);
    assert!(library.reveal_top);
    assert!(!library.write_in_flight());
}

#[test]
fn a_confirmed_create_switches_the_chip_to_playlists() {
    let mut library = naming_library(&["a"], "Mix");
    assert!(library.sidebar_kind.choose(SidebarKind::Albums));
    assert!(library.create_playlist().is_some());

    library.apply(&Update::PlaylistCreated {
        entry: created("spotify:playlist:new", "Mix", None),
        revision: revision(0x08),
    });

    assert_eq!(library.sidebar_kind.chosen(), SidebarKind::Playlists);
}

#[test]
fn a_confirmed_create_never_leaves_the_list_idle() {
    let mut library = naming_library(&["a"], "Mix");
    assert!(library.create_playlist().is_some());

    library.apply(&Update::PlaylistCreated {
        entry: created("spotify:playlist:new", "Mix", None),
        revision: revision(0x08),
    });

    assert!(
        matches!(library.playlists, Load::Ready(_)),
        "a created row must never leave the sidebar waiting"
    );
}

#[test]
fn a_created_reply_with_no_ready_list_is_ignored() {
    let mut library = naming_library(&["a"], "Mix");
    assert!(library.create_playlist().is_some());
    library.playlists = Load::Loading;

    library.apply(&Update::PlaylistCreated {
        entry: created("spotify:playlist:new", "Mix", access(0x09, true)),
        revision: revision(0x08),
    });

    assert!(
        matches!(library.playlists, Load::Loading),
        "a read in flight was replaced by the created row's list"
    );
    assert!(!library.write_in_flight(), "the create stayed pending");
}

#[test]
fn a_playlists_read_during_a_create_is_not_adopted_and_its_access_goes_with_it() {
    let mut library = naming_library(&["a", "b"], "Mix");
    assert!(library.create_playlist().is_some());

    library.apply(&Update::Playlists(vec![playlist("c", "c")]));
    library.apply(&Update::RootlistAccess(rootlist_access(0x0f, true)));

    assert_eq!(
        sidebar_order(&library),
        names(&["a", "b"]),
        "a read taken before the link landed replaced the list"
    );
    assert_eq!(
        library
            .rootlist
            .as_ref()
            .map(|access| access.revision.clone()),
        Some(revision(0x07)),
        "the dropped read's revision was adopted"
    );
}

#[test]
fn a_rootlist_access_during_a_create_is_not_adopted() {
    let mut library = naming_library(&["a"], "Mix");
    assert!(library.create_playlist().is_some());

    library.apply(&Update::RootlistAccess(rootlist_access(0x0f, false)));

    let held = library.rootlist.clone().expect("the held access");
    assert_eq!(held.revision, revision(0x07));
    assert!(held.editable, "the access was replaced mid-create");
}

#[test]
fn a_selected_playlist_stays_selected_after_the_insert() {
    let mut library = naming_library(&["a", "b"], "Mix");
    library.select("b");
    assert!(library.create_playlist().is_some());

    library.apply(&Update::PlaylistCreated {
        entry: created("spotify:playlist:new", "Mix", None),
        revision: revision(0x08),
    });

    assert_eq!(library.selected(), Some("b"), "the pick follows the URI");
}

#[test]
fn a_make_failure_says_create_and_keeps_the_text() {
    let mut library = naming_library(&["a"], "Night Drive");
    assert!(library.create_playlist().is_some());

    library.apply(&Update::PlaylistCreateFailed);

    assert_eq!(library.sidebar_notice, Some(SidebarWrite::Create));
    assert!(library.naming.is_open());
    assert!(!library.naming.writing());
    assert!(library.naming.wants_focus(), "the retry had no keyboard");
    assert_eq!(library.naming.text(), "Night Drive");
    assert_eq!(sidebar_order(&library), names(&["a"]), "a row appeared");
    assert!(!library.write_in_flight());
}

#[test]
fn a_link_failure_says_link_and_closes_naming() {
    let mut library = naming_library(&["a"], "Night Drive");
    assert!(library.create_playlist().is_some());

    library.apply(&Update::PlaylistUnlinked);

    assert_eq!(library.sidebar_notice, Some(SidebarWrite::Link));
    assert!(!library.naming.is_open());
    assert_eq!(library.naming.text(), "");
    assert_eq!(sidebar_order(&library), names(&["a"]), "a row appeared");
    assert!(!library.write_in_flight());
}

#[test]
fn a_link_failure_asks_for_a_fresh_read() {
    let mut library = naming_library(&["a"], "Night Drive");
    assert!(library.create_playlist().is_some());

    library.apply(&Update::PlaylistUnlinked);

    assert!(
        library.rootlist.is_none(),
        "a revision from before the link is one the next write cannot name"
    );
    library.naming.open();
    library.naming.edit().push_str("Again");
    assert_eq!(library.create_playlist(), None, "a write on no revision");
    assert_eq!(library.rootlist_request(), Some(Command::ReloadLibrary));
    assert_eq!(library.rootlist_request(), None, "a second read");
}

#[test]
fn a_stray_reply_with_nothing_pending_is_ignored() {
    let mut library = sidebar_library(&["a"], 0x07, true, None);

    library.apply(&Update::PlaylistCreated {
        entry: created("spotify:playlist:new", "Mix", None),
        revision: revision(0x08),
    });
    library.apply(&Update::PlaylistCreateFailed);
    library.apply(&Update::PlaylistUnlinked);

    assert_eq!(sidebar_order(&library), names(&["a"]));
    assert_eq!(library.sidebar_notice, None);
    assert_eq!(
        library
            .rootlist
            .as_ref()
            .map(|access| access.revision.clone()),
        Some(revision(0x07))
    );
}

#[test]
fn a_new_gesture_clears_the_create_sentence() {
    let mut library = naming_library(&["a", "b"], "Mix");
    assert!(library.create_playlist().is_some());
    library.apply(&Update::PlaylistCreateFailed);
    assert_eq!(library.sidebar_notice, Some(SidebarWrite::Create));

    assert!(library.move_playlist(0, 2, "a").is_some());
    assert_eq!(library.sidebar_notice, None);
}

#[test]
fn the_created_entry_is_addable_when_the_state_read_named_an_editable_access() {
    let mut library = naming_library(&[], "Mix");
    assert!(library.create_playlist().is_some());
    library.apply(&Update::PlaylistCreated {
        entry: created("spotify:playlist:new", "Mix", access(0x09, true)),
        revision: revision(0x08),
    });

    assert!(library.addable(0), "the new row cannot take a track");
    let entry = sidebar_entry(&library, "spotify:playlist:new").expect("the new row");
    assert_eq!(entry.tracks, Some(0));
    assert_eq!(entry.add_basis().map(|(_, count)| count), Some(0));
}
