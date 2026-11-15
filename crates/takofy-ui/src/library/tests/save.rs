use super::super::*;
use super::test_support::*;

#[test]
fn a_save_asks_for_the_write_and_is_confirmed_by_its_echo() {
    let mut library = saved_library();
    // An album the library does not have: the page's band offers the heart
    // for it, and the list is ready, so the answer is a known "not saved".
    assert_eq!(library.is_saved("spotify:album:new"), Some(false));
    assert_eq!(
        library.save_to_library(&item(SidebarKind::Albums, "spotify:album:new")),
        Some(Command::SetSaved {
            uri: "spotify:album:new".to_owned(),
            saved: true,
        })
    );
    assert!(library.saved_pending("spotify:album:new", true));
    assert_eq!(
        library.is_saved("spotify:album:new"),
        Some(false),
        "the list has not changed yet"
    );

    library.apply(&Update::Saved {
        uri: "spotify:album:new".to_owned(),
        saved: true,
    });

    assert_eq!(
        library.is_saved("spotify:album:new"),
        Some(true),
        "the echo outlives the list an add cannot join"
    );
    assert!(!library.write_in_flight());
    assert_eq!(library.sidebar_notice, None);
}

/// A saved album reaches the sidebar by re-reading the list that holds it: an
/// add names no card, so the list cannot gain the row on its own.
/// An artist page draws a heart on each of its releases, and only the whole
/// set can say what they are, so the page asks for it.
/// A heart carries the value it was pressed into, so the library is told the
/// direction instead of inferring it from a state that may have moved.
#[test]
fn a_heart_that_asks_to_save_writes_one_save() {
    let mut library = saved_library();
    let item = item(SidebarKind::Albums, "spotify:album:new");

    assert_eq!(
        library.set_saved(&item, true),
        Some(Command::SetSaved {
            uri: "spotify:album:new".to_owned(),
            saved: true,
        }),
        "an unsaved album's heart must save it"
    );
}

#[test]
fn a_heart_that_asks_to_unsave_writes_one_removal() {
    let mut library = saved_library();
    let item = item(SidebarKind::Albums, "spotify:album:new");
    library.apply(&Update::SavedAlbums(vec![saved_album("spotify:album:new")]));
    assert_eq!(library.is_saved("spotify:album:new"), Some(true));

    assert_eq!(
        library.set_saved(&item, false),
        Some(Command::SetSaved {
            uri: "spotify:album:new".to_owned(),
            saved: false,
        }),
        "a saved album's heart asked to save it again"
    );
}

#[test]
fn an_artist_page_wants_the_saved_albums() {
    let mut library = Library::default();
    library.apply(&Update::LibraryReady);
    library.apply(&Update::Playlists(Vec::new()));
    library.open(Page::Artist("spotify:artist:a".to_owned()));

    assert_eq!(
        library.saved_albums_request(),
        Some(Command::LoadSavedAlbums),
        "an artist page's hearts have nothing to read"
    );
}

#[test]
fn a_playlist_page_does_not_want_the_saved_albums() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist(
        "spotify:playlist:a",
        "Alpha",
    )]));
    library
        .root_filter
        .choose(crate::library_filter::RootKind::Playlists);
    library.open(Page::Playlist("spotify:playlist:a".to_owned()));

    assert_eq!(
        library.saved_albums_request(),
        None,
        "a page with no hearts on it asked for a set it does not draw"
    );
}

#[test]
fn a_saved_album_re_arms_the_list_that_would_hold_it() {
    let mut library = saved_library();
    assert!(matches!(library.saved_albums, Load::Ready(_)));
    library.save_to_library(&item(SidebarKind::Albums, "spotify:album:new"));
    library.apply(&Update::Saved {
        uri: "spotify:album:new".to_owned(),
        saved: true,
    });

    assert_eq!(
        library.saved_albums,
        Load::Idle,
        "the list was not re-armed, so nothing would read it again"
    );
    assert_eq!(library.is_saved("spotify:album:new"), Some(true));
}

#[test]
fn a_saved_show_re_arms_the_shows_read() {
    let mut library = saved_library();
    library.save_to_library(&item(SidebarKind::Podcasts, "spotify:show:new"));
    library.apply(&Update::Saved {
        uri: "spotify:show:new".to_owned(),
        saved: true,
    });
    assert_eq!(library.followed, Load::Idle);
}

