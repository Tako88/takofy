use super::super::*; // the module under test
use super::*; // the slice's shared fixtures, from tests/mod.rs

#[test]
fn a_chip_change_clears_the_remove_sentence() {
    let mut library = sidebar_library(&["a"], 0x07, true, None);
    library.sidebar_notice = Some(SidebarWrite::Remove);

    assert!(library.choose_kind(SidebarKind::Podcasts));

    assert_eq!(library.sidebar_notice, None);
}
/// A sidebar holding one editable playlist, its page loaded.
fn renaming_library() -> Library {
    let mut library = sidebar_library(&["a"], 0x07, true, None);
    library.select("a");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t0")],
        cover: None,
        access: access(0x07, true),
    });
    library
}

#[test]
fn a_confirmed_rename_sets_the_name_named_and_both_revisions() {
    let mut library = renaming_library();
    rename_pending(&mut library, "a", crate::naming::Surface::Row);

    library.apply(&Update::PlaylistRenamed {
        playlist: "a".to_owned(),
        name: "New Name".to_owned(),
        revision: revision(0x09),
    });

    let entry = sidebar_entry(&library, "a").expect("the row");
    assert_eq!(entry.name, "New Name");
    assert!(entry.named);
    assert_eq!(
        entry.access.as_ref().map(|held| held.revision.clone()),
        Some(revision(0x09))
    );
    assert_eq!(
        match &library.tracks {
            Load::Ready(tracks) => tracks.access.as_ref().map(|held| held.revision.clone()),
            Load::Idle | Load::Loading | Load::Failed(_) => None,
        },
        Some(revision(0x09)),
        "the page's next move would name a stale base"
    );
    assert!(!library.renaming.is_open());
    assert!(!library.write_in_flight());
    assert_eq!(library.sidebar_notice, None);
}

#[test]
fn a_rename_leaves_the_cover_the_band_and_the_row_already_showed() {
    let first = cover_url("first");
    let mut library = covered_page(&first, &["spotify:track:t0"]);
    rename_pending(&mut library, "a", crate::naming::Surface::Band);

    library.apply(&Update::PlaylistRenamed {
        playlist: "a".to_owned(),
        name: "New Name".to_owned(),
        revision: revision(0x09),
    });

    let entry = library.selected_entry().expect("the band's entry");
    assert_eq!(
        entry.name, "New Name",
        "the band titles the playlist from a name the rename did not write"
    );
    assert_eq!(
        band_cover(entry, settled(&library)),
        BandCover::Entry(Some(&CoverSet::single(first.clone()))),
        "a rename names the playlist, so it cannot change the picture beside the name"
    );
    assert_eq!(
        entry_cover(&library, "a"),
        Some(PlaylistCover::derived(CoverSet::single(first))),
        "the sidebar row and the library card keep the cover they showed"
    );
    assert_eq!(
        library.tracks_request(),
        None,
        "a rename marks no cover due, so it asks for no read of the page"
    );
}

#[test]
fn a_named_false_playlist_can_be_renamed_and_the_confirm_names_it() {
    let mut library = renaming_library();
    if let Load::Ready(playlists) = &mut library.playlists {
        playlists[0].named = false;
    }
    rename_pending(&mut library, "a", crate::naming::Surface::Row);

    library.apply(&Update::PlaylistRenamed {
        playlist: "a".to_owned(),
        name: "Named".to_owned(),
        revision: revision(0x09),
    });

    let entry = sidebar_entry(&library, "a").expect("the row");
    assert_eq!(entry.name, "Named");
    assert!(entry.named);
}

#[test]
fn a_rename_confirm_for_another_uri_changes_nothing() {
    let mut library = renaming_library();
    rename_pending(&mut library, "a", crate::naming::Surface::Row);

    library.apply(&Update::PlaylistRenamed {
        playlist: "b".to_owned(),
        name: "Other".to_owned(),
        revision: revision(0x09),
    });

    let entry = sidebar_entry(&library, "a").expect("the row");
    assert_eq!(entry.name, "a");
    assert!(library.renaming.is_open());
    assert!(library.write_in_flight());
    assert_eq!(
        entry.access.as_ref().map(|held| held.revision.clone()),
        Some(revision(0x07))
    );
}

#[test]
fn a_rename_failure_keeps_the_text_and_says_so() {
    let mut library = renaming_library();
    rename_pending(&mut library, "a", crate::naming::Surface::Row);
    library.renaming.edit().clear();
    library.renaming.edit().push_str("Night Drive");

    library.apply(&Update::PlaylistRenameFailed {
        playlist: "a".to_owned(),
    });

    assert!(library.renaming.is_open());
    assert_eq!(library.renaming.text(), "Night Drive");
    assert!(library.renaming.wants_focus());
    assert!(!library.renaming.writing());
    assert_eq!(library.sidebar_notice, Some(SidebarWrite::Rename));
    assert!(!library.write_in_flight());
}

