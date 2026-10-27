use super::super::*;
use super::test_support::*;

#[test]
fn write_in_flight_is_any_pending_write() {
    let library = Library::default();
    assert!(!library.write_in_flight(), "an idle library has no write");

    let mut page = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
    assert!(move_one(&mut page, "a", 0, 2, &track("t0")).is_some());
    assert!(page.write_in_flight(), "a page move is a write");

    let mut sidebar = sidebar_library(&["a", "b", "c"], 0x07, true, None);
    assert!(sidebar.move_playlist(0, 2, "a").is_some());
    assert!(sidebar.write_in_flight(), "a sidebar write is a write");
}

#[test]
fn a_failed_like_reverts_and_the_healing_read_clears_the_sentence() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(Vec::new()));
    assert_eq!(library.likes_request(), Some(Command::LoadLikedTracks));
    library.apply(&Update::LikedTracks(Vec::new()));

    let command = library
        .set_liked(vec!["spotify:track:a".to_owned()], true)
        .expect("a write");
    assert_eq!(
        command,
        Command::SetLiked {
            tracks: vec!["spotify:track:a".to_owned()],
            liked: true,
        }
    );
    assert_eq!(library.likes.state("spotify:track:a"), Some(true));
    assert!(!library.write_in_flight(), "a like is not a playlist write");

    library.apply(&Update::LikeFailed {
        tracks: vec!["spotify:track:a".to_owned()],
        failure: Failure::Save,
    });
    assert_eq!(library.likes.state("spotify:track:a"), Some(false));
    assert_eq!(library.likes.notice(), Some(Failure::Save));
    assert_eq!(library.likes_request(), Some(Command::LoadLikedTracks));

    library.apply(&Update::LikedTracks(Vec::new()));
    assert_eq!(library.likes.notice(), None);
}

#[test]
fn a_like_confirmation_ends_the_write_and_leaves_the_heart_on() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(Vec::new()));
    assert_eq!(library.likes_request(), Some(Command::LoadLikedTracks));
    library.apply(&Update::LikedTracks(Vec::new()));
    let track = "spotify:track:a".to_owned();
    assert!(
        library.set_liked(vec![track.clone()], true).is_some(),
        "arrange: the like must be in flight"
    );
    // A sweep asked for after the click is answered after it: it carries the
    // set from before the write, so adopting it would wipe the confirmed like.
    let _ = library.reload();
    library.apply(&Update::Playlists(Vec::new()));
    assert_eq!(
        library.likes_request(),
        Some(Command::LoadLikedTracks),
        "arrange: the sweep must be in flight"
    );

    library.apply(&Update::Liked {
        tracks: vec![track.clone()],
        liked: true,
    });
    library.apply(&Update::LikedTracks(Vec::new()));

    assert_eq!(
        library.likes.state(&track),
        Some(true),
        "a confirmed like outlives a read issued before its write landed"
    );
    assert!(
        library.set_liked(vec![track], false).is_some(),
        "the confirmation never ended the write, so the heart could not be flipped again"
    );
}

#[test]
fn a_failure_sentence_does_not_survive_a_page_change() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(Vec::new()));
    assert_eq!(library.likes_request(), Some(Command::LoadLikedTracks));
    library.apply(&Update::LikedTracks(Vec::new()));
    assert!(
        library
            .set_liked(vec!["spotify:track:a".to_owned()], true)
            .is_some()
    );
    library.apply(&Update::LikeFailed {
        tracks: vec!["spotify:track:a".to_owned()],
        failure: Failure::Save,
    });
    assert_eq!(library.likes.notice(), Some(Failure::Save));

    let _ = library.open(Page::Album("spotify:album:b".to_owned()));
    assert_eq!(
        library.likes.notice(),
        None,
        "the sentence names a page no longer on screen"
    );

    assert!(
        library
            .set_liked(vec!["spotify:track:a".to_owned()], true)
            .is_some()
    );
    library.apply(&Update::LikeFailed {
        tracks: vec!["spotify:track:a".to_owned()],
        failure: Failure::Save,
    });
    assert_eq!(library.likes.notice(), Some(Failure::Save));
    library.to_playlists();
    assert_eq!(
        library.likes.notice(),
        None,
        "the grid is not the page the like came from"
    );
}

#[test]
fn a_loading_list_refuses_a_move() {
    let mut library = Library::default();
    library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));
    assert_eq!(library.move_playlist(0, 2, "a"), None);

    library.playlists = Load::Loading;
    assert!(!library.rootlist_movable());
}