#[test]
fn a_followed_artist_re_arms_the_artist_read() {
    let mut library = Library::default();
    library.apply(&Update::FollowedArtists(Vec::new()));
    library.open(crate::route::Page::Artist("spotify:artist:new".to_owned()));
    library.apply(&Update::Artist(artist_detail("spotify:artist:new")));
    library.apply(&Update::Followed {
        artist: "spotify:artist:new".to_owned(),
        followed: false,
    });
    // The press is what makes the answer ours: the page's own probe carries
    // the same shape and must not re-arm anything.
    assert!(library.follow.pressed().is_some());

    library.apply(&Update::Followed {
        artist: "spotify:artist:new".to_owned(),
        followed: true,
    });
    assert!(
        matches!(library.followed_artists, Load::Ready(_)),
        "the rows stay while the read that names the new artist is in flight"
    );
}

/// The page's own press has to reach the list the sidebar draws; the page's
/// probe arrives in the same shape and must not.
#[test]
fn a_press_of_follow_asks_for_the_artist_list_again() {
    let mut library = Library::default();
    library.apply(&Update::LibraryReady);
    library.apply(&Update::Playlists(Vec::new()));
    library.apply(&Update::FollowedArtists(Vec::new()));
    library
        .sidebar_kind
        .choose(crate::sidebar_kind::SidebarKind::Artists);
    library.open(Page::Artist("spotify:artist:a".to_owned()));
    library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
    library.apply(&Update::Followed {
        artist: "spotify:artist:a".to_owned(),
        followed: false,
    });
    assert!(library.follow.pressed().is_some(), "arrange: the press");

    library.apply(&Update::Followed {
        artist: "spotify:artist:a".to_owned(),
        followed: true,
    });

    assert_eq!(
        library.followed_artists_request(),
        Some(Command::LoadFollowedArtists),
        "a follow the user asked for must reach the list"
    );
}

/// An unfollow names a row the list already holds, so the row goes at once.
#[test]
fn a_press_of_unfollow_drops_the_artist_from_the_list() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(Vec::new()));
    library.apply(&Update::FollowedArtists(vec![followed_artist(
        "spotify:artist:a",
    )]));
    library
        .sidebar_kind
        .choose(crate::sidebar_kind::SidebarKind::Artists);
    library.open(Page::Artist("spotify:artist:a".to_owned()));
    library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
    library.apply(&Update::Followed {
        artist: "spotify:artist:a".to_owned(),
        followed: true,
    });
    assert_eq!(
        library.follow.pressed(),
        Some(Command::SetFollow {
            artist: "spotify:artist:a".to_owned(),
            followed: false,
        }),
        "arrange: the press unfollows"
    );

    library.apply(&Update::Followed {
        artist: "spotify:artist:a".to_owned(),
        followed: false,
    });

    match &library.followed_artists {
        Load::Ready(artists) => assert!(artists.is_empty(), "the row stayed"),
        other => panic!("the list was dropped: {other:?}"),
    }
}

/// An answer that agrees with the list is not a reason to read it again, so a
/// page's opening probe costs nothing.
#[test]
fn a_probe_that_agrees_with_the_list_re_arms_nothing() {
    let mut library = Library::default();
    library.apply(&Update::FollowedArtists(vec![followed_artist(
        "spotify:artist:new",
    )]));
    library.open(crate::route::Page::Artist("spotify:artist:new".to_owned()));
    library.apply(&Update::Artist(artist_detail("spotify:artist:new")));

    library.apply(&Update::Followed {
        artist: "spotify:artist:new".to_owned(),
        followed: true,
    });

    assert!(
        matches!(library.followed_artists, Load::Ready(_)),
        "a list that already holds the artist was re-read for nothing"
    );
}

/// The opposite disagreement is a stale set, and only a read fixes it.
#[test]
fn a_probe_that_disagrees_asks_for_the_list() {
    let mut library = Library::default();
    library.apply(&Update::FollowedArtists(Vec::new()));
    library.open(crate::route::Page::Artist("spotify:artist:new".to_owned()));
    library.apply(&Update::Artist(artist_detail("spotify:artist:new")));

    library.apply(&Update::Followed {
        artist: "spotify:artist:new".to_owned(),
        followed: true,
    });

    assert!(
        matches!(library.followed_artists, Load::Ready(_)),
        "the server says followed and the list does not hold it, so it is re-read"
    );
}