#[test]
fn a_band_rename_failure_also_marks_the_loaded_page() {
    let mut library = renaming_library();
    rename_pending(&mut library, "a", crate::naming::Surface::Band);

    library.apply(&Update::PlaylistRenameFailed {
        playlist: "a".to_owned(),
    });

    assert_eq!(
        match &library.tracks {
            Load::Ready(tracks) => tracks.notice,
            Load::Idle | Load::Loading | Load::Failed(_) => None,
        },
        Some(Failure::Rename)
    );
    assert_eq!(library.sidebar_notice, Some(SidebarWrite::Rename));
}

#[test]
fn a_row_rename_failure_leaves_the_page_notice_alone() {
    let mut library = renaming_library();
    rename_pending(&mut library, "a", crate::naming::Surface::Row);

    library.apply(&Update::PlaylistRenameFailed {
        playlist: "a".to_owned(),
    });

    assert_eq!(
        match &library.tracks {
            Load::Ready(tracks) => tracks.notice,
            Load::Idle | Load::Loading | Load::Failed(_) => None,
        },
        None
    );
}

#[test]
fn a_rename_confirm_clears_only_its_own_page_notice() {
    let mut library = renaming_library();
    if let Load::Ready(tracks) = &mut library.tracks {
        tracks.notice = Some(Failure::Rename);
    }
    rename_pending(&mut library, "a", crate::naming::Surface::Band);

    library.apply(&Update::PlaylistRenamed {
        playlist: "a".to_owned(),
        name: "New".to_owned(),
        revision: revision(0x09),
    });
    assert_eq!(
        match &library.tracks {
            Load::Ready(tracks) => tracks.notice,
            Load::Idle | Load::Loading | Load::Failed(_) => None,
        },
        None
    );

    if let Load::Ready(tracks) = &mut library.tracks {
        tracks.notice = Some(Failure::Save);
    }
    rename_pending(&mut library, "a", crate::naming::Surface::Band);
    library.apply(&Update::PlaylistRenamed {
        playlist: "a".to_owned(),
        name: "Newer".to_owned(),
        revision: revision(0x0a),
    });
    assert_eq!(
        match &library.tracks {
            Load::Ready(tracks) => tracks.notice,
            Load::Idle | Load::Loading | Load::Failed(_) => None,
        },
        Some(Failure::Save),
        "an unrelated notice was cleared"
    );
}

#[test]
fn a_playlists_read_during_a_rename_is_adopted() {
    let mut library = renaming_library();
    rename_pending(&mut library, "a", crate::naming::Surface::Row);

    library.apply(&Update::Playlists(vec![playlist("b", "B")]));

    match &library.playlists {
        Load::Ready(playlists) => {
            assert_eq!(playlists.len(), 1);
            assert_eq!(playlists[0].uri, "b");
        }
        other => panic!("the read was not adopted: {other:?}"),
    }
    assert!(library.write_in_flight(), "the rename went with it");
}

#[test]
fn a_stale_tracks_read_for_another_playlist_leaves_the_revision_alone() {
    let mut library = renaming_library();
    rename_pending(&mut library, "a", crate::naming::Surface::Band);
    library.apply(&Update::PlaylistRenamed {
        playlist: "a".to_owned(),
        name: "New".to_owned(),
        revision: revision(0x09),
    });

    library.apply(&Update::Tracks {
        playlist: "b".to_owned(),
        tracks: vec![track("t1")],
        cover: None,
        access: access(0x05, true),
    });

    assert_eq!(
        match &library.tracks {
            Load::Ready(tracks) => tracks.access.as_ref().map(|held| held.revision.clone()),
            Load::Idle | Load::Loading | Load::Failed(_) => None,
        },
        Some(revision(0x09)),
        "another page's read replaced this page's base"
    );
}

#[test]
fn can_rename_needs_a_ready_editable_list_and_no_write() {
    assert!(sidebar_library(&["a"], 0x07, true, None).can_rename("a"));

    assert!(
        !sidebar_library(&["a"], 0x07, false, None).can_rename("a"),
        "a list the user cannot write offered a rename"
    );
    assert!(
        !sidebar_library(&["a"], 0x07, true, None).can_rename("b"),
        "a row the list does not hold offered a rename"
    );
    let mut no_access = Library::default();
    no_access.apply(&Update::Playlists(vec![playlist("a", "A")]));
    assert!(
        !no_access.can_rename("a"),
        "a row with no access offered a rename"
    );
    let mut writing = sidebar_library(&["a", "b"], 0x07, true, None);
    assert!(writing.move_playlist(0, 2, "a").is_some());
    assert!(
        !writing.can_rename("a"),
        "a second write went out beside the first"
    );
    let mut loading = sidebar_library(&["a"], 0x07, true, None);
    loading.playlists = Load::Loading;
    assert!(!loading.can_rename("a"), "a list that is not ready");
}

#[test]
fn begin_rename_fills_the_field_with_the_current_name() {
    let mut library = sidebar_library(&["a"], 0x07, true, None);

    assert!(library.begin_rename("a", crate::naming::Surface::Row));

    assert!(
        library
            .renaming
            .is_open_on("a", crate::naming::Surface::Row)
    );
    assert_eq!(library.renaming.text(), "a");
    assert!(library.renaming.wants_focus());
    assert!(!library.write_in_flight());
}

