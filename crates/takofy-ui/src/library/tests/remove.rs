use super::super::*; // the module under test
use super::*; // the slice's shared fixtures, from tests/mod.rs

#[test]
fn a_removal_issues_one_command_with_the_chain_and_the_held_revision() {
    let mut library = page_with("a", &["t0", "t1", "t2", "t3", "t4"], access(0x01, true));

    assert_eq!(
        library.remove_tracks("a", &[1, 3], &moved(&["t1", "t3"])),
        Some(Command::RemoveTracks {
            playlist: "a".to_owned(),
            revision: revision(0x01),
            removals: vec![
                Removal { from: 3, length: 1 },
                Removal { from: 1, length: 1 },
            ],
        })
    );
}

#[test]
fn a_removal_applies_the_rows_at_once_and_records_pending() {
    let mut library = page_with("a", &["t0", "t1", "t2", "t3"], access(0x01, true));

    assert!(
        library
            .remove_tracks("a", &[1, 2], &moved(&["t1", "t2"]))
            .is_some()
    );
    assert_eq!(
        library.tracks,
        ready("a", &["t0", "t3"], access(0x01, true)),
        "the rows are gone before the write is answered"
    );
    assert_eq!(
        library.pending,
        Some(crate::reorder::Pending {
            playlist: "a".to_owned(),
            before: moved(&["t0", "t1", "t2", "t3"]),
            after: moved(&["t0", "t3"]),
            query: None,
        })
    );
}

#[test]
fn a_removal_that_fails_puts_the_query_back_with_the_rows() {
    let mut library = page_with("a", &["midnight"], access(0x01, true));
    library.track_filter.edit("a").push_str("midnight");

    assert!(
        library
            .remove_tracks("a", &[0], &moved(&["midnight"]))
            .is_some()
    );
    assert_eq!(
        library.track_filter.needle("a"),
        None,
        "arrange: the empty page takes the query with the last row"
    );

    library.apply(&Update::RemoveFailed {
        playlist: "a".to_owned(),
        failure: Failure::Save,
    });

    assert_eq!(
        library.track_filter.needle("a").as_deref(),
        Some("midnight"),
        "a failed write puts the rows back, and the query with them"
    );
}

#[test]
fn a_removal_that_empties_the_page_drops_the_track_filter() {
    let mut library = page_with("a", &["midnight"], access(0x01, true));
    library.track_filter.edit("a").push_str("midnight");

    assert!(
        library
            .remove_tracks("a", &[0], &moved(&["midnight"]))
            .is_some()
    );

    assert_eq!(
        library.track_filter.needle("a"),
        None,
        "an empty page cannot show the query, so the last row takes it with it"
    );
}

#[test]
fn a_removal_waits_while_a_page_write_is_pending() {
    let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
    assert!(library.remove_tracks("a", &[0], &moved(&["t0"])).is_some());

    assert_eq!(
        library.remove_tracks("a", &[0], &moved(&["t1"])),
        None,
        "one gesture is one write, so a second waits for the reply"
    );
    assert!(!library.removable());
}

#[test]
fn a_removal_waits_while_a_sidebar_write_is_pending() {
    let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
    library.playlists = Load::Ready(vec![
        with_access(playlist("a", "a"), 0x07, true, None),
        with_access(playlist("b", "b"), 0x07, true, None),
    ]);
    library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));
    assert!(library.move_playlist(0, 2, "a").is_some());

    assert_eq!(
        library.remove_tracks("a", &[0], &moved(&["t0"])),
        None,
        "a sidebar write holds every list still"
    );
}

#[test]
fn a_removal_refuses_a_read_only_page() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, false));

    assert_eq!(library.remove_tracks("a", &[0], &moved(&["t0"])), None);
    assert!(!library.removable());
    assert_eq!(library.pending, None);
    assert_eq!(
        library.tracks,
        ready("a", &["t0", "t1"], access(0x01, false))
    );
}

#[test]
fn a_removal_refuses_a_stale_pick() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));

    assert_eq!(
        library.remove_tracks("a", &[0], &moved(&["gone"])),
        None,
        "the row holds another entry now"
    );
    assert_eq!(
        library.remove_tracks("a", &[0, 1], &moved(&["t0"])),
        None,
        "the rows and the entries are not parallel"
    );
    assert_eq!(
        library.remove_tracks("a", &[9], &moved(&["gone"])),
        None,
        "a row the list does not hold"
    );
    assert_eq!(library.pending, None);
}