/// A band's `…` menu writes without opening the page's Follow state, so the
/// answer is all there is to go on.
#[test]
fn a_menu_unfollow_with_nothing_pending_drops_the_row() {
    let mut library = Library::default();
    library.apply(&Update::FollowedArtists(vec![followed_artist(
        "spotify:artist:a",
    )]));

    library.apply(&Update::Followed {
        artist: "spotify:artist:a".to_owned(),
        followed: false,
    });

    match &library.followed_artists {
        Load::Ready(artists) => assert!(artists.is_empty(), "the row stayed"),
        other => panic!("the list was dropped: {other:?}"),
    }
}

#[test]
fn a_menu_follow_with_nothing_pending_asks_for_the_list() {
    let mut library = Library::default();
    library.apply(&Update::FollowedArtists(Vec::new()));

    library.apply(&Update::Followed {
        artist: "spotify:artist:a".to_owned(),
        followed: true,
    });

    assert!(
        matches!(library.followed_artists, Load::Ready(_)),
        "the rows stay while the list is re-read for the artist's name"
    );
}

#[test]
fn a_probe_is_asked_for_when_no_list_can_answer() {
    let mut library = Library::default();
    // Nothing has been read, so no list can say whether this album is saved.
    library.open(crate::route::Page::Album("spotify:album:new".to_owned()));
    assert_eq!(library.is_saved("spotify:album:new"), None);

    assert_eq!(
        library.saved_request(),
        Some(Command::LoadSaved {
            uri: "spotify:album:new".to_owned(),
        })
    );
    assert_eq!(library.saved_request(), None, "one probe per page");

    library.apply(&Update::SavedState {
        uri: "spotify:album:new".to_owned(),
        saved: Some(true),
    });
    assert_eq!(library.is_saved("spotify:album:new"), Some(true));
    assert_eq!(library.saved_request(), None, "the answer ends the asking");
}

#[test]
fn a_probe_that_does_not_answer_leaves_the_state_unknown() {
    let mut library = Library::default();
    library.open(crate::route::Page::Album("spotify:album:new".to_owned()));
    library.saved_request();
    library.apply(&Update::SavedState {
        uri: "spotify:album:new".to_owned(),
        saved: None,
    });

    assert_eq!(
        library.is_saved("spotify:album:new"),
        None,
        "an unanswered probe claims nothing"
    );
}

#[test]
fn a_page_that_is_not_an_album_or_a_show_asks_nothing() {
    let mut library = Library::default();
    library.open(crate::route::Page::Playlist(
        "spotify:playlist:a".to_owned(),
    ));
    assert_eq!(library.saved_request(), None);
}

#[test]
fn a_failed_save_keeps_the_heart_off_and_says_so() {
    let mut library = saved_library();
    library.save_to_library(&item(SidebarKind::Albums, "spotify:album:new"));
    library.apply(&Update::SavedFailed {
        uri: "spotify:album:new".to_owned(),
        saved: true,
    });

    assert_eq!(library.is_saved("spotify:album:new"), Some(false));
    assert_eq!(library.sidebar_notice, Some(SidebarWrite::Save));
    assert!(!library.write_in_flight());
}

#[test]
fn an_item_the_library_has_is_not_saved_again() {
    let mut library = saved_library();
    assert_eq!(library.is_saved("spotify:album:r"), Some(true));
    assert_eq!(
        library.save_to_library(&item(SidebarKind::Albums, "spotify:album:r")),
        None
    );
    assert!(!library.write_in_flight());
}

#[test]
fn an_artist_save_does_not_wait_for_the_artist_list_to_be_ready() {
    for load in [Load::Loading, Load::Failed(Failure::FollowedArtists)] {
        let mut library = Library {
            followed_artists: load,
            ..Library::default()
        };
        assert_eq!(
            library.is_saved("spotify:artist:new"),
            Some(false),
            "an artist list the session asked for answers not saved"
        );
        assert_eq!(
            library.save_to_library(&item(SidebarKind::Artists, "spotify:artist:new")),
            Some(Command::SetFollow {
                artist: "spotify:artist:new".to_owned(),
                followed: true,
            }),
            "an artist save was refused while the list was not ready"
        );
        assert!(!library.write_in_flight(), "an artist save holds no write");
    }
}

