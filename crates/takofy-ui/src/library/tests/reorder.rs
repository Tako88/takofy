use super::super::*;
use super::test_support::*;

#[test]
fn tracks_before_any_selection_are_discarded() {
    let mut library = Library::default();
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: None,
        access: None,
    });
    assert_eq!(library.tracks, Load::Idle);
}

#[test]
fn a_downward_drag_names_the_gap_after_its_own_row() {
    let mut library = sidebar_library(&["a", "b", "c", "d"], 0x07, true, None);

    let found = library.move_playlist(0, 2, "a");

    assert_eq!(
        found,
        Some(Command::MovePlaylist {
            revision: revision(0x07),
            from: 0,
            to: 2,
        })
    );
    assert_eq!(
        sidebar_order(&library),
        names(&["b", "a", "c", "d"]),
        "the gap after b is where a lands"
    );
}

#[test]
fn a_downward_drag_to_the_very_end_names_the_gap_at_the_length() {
    let mut library = sidebar_library(&["a", "b", "c", "d"], 0x07, true, None);

    let found = library.move_playlist(1, 4, "b");

    assert_eq!(
        found,
        Some(Command::MovePlaylist {
            revision: revision(0x07),
            from: 1,
            to: 4,
        })
    );
    assert_eq!(sidebar_order(&library), names(&["a", "c", "d", "b"]));
}

#[test]
fn an_upward_drag_names_the_gap_it_lands_in() {
    let mut library = sidebar_library(&["a", "b", "c", "d"], 0x07, true, None);

    let found = library.move_playlist(3, 0, "d");

    assert_eq!(
        found,
        Some(Command::MovePlaylist {
            revision: revision(0x07),
            from: 3,
            to: 0,
        })
    );
    assert_eq!(sidebar_order(&library), names(&["d", "a", "b", "c"]));
}

#[test]
fn a_drag_beside_itself_sends_nothing() {
    let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, None);

    for boundary in [1, 2] {
        assert_eq!(
            library.move_playlist(1, boundary, "b"),
            None,
            "the gap {boundary} is where b already is"
        );
    }
    assert_eq!(sidebar_order(&library), names(&["a", "b", "c"]));
    assert!(library.rootlist_movable(), "and nothing was written");
}

#[test]
fn a_boundary_past_the_end_is_refused() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, None);
    assert_eq!(library.move_playlist(0, 3, "a"), None);
}

#[test]
fn a_move_names_the_held_revision() {
    let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, None);
    library.apply(&Update::RootlistAccess(rootlist_access(0x09, true)));

    assert_eq!(
        library.move_playlist(0, 2, "a"),
        Some(Command::MovePlaylist {
            revision: revision(0x09),
            from: 0,
            to: 2,
        })
    );
}

#[test]
fn no_rootlist_access_refuses_a_move() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![
        playlist("a", "a"),
        playlist("b", "b"),
    ]));

    assert!(!library.rootlist_movable());
    assert_eq!(library.move_playlist(0, 2, "a"), None);
    assert_eq!(sidebar_order(&library), names(&["a", "b"]));
}

#[test]
fn a_not_faithful_rootlist_refuses_a_move() {
    let mut library = sidebar_library(&["a", "b"], 0x07, false, None);

    assert!(!library.rootlist_movable());
    assert_eq!(library.move_playlist(0, 2, "a"), None);
}

#[test]
fn a_playlist_that_is_not_where_the_drag_found_it_is_refused() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, None);

    assert_eq!(
        library.move_playlist(0, 2, "b"),
        None,
        "the row holds another playlist now"
    );
    assert_eq!(library.move_playlist(9, 0, "a"), None, "no such row");
    assert_eq!(sidebar_order(&library), names(&["a", "b"]));
}

#[test]
fn a_move_waits_while_a_write_is_pending() {
    let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, None);
    assert!(library.move_playlist(0, 2, "a").is_some());

    assert_eq!(library.move_playlist(0, 3, "b"), None);
    assert!(!library.rootlist_movable());
}

#[test]
fn a_move_waits_while_a_page_move_is_pending() {
    let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
    library.playlists = Load::Ready(vec![
        with_access(playlist("p0", "p0"), 0x07, true, None),
        with_access(playlist("p1", "p1"), 0x07, true, None),
    ]);
    library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));
    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

    assert_eq!(library.move_playlist(0, 2, "p0"), None);
}

#[test]
fn a_page_move_waits_while_a_sidebar_write_is_pending() {
    let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
    library.playlists = Load::Ready(vec![
        with_access(playlist("p0", "p0"), 0x07, true, None),
        with_access(playlist("p1", "p1"), 0x07, true, None),
    ]);
    library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));
    assert!(library.move_playlist(0, 2, "p0").is_some());

    assert_eq!(move_one(&mut library, "a", 0, 2, &track("t0")), None);
    assert!(!library.movable());
}