#[test]
fn a_removal_refuses_an_unsorted_pick() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));

    assert_eq!(
        library.remove_tracks("a", &[1, 0], &moved(&["t1", "t0"])),
        None
    );
    assert_eq!(
        library.remove_tracks("a", &[0, 0], &moved(&["t0", "t0"])),
        None,
        "a repeated row is not a pick"
    );
    assert_eq!(library.pending, None);
}

#[test]
fn the_reply_revision_ends_the_write_and_the_next_removal_uses_it() {
    let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
    assert!(library.remove_tracks("a", &[1], &moved(&["t1"])).is_some());

    library.apply(&Update::TracksRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });

    assert_eq!(library.pending, None);
    assert_eq!(
        library.tracks,
        ready("a", &["t0", "t2"], access(0x02, true))
    );
    assert_eq!(
        library.remove_tracks("a", &[1], &moved(&["t2"])),
        Some(Command::RemoveTracks {
            playlist: "a".to_owned(),
            revision: revision(0x02),
            removals: vec![Removal { from: 1, length: 1 }],
        })
    );
}

#[test]
fn the_reply_lowers_the_sidebar_count_by_the_rows_removed() {
    let mut library = page_with("a", &["t0", "t1", "t2", "t3"], access(0x01, true));
    library.playlists = Load::Ready(vec![with_access(playlist("a", "a"), 0x07, true, Some(12))]);
    assert!(
        library
            .remove_tracks("a", &[1, 2], &moved(&["t1", "t2"]))
            .is_some()
    );

    library.apply(&Update::TracksRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });

    assert_eq!(
        sidebar_entry(&library, "a").and_then(|entry| entry.tracks),
        Some(10)
    );
}

#[test]
fn a_move_reply_leaves_the_count_alone() {
    let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
    library.playlists = Load::Ready(vec![with_access(playlist("a", "a"), 0x07, true, Some(12))]);
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

    library.apply(&Update::TrackMoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });

    assert_eq!(
        sidebar_entry(&library, "a").and_then(|entry| entry.tracks),
        Some(12),
        "a move changes the order, never the count"
    );
}

#[test]
fn a_failed_removal_restores_the_rows_and_says_so() {
    let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
    assert!(library.remove_tracks("a", &[1], &moved(&["t1"])).is_some());

    library.apply(&Update::RemoveFailed {
        playlist: "a".to_owned(),
        failure: Failure::Save,
    });

    assert_eq!(library.pending, None);
    assert_eq!(
        library.tracks,
        Load::Ready(TracksFor {
            playlist: "a".to_owned(),
            tracks: moved(&["t0", "t1", "t2"]),
            access: access(0x01, true),
            notice: Some(Failure::Save),
        })
    );
}

#[test]
fn a_failed_removal_leaves_the_sidebar_count_alone() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
    library.playlists = Load::Ready(vec![with_access(playlist("a", "a"), 0x07, true, Some(7))]);
    assert!(library.remove_tracks("a", &[1], &moved(&["t1"])).is_some());

    library.apply(&Update::RemoveFailed {
        playlist: "a".to_owned(),
        failure: Failure::Save,
    });

    assert_eq!(
        sidebar_entry(&library, "a").and_then(|entry| entry.tracks),
        Some(7)
    );
}

#[test]
fn a_read_while_a_removal_is_pending_keeps_the_removed_order() {
    let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
    assert!(library.remove_tracks("a", &[1], &moved(&["t1"])).is_some());

    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: moved(&["t0", "t1", "t2"]),
        cover: None,
        access: access(0x01, true),
    });

    assert_eq!(
        library.tracks,
        ready("a", &["t0", "t2"], access(0x01, true)),
        "a read issued before the write landed must not resurrect the rows"
    );
}

#[test]
fn a_removal_never_drops_the_page() {
    let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
    assert!(library.remove_tracks("a", &[1], &moved(&["t1"])).is_some());
    assert!(
        matches!(library.tracks, Load::Ready(_)),
        "the issue keeps the page ready, so nothing has to be re-requested"
    );

    library.apply(&Update::TracksRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });
    assert!(matches!(library.tracks, Load::Ready(_)));

    library.apply(&Update::RemoveFailed {
        playlist: "a".to_owned(),
        failure: Failure::Save,
    });
    assert!(matches!(library.tracks, Load::Ready(_)));
}