#[test]
fn a_playlist_is_put_back_on_the_rootlist_by_a_link() {
    let mut library = sidebar_library(&["a"], 0x07, true, None);
    // A playlist the sidebar does not list: the only way one is missing from
    // the library is that it was unlinked, and a band then offers the heart.
    assert_eq!(library.is_saved("spotify:playlist:gone"), Some(false));
    assert_eq!(
        library.save_to_library(&item(SidebarKind::Playlists, "gone")),
        Some(Command::LinkPlaylist {
            playlist: "gone".to_owned(),
            revision: revision(0x07),
        })
    );
    assert!(library.write_in_flight(), "the link holds the list still");

    library.apply(&Update::PlaylistLinked {
        playlist: "gone".to_owned(),
        revision: revision(0x08),
    });

    assert!(!library.write_in_flight());
    assert_eq!(library.sidebar_notice, None);
    assert_eq!(
        library
            .rootlist
            .as_ref()
            .map(|access| access.revision.clone()),
        Some(revision(0x08)),
        "the next write names the revision the link left"
    );
}

#[test]
fn a_link_invents_no_row_and_asks_for_the_list() {
    let mut library = sidebar_library(&["a"], 0x07, true, None);
    assert!(
        library
            .save_to_library(&item(SidebarKind::Playlists, "gone"))
            .is_some()
    );

    library.apply(&Update::PlaylistLinked {
        playlist: "gone".to_owned(),
        revision: revision(0x08),
    });

    assert_eq!(
        sidebar_order(&library),
        names(&["a"]),
        "the reply names no row, so the sidebar cannot draw one from it"
    );
    assert_eq!(
        library.rootlist_request(),
        Some(Command::ReloadLibrary),
        "the linked row comes back by re-reading the list, not by inventing one"
    );
}

#[test]
fn a_failed_link_says_so_and_leaves_the_list_alone() {
    let mut library = sidebar_library(&["a"], 0x07, true, None);
    library.save_to_library(&item(SidebarKind::Playlists, "gone"));
    library.apply(&Update::PlaylistLinkFailed {
        playlist: "gone".to_owned(),
    });

    assert_eq!(library.sidebar_notice, Some(SidebarWrite::Save));
    assert!(!library.write_in_flight());
    assert_eq!(
        library
            .rootlist
            .as_ref()
            .map(|access| access.revision.clone()),
        Some(revision(0x07)),
        "a failed link keeps the revision it had"
    );
}

#[test]
fn a_failed_saved_removal_keeps_the_row_and_says_so() {
    let mut library = saved_library();
    assert!(
        library
            .remove_from_library(&item(SidebarKind::Albums, "spotify:album:r"))
            .is_some()
    );
    library.apply(&Update::SavedFailed {
        uri: "spotify:album:r".to_owned(),
        saved: false,
    });

    match &library.saved_albums {
        Load::Ready(albums) => assert_eq!(albums.len(), 1),
        other => panic!("the list was dropped: {other:?}"),
    }
    assert_eq!(library.sidebar_notice, Some(SidebarWrite::Remove));
    assert!(!library.write_in_flight());
}

#[test]
fn an_artist_removal_rides_the_unfollow_and_resets_the_pages_label() {
    let mut library = saved_library();
    library.open(Page::Artist("spotify:artist:a".to_owned()));
    library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
    library.apply(&Update::Followed {
        artist: "spotify:artist:a".to_owned(),
        followed: true,
    });
    assert_eq!(library.follow.label(), "Following");

    assert_eq!(
        library.remove_from_library(&item(SidebarKind::Artists, "spotify:artist:a")),
        Some(Command::SetFollow {
            artist: "spotify:artist:a".to_owned(),
            followed: false,
        })
    );
    library.apply(&Update::Followed {
        artist: "spotify:artist:a".to_owned(),
        followed: false,
    });

    match &library.followed_artists {
        Load::Ready(artists) => assert!(artists.is_empty(), "the row stayed"),
        other => panic!("the list was dropped: {other:?}"),
    }
    assert_eq!(library.follow.label(), "Follow");
    assert!(!library.write_in_flight());
}

#[test]
fn a_failed_artist_removal_keeps_the_row_and_says_so() {
    let mut library = saved_library();
    assert!(
        library
            .remove_from_library(&item(SidebarKind::Artists, "spotify:artist:a"))
            .is_some()
    );
    library.apply(&Update::FollowFailed {
        artist: "spotify:artist:a".to_owned(),
        failure: Failure::Save,
    });

    match &library.followed_artists {
        Load::Ready(artists) => assert_eq!(artists.len(), 1),
        other => panic!("the list was dropped: {other:?}"),
    }
    assert_eq!(library.sidebar_notice, Some(SidebarWrite::Remove));
    assert!(!library.write_in_flight());
}