#[test]
fn a_refused_rename_start_changes_nothing() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, None);
    assert!(library.move_playlist(0, 2, "a").is_some());

    assert!(!library.begin_rename("a", crate::naming::Surface::Row));
    assert!(!library.renaming.is_open());
}

#[test]
fn rename_issues_one_command_with_the_trimmed_name() {
    let mut library = sidebar_library(&["a"], 0x07, true, None);
    assert!(library.begin_rename("a", crate::naming::Surface::Row));
    *library.renaming.edit() = "  Night  Drive ".to_owned();

    assert_eq!(
        library.rename_playlist(),
        Some(Command::RenamePlaylist {
            playlist: "a".to_owned(),
            name: "Night  Drive".to_owned(),
        })
    );
    assert!(library.renaming.writing());
    assert!(library.write_in_flight());
    assert_eq!(
        library.sidebar_pending,
        Some(SidebarPending::Rename {
            playlist: "a".to_owned(),
        })
    );
    assert_eq!(library.rename_playlist(), None, "a second write");
}

#[test]
fn a_rename_to_the_same_name_sends_nothing_and_closes() {
    let mut library = sidebar_library(&["a"], 0x07, true, None);
    assert!(library.begin_rename("a", crate::naming::Surface::Band));
    *library.renaming.edit() = "  a  ".to_owned();

    assert_eq!(library.rename_playlist(), None);
    assert!(!library.renaming.is_open());
    assert!(!library.write_in_flight());
    assert!(library.renaming.wants_return());
}

#[test]
fn a_rename_to_the_text_an_unnamed_row_shows_is_sent() {
    // A create whose own rename failed keeps the typed name in the row for
    // this session only: `named` is false, so the server has never heard it.
    // Opening Rename on that row prefills the text it shows, and Enter must
    // still reach the wire.
    let mut library = renaming_library();
    if let Load::Ready(playlists) = &mut library.playlists {
        playlists[0].named = false;
    }
    assert!(library.begin_rename("a", crate::naming::Surface::Row));
    assert_eq!(library.renaming.text(), "a", "the field prefilled nothing");

    assert_eq!(
        library.rename_playlist(),
        Some(Command::RenamePlaylist {
            playlist: "a".to_owned(),
            name: "a".to_owned(),
        }),
        "an unnamed row's own text was treated as the server's name"
    );
    assert!(library.renaming.writing());

    library.apply(&Update::PlaylistRenamed {
        playlist: "a".to_owned(),
        name: "a".to_owned(),
        revision: revision(0x09),
    });
    let entry = sidebar_entry(&library, "a").expect("the row");
    assert!(entry.named, "the sent name did not make the row named");
}

#[test]
fn a_rename_enter_is_refused_while_a_page_write_holds_the_list() {
    let mut library = renaming_library();
    assert!(library.begin_rename("a", crate::naming::Surface::Band));
    *library.renaming.edit() = "New".to_owned();
    let first = match &library.tracks {
        Load::Ready(tracks) => tracks.tracks[0].clone(),
        Load::Idle | Load::Loading | Load::Failed(_) => panic!("the page is ready"),
    };
    assert!(library.remove_tracks("a", &[0], &[first]).is_some());

    assert_eq!(
        library.rename_playlist(),
        None,
        "a rename overran a write the page had already started"
    );
    assert!(library.renaming.is_open(), "a refusal closed the field");
    assert!(!library.renaming.writing());
    assert_eq!(library.renaming.text(), "New", "a refusal dropped the text");
}

#[test]
fn a_rename_after_a_changed_name_sends_once() {
    let mut library = sidebar_library(&["a"], 0x07, true, None);
    assert!(library.begin_rename("a", crate::naming::Surface::Row));
    *library.renaming.edit() = "New".to_owned();

    assert!(library.rename_playlist().is_some());
    assert_eq!(library.rename_playlist(), None);
    assert!(library.renaming.writing());
}

#[test]
fn a_page_change_closes_a_band_field_but_not_a_row_field_or_a_writing_one() {
    let mut band = renaming_library();
    assert!(band.begin_rename("a", crate::naming::Surface::Band));
    band.to_playlists();
    assert!(!band.renaming.is_open(), "a band field outlived its page");

    let mut row = renaming_library();
    assert!(row.begin_rename("a", crate::naming::Surface::Row));
    row.to_playlists();
    assert!(row.renaming.is_open(), "a row field outlived its page");

    let mut writing = renaming_library();
    assert!(writing.begin_rename("a", crate::naming::Surface::Band));
    *writing.renaming.edit() = "New".to_owned();
    assert!(writing.rename_playlist().is_some());
    writing.to_playlists();
    assert!(
        writing.renaming.is_open(),
        "a write in flight lost its field"
    );
}

#[test]
fn a_removed_playlist_closes_its_rename_field() {
    let mut library = renaming_library();
    assert!(library.begin_rename("a", crate::naming::Surface::Row));

    library.apply(&Update::Playlists(Vec::new()));

    assert!(!library.renaming.is_open());
    assert!(!library.renaming.wants_return());
}