#[test]
fn the_reply_replaces_the_revision_and_reorders_nothing() {
    let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, None);
    assert!(library.move_playlist(0, 2, "a").is_some());

    library.apply(&Update::PlaylistMoved {
        revision: revision(0x08),
    });

    assert_eq!(
        sidebar_order(&library),
        names(&["b", "a", "c"]),
        "the order already moved at drop time"
    );
    assert!(library.rootlist_movable(), "the write is answered");
    assert_eq!(
        library.move_playlist(0, 3, "b"),
        Some(Command::MovePlaylist {
            revision: revision(0x08),
            from: 0,
            to: 3,
        }),
        "the next move names the reply's revision"
    );
}

#[test]
fn a_failed_move_restores_the_order_before_the_drop_and_says_so() {
    let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, None);
    assert!(library.move_playlist(0, 2, "a").is_some());

    library.apply(&Update::PlaylistMoveFailed);

    assert_eq!(sidebar_order(&library), names(&["a", "b", "c"]));
    assert_eq!(library.sidebar_notice, Some(SidebarWrite::Reorder));
    assert!(
        library.rootlist_movable(),
        "the held revision is kept, so another drag may try again"
    );
    assert_eq!(
        library.move_playlist(0, 2, "a"),
        Some(Command::MovePlaylist {
            revision: revision(0x07),
            from: 0,
            to: 2,
        }),
        "the revision an unanswered write used is still the one held"
    );
}

#[test]
fn a_playlists_read_while_a_move_is_pending_is_not_adopted() {
    let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, None);
    assert!(library.move_playlist(0, 2, "a").is_some());

    library.apply(&Update::Playlists(vec![
        with_access(playlist("a", "a"), 0x07, true, None),
        with_access(playlist("b", "b"), 0x07, true, None),
        with_access(playlist("c", "c"), 0x07, true, None),
    ]));
    library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));

    assert_eq!(
        sidebar_order(&library),
        names(&["b", "a", "c"]),
        "a read taken before the drop would otherwise undo it"
    );
}

#[test]
fn a_rootlist_access_read_while_a_move_is_pending_is_not_adopted() {
    let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, None);
    assert!(library.move_playlist(0, 2, "a").is_some());

    library.apply(&Update::RootlistAccess(rootlist_access(0x09, true)));
    library.apply(&Update::PlaylistMoveFailed);

    assert_eq!(
        library.move_playlist(0, 2, "a"),
        Some(Command::MovePlaylist {
            revision: revision(0x07),
            from: 0,
            to: 2,
        }),
        "the access that arrived mid-write never became the held one"
    );
}

#[test]
fn a_playlists_read_after_the_reply_is_adopted() {
    let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, None);
    assert!(library.move_playlist(0, 2, "a").is_some());
    library.apply(&Update::PlaylistMoved {
        revision: revision(0x08),
    });

    library.apply(&Update::Playlists(vec![with_access(
        playlist("z", "z"),
        0x09,
        true,
        None,
    )]));

    assert_eq!(sidebar_order(&library), names(&["z"]));
}

#[test]
fn a_playlists_read_clears_the_held_access_until_its_own_arrives() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, None);
    assert!(library.rootlist_movable());

    library.apply(&Update::Playlists(vec![
        with_access(playlist("a", "a"), 0x07, true, None),
        with_access(playlist("b", "b"), 0x07, true, None),
    ]));

    assert!(
        !library.rootlist_movable(),
        "the pair cannot straddle: the list waits for its own access"
    );

    library.apply(&Update::RootlistAccess(rootlist_access(0x09, true)));
    assert!(library.rootlist_movable());
}

#[test]
fn an_adopted_read_clears_the_sidebar_notice() {
    let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, None);
    assert!(library.move_playlist(0, 2, "a").is_some());
    library.apply(&Update::PlaylistMoveFailed);
    assert_eq!(library.sidebar_notice, Some(SidebarWrite::Reorder));

    library.apply(&Update::Playlists(vec![with_access(
        playlist("a", "a"),
        0x07,
        true,
        None,
    )]));

    assert_eq!(library.sidebar_notice, None);
}

#[test]
fn a_new_gesture_clears_the_sidebar_notice_at_once() {
    let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, Some(2));
    assert!(library.move_playlist(0, 2, "a").is_some());
    library.apply(&Update::PlaylistMoveFailed);
    assert_eq!(library.sidebar_notice, Some(SidebarWrite::Reorder));

    assert!(library.move_playlist(0, 2, "a").is_some());

    assert_eq!(library.sidebar_notice, None);
}

#[test]
fn a_confirmed_write_after_a_failed_one_clears_the_sidebar_notice() {
    let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, Some(2));
    assert!(library.move_playlist(0, 2, "a").is_some());
    library.apply(&Update::PlaylistMoveFailed);
    assert!(library.move_playlist(0, 2, "a").is_some());

    library.apply(&Update::PlaylistMoved {
        revision: revision(0x08),
    });

    assert_eq!(library.sidebar_notice, None);
}