#[test]
fn removing_the_last_row_leaves_an_empty_ready_list() {
    let mut library = sidebar_library(&["a"], 0x07, true, None);
    assert!(
        library
            .remove_from_library(&item(SidebarKind::Playlists, "a"))
            .is_some()
    );
    library.apply(&Update::PlaylistRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x09),
    });

    assert_eq!(library.playlists, Load::Ready(Vec::new()));
}

#[test]
fn a_playlists_read_lands_during_a_saved_removal_but_not_a_playlist_one() {
    let mut saved = saved_library();
    assert!(
        saved
            .remove_from_library(&item(SidebarKind::Albums, "spotify:album:r"))
            .is_some()
    );
    saved.apply(&Update::Playlists(vec![playlist("x", "X")]));
    assert_eq!(
        sidebar_order(&saved),
        names(&["x"]),
        "a collection removal dropped a playlists read"
    );

    let mut unlinking = sidebar_library(&["a"], 0x07, true, None);
    assert!(
        unlinking
            .remove_from_library(&item(SidebarKind::Playlists, "a"))
            .is_some()
    );
    unlinking.apply(&Update::Playlists(vec![playlist("x", "X")]));
    assert_eq!(sidebar_order(&unlinking), names(&["a"]));
    assert!(
        unlinking.rootlist_stale,
        "the dropped read owed a fresh one"
    );
}

#[test]
fn can_remove_is_the_librarys_state_per_kind() {
    let library = saved_library();
    assert!(
        !library.can_remove(SidebarKind::Playlists),
        "a rootlist with no revision offered an unlink"
    );
    for kind in [
        SidebarKind::Albums,
        SidebarKind::Artists,
        SidebarKind::Podcasts,
    ] {
        assert!(library.can_remove(kind), "{kind:?} is ready and idle");
    }

    let editable = sidebar_library(&["a"], 0x07, true, None);
    assert!(editable.can_remove(SidebarKind::Playlists));
    assert!(!editable.can_remove(SidebarKind::Albums));

    let mut writing = sidebar_library(&["a", "b"], 0x07, true, None);
    assert!(writing.move_playlist(0, 2, "a").is_some());
    for kind in crate::sidebar_kind::ROW {
        assert!(!writing.can_remove(kind), "{kind:?} during a write");
    }
}

/// A sidebar holding one list, and a page for another list the sidebar does
/// not name.
fn pasted_library() -> Library {
    let mut library = sidebar_library(&["spotify:playlist:mine"], 0x07, true, None);
    library.select("spotify:playlist:pasted");
    library.apply(&Update::Tracks {
        playlist: "spotify:playlist:pasted".to_owned(),
        tracks: vec![track("spotify:track:t0")],
        cover: None,
        access: None,
        head: Some(head("Pasted")),
    });
    library
}

/// The same page, with no rootlist revision ever read: the transitional and the
/// permanent inert heart both start here.
fn pasted_library_rootless() -> Library {
    let mut library = Library::default();
    library.apply(&Update::LibraryReady);
    library.apply(&Update::Playlists(vec![with_access(
        playlist("spotify:playlist:mine", "Mine"),
        0x07,
        true,
        None,
    )]));
    library.select("spotify:playlist:pasted");
    library.apply(&Update::Tracks {
        playlist: "spotify:playlist:pasted".to_owned(),
        tracks: vec![track("spotify:track:t0")],
        cover: None,
        access: None,
        head: Some(head("Pasted")),
    });
    library
}

/// The item the band's menu and heart act on for the pasted page.
fn pasted_item() -> LibraryItem {
    item(SidebarKind::Playlists, "spotify:playlist:pasted")
}

#[test]
fn an_unlisted_page_offers_the_heart_and_no_rename_or_remove() {
    let library = pasted_library();
    assert_eq!(
        library.is_saved("spotify:playlist:pasted"),
        Some(false),
        "arrange: the list is ready and does not hold it"
    );

    let rights = library.band_rights("spotify:playlist:pasted");
    assert!(
        !rights.renamable,
        "no sidebar row carries the access a rename needs"
    );
    assert!(!rights.removable, "nothing is listed to unlink");
    assert_eq!(rights.saved, Some(false), "the heart is live and empty");
}