#[test]
fn removable_matches_movable() {
    for (uris, access) in [
        (&["t0", "t1"][..], access(0x01, true)),
        (&["t0", "t1"][..], access(0x01, false)),
        (&["t0", "t1"][..], None),
    ] {
        let library = page_with("a", uris, access);
        assert_eq!(library.removable(), library.movable(), "{uris:?}");
    }

    let mut writing = page_with("a", &["t0", "t1"], access(0x01, true));
    assert!(writing.remove_tracks("a", &[0], &moved(&["t0"])).is_some());
    assert_eq!(writing.removable(), writing.movable());
    assert!(!writing.removable());
}

#[test]
fn the_created_entry_is_not_addable_without_an_editable_access() {
    let mut library = naming_library(&[], "Mix");
    assert!(library.create_playlist().is_some());
    library.apply(&Update::PlaylistCreated {
        entry: created("spotify:playlist:new", "Mix", None),
        revision: revision(0x08),
    });

    assert!(!library.addable(0), "a row with no access took a track");
}

#[test]
fn a_playlist_removal_names_the_row_and_the_rootlist_revision() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, None);

    let command = library.remove_from_library(&item(SidebarKind::Playlists, "b"));

    assert_eq!(
        command,
        Some(Command::RemovePlaylist {
            playlist: "b".to_owned(),
            revision: revision(0x07),
            index: 1,
        })
    );
    assert_eq!(
        library.sidebar_pending,
        Some(SidebarPending::Remove {
            playlist: "b".to_owned(),
            index: 1,
        })
    );
}

#[test]
fn a_playlist_removal_is_refused_without_leaving_a_pending() {
    let mut no_access = Library::default();
    no_access.apply(&Update::Playlists(vec![playlist("a", "A")]));
    assert_eq!(
        no_access.remove_from_library(&item(SidebarKind::Playlists, "a")),
        None,
        "a read with no revision offered a write"
    );
    assert!(!no_access.write_in_flight());

    let mut not_editable = sidebar_library(&["a"], 0x07, false, None);
    assert_eq!(
        not_editable.remove_from_library(&item(SidebarKind::Playlists, "a")),
        None,
        "a list the user cannot write offered a write"
    );
    assert!(!not_editable.write_in_flight());

    let mut writing = sidebar_library(&["a", "b"], 0x07, true, None);
    assert!(writing.move_playlist(0, 2, "a").is_some());
    assert_eq!(
        writing.remove_from_library(&item(SidebarKind::Playlists, "a")),
        None,
        "a second write went out beside the first"
    );

    let mut gone = sidebar_library(&["b"], 0x07, true, None);
    assert_eq!(
        gone.remove_from_library(&item(SidebarKind::Playlists, "a")),
        None,
        "a row the list does not hold was removed"
    );
    assert!(!gone.write_in_flight());

    let mut loading = sidebar_library(&["a"], 0x07, true, None);
    loading.playlists = Load::Loading;
    assert_eq!(
        loading.remove_from_library(&item(SidebarKind::Playlists, "a")),
        None,
        "a list that is not ready offered a write"
    );
    assert!(!loading.write_in_flight());
}

#[test]
fn a_saved_removal_is_refused_until_its_own_list_is_ready() {
    for (kind, uri) in [
        (SidebarKind::Albums, "spotify:album:r"),
        (SidebarKind::Artists, "spotify:artist:a"),
        (SidebarKind::Podcasts, "spotify:show:s"),
    ] {
        let mut library = saved_library();
        assert!(
            library.remove_from_library(&item(kind, uri)).is_some(),
            "{kind:?} was not removed at all"
        );
        assert_eq!(
            library.remove_from_library(&item(kind, uri)),
            None,
            "{kind:?} sent a second write beside the first"
        );
    }

    let mut library = saved_library();
    library.saved_albums = Load::Idle;
    assert_eq!(
        library.remove_from_library(&item(SidebarKind::Albums, "spotify:album:r")),
        None
    );
    library.followed_artists = Load::Failed(Failure::FollowedArtists);
    assert_eq!(
        library.remove_from_library(&item(SidebarKind::Artists, "spotify:artist:a")),
        None
    );
    library.followed = Load::Loading;
    assert_eq!(
        library.remove_from_library(&item(SidebarKind::Podcasts, "spotify:show:s")),
        None
    );
    assert!(!library.write_in_flight());
}

