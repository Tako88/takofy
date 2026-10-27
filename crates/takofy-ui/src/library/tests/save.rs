use super::super::*; // the module under test
use super::*; // the slice's shared fixtures, from tests/mod.rs

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
    assert_eq!(library.followed_artists, Load::Idle);
}

/// The page's own press has to reach the list the sidebar draws; the page's
/// probe arrives in the same shape and must not.
#[test]
fn a_press_of_follow_asks_for_the_artist_list_again() {
    let mut library = Library::default();
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

    assert_eq!(
        library.followed_artists,
        Load::Idle,
        "the server says followed and the list does not hold it"
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

    assert_eq!(library.followed_artists, Load::Idle);
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