#[test]
fn an_unlisted_page_is_not_removable_even_with_an_editable_rootlist() {
    let library = pasted_library();
    assert!(
        library.can_remove(SidebarKind::Playlists),
        "arrange: the rootlist may be written"
    );

    assert!(
        !library.band_rights("spotify:playlist:pasted").removable,
        "the page's list is not in the sidebar, so there is no row to unlink"
    );
}

#[test]
fn the_heart_is_inert_until_the_rootlist_is_read() {
    let library = pasted_library_rootless();
    assert_eq!(
        library.is_saved("spotify:playlist:pasted"),
        Some(false),
        "the list still knows it is missing"
    );
    assert_eq!(
        library.band_saved("spotify:playlist:pasted"),
        None,
        "a link needs a revision to name, so the heart waits for one"
    );
}

#[test]
fn the_heart_stays_inert_when_the_rootlist_is_never_readable() {
    let mut library = pasted_library_rootless();
    // A read that lists nothing, and never an access after it.
    library.apply(&Update::Playlists(Vec::new()));

    assert_eq!(
        library.is_saved("spotify:playlist:pasted"),
        Some(false),
        "the list is ready and does not hold the page"
    );
    assert_eq!(
        library.band_rights("spotify:playlist:pasted").saved,
        None,
        "the heart stays honest rather than claiming it can link"
    );
    assert_eq!(
        library.save_to_library(&pasted_item()),
        None,
        "and the press names no revision"
    );
}

#[test]
fn the_heart_waits_for_the_reply() {
    let mut library = pasted_library();
    assert_eq!(library.band_saved("spotify:playlist:pasted"), Some(false));
    assert_eq!(
        library.save_to_library(&pasted_item()),
        Some(Command::LinkPlaylist {
            playlist: "spotify:playlist:pasted".to_owned(),
            revision: revision(0x07),
        })
    );

    assert_eq!(
        library.band_saved("spotify:playlist:pasted"),
        None,
        "the press fills nothing until the reply"
    );
    assert_eq!(
        library.save_to_library(&pasted_item()),
        None,
        "and a second press sends no second link"
    );
}

#[test]
fn the_link_reply_fills_the_heart_before_the_list_is_re_read() {
    let mut library = pasted_library();
    assert!(library.save_to_library(&pasted_item()).is_some());
    library.apply(&Update::PlaylistLinked {
        playlist: "spotify:playlist:pasted".to_owned(),
        revision: revision(0x08),
    });

    assert_eq!(
        library.is_saved("spotify:playlist:pasted"),
        Some(true),
        "the reply is the whole confirmation a link gets"
    );
    assert_eq!(
        library.band_saved("spotify:playlist:pasted"),
        Some(true),
        "the heart fills without waiting for the row"
    );
    assert!(library.rootlist_stale, "the row still comes from a re-read");
}

#[test]
fn a_playlists_read_after_a_link_gives_the_sidebar_entry_its_rights() {
    let mut library = pasted_library();
    assert!(library.save_to_library(&pasted_item()).is_some());
    library.apply(&Update::PlaylistLinked {
        playlist: "spotify:playlist:pasted".to_owned(),
        revision: revision(0x08),
    });

    library.apply(&Update::Playlists(vec![with_access(
        playlist("spotify:playlist:pasted", "Pasted"),
        0x09,
        true,
        Some(1),
    )]));
    library.apply(&Update::RootlistAccess(rootlist_access(0x0a, true)));

    let rights = library.band_rights("spotify:playlist:pasted");
    assert!(
        rights.removable,
        "the row is listed and the rootlist is editable"
    );
    assert!(rights.renamable, "the entry's own access is editable");
    assert_eq!(rights.saved, Some(true), "the sidebar holds it");
    assert_eq!(
        library.band_source().map(|source| source.name),
        Some("Pasted".to_owned()),
        "the entry heads the page once the read lists it"
    );
}