#[test]
fn an_artist_removal_is_refused_while_its_pages_follow_write_is_pending() {
    let mut library = saved_library();
    library.open(Page::Artist("spotify:artist:a".to_owned()));
    library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
    library.apply(&Update::Followed {
        artist: "spotify:artist:a".to_owned(),
        followed: false,
    });
    assert_eq!(
        library.follow.pressed(),
        Some(Command::SetFollow {
            artist: "spotify:artist:a".to_owned(),
            followed: true,
        })
    );

    assert_eq!(
        library.remove_from_library(&item(SidebarKind::Artists, "spotify:artist:a")),
        None,
        "the removal raced the page's own follow write"
    );
    assert!(!library.write_in_flight());
}

#[test]
fn a_confirmed_playlist_removal_drops_the_row_and_holds_the_reply_revision() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, None);
    assert!(
        library
            .remove_from_library(&item(SidebarKind::Playlists, "a"))
            .is_some()
    );
    assert_eq!(
        sidebar_order(&library),
        names(&["a", "b"]),
        "the row left before the write was confirmed"
    );

    library.apply(&Update::PlaylistRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x09),
    });

    assert_eq!(sidebar_order(&library), names(&["b"]));
    assert!(!library.write_in_flight());
    assert_eq!(
        library.rootlist.as_ref().map(|held| held.revision.clone()),
        Some(revision(0x09)),
        "the next write must name the reply's revision"
    );
}

#[test]
fn a_confirm_drops_one_row_of_a_duplicate_uri() {
    let mut library = sidebar_library(&["a", "a"], 0x07, true, None);

    assert!(
        library
            .remove_from_library(&item(SidebarKind::Playlists, "a"))
            .is_some()
    );
    library.apply(&Update::PlaylistRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x09),
    });

    assert_eq!(
        sidebar_order(&library),
        names(&["a"]),
        "the confirm dropped every row naming that playlist"
    );
}

#[test]
fn a_confirm_at_a_stale_index_rereads_instead_of_mutating_the_list() {
    let mut shifted = sidebar_library(&["a", "b"], 0x07, true, None);
    assert!(
        shifted
            .remove_from_library(&item(SidebarKind::Playlists, "b"))
            .is_some()
    );
    // The list no longer holds the row at the index the write named, as a
    // replacement list would leave it.
    shifted.playlists = Load::Ready(vec![with_access(playlist("a", "A"), 0x07, true, None)]);
    shifted.apply(&Update::PlaylistRemoved {
        playlist: "b".to_owned(),
        revision: revision(0x09),
    });

    assert_eq!(
        sidebar_order(&shifted),
        names(&["a"]),
        "a removal at a stale index dropped the wrong row"
    );
    assert!(
        shifted.rootlist.is_none(),
        "a stale index kept the held revision"
    );
    assert!(shifted.rootlist_stale, "the moved list owed a fresh read");
    assert_eq!(shifted.rootlist_request(), Some(Command::ReloadLibrary));

    let mut vanished = sidebar_library(&["a"], 0x07, true, None);
    assert!(
        vanished
            .remove_from_library(&item(SidebarKind::Playlists, "a"))
            .is_some()
    );
    vanished.playlists = Load::Ready(Vec::new());
    vanished.apply(&Update::PlaylistRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x09),
    });

    assert_eq!(sidebar_order(&vanished), names(&[]));
    assert!(vanished.rootlist_stale, "a missing row owed a fresh read");
}

#[test]
fn a_follow_up_removal_names_the_shifted_index() {
    let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, None);
    assert!(
        library
            .remove_from_library(&item(SidebarKind::Playlists, "a"))
            .is_some()
    );
    library.apply(&Update::PlaylistRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x09),
    });

    assert_eq!(
        library.remove_from_library(&item(SidebarKind::Playlists, "b")),
        Some(Command::RemovePlaylist {
            playlist: "b".to_owned(),
            revision: revision(0x09),
            index: 0,
        })
    );
}