#[test]
fn a_playlists_read_that_omits_the_list_supersedes_the_link_echo() {
    let mut library = pasted_library();
    assert!(library.save_to_library(&pasted_item()).is_some());
    library.apply(&Update::PlaylistLinked {
        playlist: "spotify:playlist:pasted".to_owned(),
        revision: revision(0x08),
    });
    assert_eq!(library.is_saved("spotify:playlist:pasted"), Some(true));

    // The server does not hold the link, so the echo that said it did is not
    // allowed to speak over the list that says otherwise.
    library.apply(&Update::Playlists(Vec::new()));
    library.apply(&Update::RootlistAccess(rootlist_access(0x0a, true)));

    assert_eq!(library.is_saved("spotify:playlist:pasted"), Some(false));
    assert_eq!(
        library.band_saved("spotify:playlist:pasted"),
        Some(false),
        "the heart empties again"
    );
}

#[test]
fn a_playlists_read_leaves_an_unparseable_echo_alone() {
    let mut library = pasted_library();
    library
        .saved_known
        .push(("not an address".to_owned(), true));
    library
        .saved_known
        .push(("spotify:playlist:pasted".to_owned(), true));

    library.apply(&Update::Playlists(Vec::new()));

    assert!(
        library
            .saved_known
            .iter()
            .any(|(known, _)| known == "not an address"),
        "a key that names no address is not a playlist's to drop: {:?}",
        library.saved_known
    );
    assert!(
        !library
            .saved_known
            .iter()
            .any(|(known, _)| known == "spotify:playlist:pasted"),
        "the playlist's own echo is gone: {:?}",
        library.saved_known
    );
}

#[test]
fn removing_a_playlist_forgets_its_saved_echo() {
    let mut library = pasted_library();
    assert!(library.save_to_library(&pasted_item()).is_some());
    library.apply(&Update::PlaylistLinked {
        playlist: "spotify:playlist:pasted".to_owned(),
        revision: revision(0x08),
    });
    // The re-read lists it, so a removal has a row to name.
    library.apply(&Update::Playlists(vec![with_access(
        playlist("spotify:playlist:pasted", "Pasted"),
        0x09,
        true,
        Some(1),
    )]));
    library.apply(&Update::RootlistAccess(rootlist_access(0x0a, true)));
    assert_eq!(
        library.is_saved("spotify:playlist:pasted"),
        Some(true),
        "arrange: the link's echo stands"
    );

    assert!(
        library.remove_from_library(&pasted_item()).is_some(),
        "arrange: the removal"
    );
    library.apply(&Update::PlaylistRemoved {
        playlist: "spotify:playlist:pasted".to_owned(),
        revision: revision(0x0b),
    });

    assert_eq!(
        library.is_saved("spotify:playlist:pasted"),
        Some(false),
        "the heart cannot stay filled for a list the library no longer holds"
    );
    assert_eq!(library.band_saved("spotify:playlist:pasted"), Some(false));
}

#[test]
fn a_removal_at_a_stale_index_still_forgets_the_echo() {
    // The reply cannot tell which row went, so it asks for a fresh read — but
    // the heart must stop claiming the library holds the list until then. The
    // echo came from a link, and only the removal clears it.
    let mut library = pasted_library();
    assert!(library.save_to_library(&pasted_item()).is_some());
    library.apply(&Update::PlaylistLinked {
        playlist: "spotify:playlist:pasted".to_owned(),
        revision: revision(0x08),
    });
    // The re-read lists it. Written straight in, because a read's own update
    // drops every playlist echo and the link's is the one under test.
    library.playlists = Load::Ready(vec![
        with_access(playlist("spotify:playlist:mine", "Mine"), 0x07, true, None),
        with_access(
            playlist("spotify:playlist:pasted", "Pasted"),
            0x08,
            true,
            Some(1),
        ),
    ]);
    assert_eq!(
        library.is_saved("spotify:playlist:pasted"),
        Some(true),
        "arrange: the link's echo stands"
    );
    assert!(
        library.remove_from_library(&pasted_item()).is_some(),
        "arrange: the removal"
    );
    // The list moved under the index the write named before the reply landed.
    library.playlists = Load::Ready(vec![with_access(
        playlist("spotify:playlist:mine", "Mine"),
        0x07,
        true,
        None,
    )]);

    library.apply(&Update::PlaylistRemoved {
        playlist: "spotify:playlist:pasted".to_owned(),
        revision: revision(0x0b),
    });

    assert!(
        library.rootlist_stale,
        "arrange: the removal owed a fresh read"
    );
    assert_eq!(
        library.is_saved("spotify:playlist:pasted"),
        Some(false),
        "the heart stayed filled for a list the library no longer holds"
    );
}