#[test]
fn removing_the_open_playlist_goes_home_while_an_album_page_survives() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, None);
    library.open(album("spotify:album:r"));
    library.select("a");
    assert_eq!(library.page(), Some(&Page::Playlist("a".to_owned())));

    assert!(
        library
            .remove_from_library(&item(SidebarKind::Playlists, "a"))
            .is_some()
    );
    library.apply(&Update::PlaylistRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x09),
    });

    assert_eq!(
        library.page(),
        None,
        "the removed page stayed open instead of falling to the root"
    );
    let removed = Page::Playlist("a".to_owned());
    assert!(
        !library.history.pages().any(|page| *page == removed),
        "the removed playlist stayed in the history: {:?}",
        library.history
    );
    assert_eq!(library.wanted, None, "the removed playlist stayed wanted");
    assert!(
        library
            .history
            .pages()
            .any(|page| *page == Page::Album("spotify:album:r".to_owned())),
        "the album behind it was pruned with it"
    );
    assert!(library.can_back());
    library.back();
    assert_eq!(
        library.page(),
        Some(&Page::Album("spotify:album:r".to_owned())),
        "Back does not land on the album behind the removed page"
    );
}

#[test]
fn a_failed_playlist_removal_keeps_the_row_and_asks_for_a_read() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, None);
    assert!(
        library
            .remove_from_library(&item(SidebarKind::Playlists, "a"))
            .is_some()
    );

    library.apply(&Update::PlaylistRemoveFailed {
        playlist: "a".to_owned(),
    });

    assert_eq!(sidebar_order(&library), names(&["a", "b"]));
    assert_eq!(library.sidebar_notice, Some(SidebarWrite::Remove));
    assert!(!library.write_in_flight());
    assert!(library.rootlist.is_none(), "the held revision was kept");
    assert_eq!(library.rootlist_request(), Some(Command::ReloadLibrary));

    // The read the failure asked for must not erase its own sentence.
    library.apply(&Update::Playlists(vec![
        playlist("a", "A"),
        playlist("b", "B"),
    ]));
    assert_eq!(sidebar_order(&library), names(&["a", "b"]));
    assert_eq!(
        library.sidebar_notice,
        Some(SidebarWrite::Remove),
        "the heal read cleared the notice it was asked for"
    );

    library.apply(&Update::RootlistAccess(rootlist_access(0x09, true)));
    assert!(
        library
            .remove_from_library(&item(SidebarKind::Playlists, "a"))
            .is_some()
    );
    assert_eq!(
        library.sidebar_notice, None,
        "the next write kept the old sentence"
    );
}

#[test]
fn a_stale_removal_reply_is_ignored() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, None);
    assert!(
        library
            .remove_from_library(&item(SidebarKind::Playlists, "a"))
            .is_some()
    );

    library.apply(&Update::PlaylistRemoved {
        playlist: "b".to_owned(),
        revision: revision(0x09),
    });
    assert_eq!(sidebar_order(&library), names(&["a", "b"]));
    assert!(
        library.write_in_flight(),
        "another row's reply ended the write"
    );

    let mut idle = sidebar_library(&["a"], 0x07, true, None);
    idle.apply(&Update::PlaylistRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x09),
    });
    assert_eq!(sidebar_order(&idle), names(&["a"]));
}

#[test]
fn an_album_or_a_show_removal_is_confirmed_by_its_reply() {
    for (kind, uri, dropped) in [
        (SidebarKind::Albums, "spotify:album:r", 0),
        (SidebarKind::Podcasts, "spotify:show:s", 2),
    ] {
        let mut library = saved_library();
        assert_eq!(
            library.remove_from_library(&item(kind, uri)),
            Some(Command::SetSaved {
                uri: uri.to_owned(),
                saved: false,
            }),
            "{kind:?}"
        );
        assert_eq!(
            sidebar_order(&library),
            names(&[]),
            "{kind:?} changed its list before the write was confirmed"
        );

        library.apply(&Update::Saved {
            uri: uri.to_owned(),
            saved: false,
        });

        let left = match dropped {
            0 => match &library.saved_albums {
                Load::Ready(albums) => albums.len(),
                _ => usize::MAX,
            },
            _ => match &library.followed {
                Load::Ready(shows) => shows.len(),
                _ => usize::MAX,
            },
        };
        assert_eq!(left, 0, "{kind:?} kept its row");
        assert!(!library.write_in_flight());
        assert_eq!(library.sidebar_notice, None);
    }
}
