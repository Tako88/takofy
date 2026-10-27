use super::super::*; // the module under test
use super::*; // the slice's shared fixtures, from tests/mod.rs

#[test]
fn movable_needs_an_editable_ready_page_and_no_write_in_flight() {
    assert!(!Library::default().movable(), "nothing is loaded");
    assert!(!page_with("a", &["t0"], None).movable(), "no access");
    assert!(!page_with("a", &["t0"], access(0x01, false)).movable());

    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
    assert!(library.movable());

    assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
    assert!(!library.movable(), "a write is in flight");
}

#[test]
fn reload_marks_the_playlists_loading_and_returns_the_command() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));

    assert_eq!(library.reload(), Command::ReloadLibrary);
    assert_eq!(library.playlists, Load::Loading);
}

#[test]
fn playlists_keep_the_tracks_when_the_selection_survives() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.select("a");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: None,
        access: None,
    });

    library.apply(&Update::Playlists(vec![
        playlist("a", "A"),
        playlist("b", "B"),
    ]));

    assert_eq!(
        library.tracks,
        Load::Ready(tracks_for("a", vec![track("t")]))
    );
    assert_eq!(library.selected(), Some("a"));
}

#[test]
fn playlists_clear_the_tracks_when_the_selection_is_gone() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.select("a");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: None,
        access: None,
    });

    library.apply(&Update::Playlists(vec![playlist("b", "B")]));

    assert_eq!(library.tracks, Load::Idle);
    assert_eq!(library.selected(), None);
}

#[test]
fn a_filter_is_cleared_when_the_page_changes() {
    let mut library = Library::default();
    library.select("a");
    library.track_filter.edit("a").push_str("midnight");

    library.select("b");
    assert!(
        library.track_filter.edit("a").is_empty(),
        "a new page clears the query"
    );

    library.track_filter.edit("b").push_str("midnight");
    library.back();
    assert!(
        library.track_filter.edit("b").is_empty(),
        "back clears the query"
    );

    library.track_filter.edit("b").push_str("midnight");
    library.forward();
    assert!(
        library.track_filter.edit("b").is_empty(),
        "forward clears the query"
    );

    library.track_filter.edit("b").push_str("midnight");
    library.to_playlists();
    assert!(
        library.track_filter.edit("b").is_empty(),
        "going home clears the query"
    );
}

#[test]
fn a_failed_tracks_load_clears_the_query_on_the_update() {
    let mut library = Library::default();
    library.select("a");
    library.track_filter.edit("a").push_str("midnight");

    library.apply(&Update::TracksFailed {
        playlist: "a".to_owned(),
        failure: Failure::Load,
    });

    assert_eq!(
        library.track_filter.needle("a"),
        None,
        "a failure that lands while another page is drawn still clears the query"
    );
}

#[test]
fn an_empty_tracks_reply_clears_the_query_on_the_update() {
    let mut library = Library::default();
    library.select("a");
    library.track_filter.edit("a").push_str("midnight");

    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: Vec::new(),
        cover: None,
        access: None,
    });

    assert_eq!(
        library.track_filter.needle("a"),
        None,
        "an emptied page has no field for the query to sit in"
    );
}

#[test]
fn a_rows_reply_keeps_the_query() {
    let mut library = Library::default();
    library.select("a");
    library.track_filter.edit("a").push_str("midnight");

    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: None,
        access: None,
    });

    assert_eq!(
        library.track_filter.needle("a").as_deref(),
        Some("midnight"),
        "a page with rows keeps what the field holds"
    );
}

#[test]
fn opening_the_create_field_clears_the_rail_query_on_the_update() {
    let mut library = Library::default();
    library.rail_filter.edit().push_str("midnight");

    library.begin_create();

    assert_eq!(
        library.rail_filter.query(),
        "",
        "a created playlist must not land in a list already hidden"
    );
    assert!(library.naming.is_open(), "the field opens with the clear");
}

#[test]
fn a_page_change_keeps_the_rail_query() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![
        playlist("a", "A"),
        playlist("b", "B"),
    ]));
    library.select("a");
    library.rail_filter.edit().push_str("midnight");

    library.select("b");
    assert_eq!(
        library.rail_filter.query(),
        "midnight",
        "a new playlist keeps the rail's query"
    );

    library.open(Page::Album("spotify:album:a".to_owned()));
    assert_eq!(library.rail_filter.query(), "midnight", "a page keeps it");

    library.back();
    assert_eq!(library.rail_filter.query(), "midnight", "back keeps it");

    library.forward();
    assert_eq!(library.rail_filter.query(), "midnight", "forward keeps it");

    library.to_playlists();
    assert_eq!(
        library.rail_filter.query(),
        "midnight",
        "going home keeps it"
    );
}

#[test]
fn a_playlists_reply_keeps_the_rail_query() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.rail_filter.edit().push_str("midnight");

    library.apply(&Update::Playlists(vec![playlist("a", "A")]));

    assert_eq!(
        library.rail_filter.query(),
        "midnight",
        "a reload of any library list keeps the query"
    );
}

#[test]
fn choosing_a_kind_keeps_the_rail_query() {
    let mut library = Library::default();
    library.rail_filter.edit().push_str("midnight");

    assert!(library.choose_kind(SidebarKind::Artists));

    assert_eq!(
        library.rail_filter.query(),
        "midnight",
        "the two axes are cumulative, so a chip keeps the query"
    );
}

#[test]
fn a_page_change_still_clears_the_track_filter() {
    let mut library = Library::default();
    library.select("a");
    library.track_filter.edit("a").push_str("midnight");
    library.rail_filter.edit().push_str("midnight");

    library.select("b");

    assert!(
        library.track_filter.edit("a").is_empty(),
        "the page's query still goes with the page"
    );
    assert_eq!(
        library.rail_filter.query(),
        "midnight",
        "the rail's query is not the page's"
    );
}

#[test]
fn reselecting_the_same_playlist_keeps_the_filter() {
    let mut library = Library::default();
    library.select("a");
    library.track_filter.edit("a").push_str("midnight");

    library.select("a");

    assert_eq!(
        library.track_filter.needle("a").as_deref(),
        Some("midnight"),
        "a reload of the same page is not a page change"
    );
}

#[test]
fn opening_another_show_starts_on_its_own_default() {
    let mut library = Library::default();
    library.open(show("spotify:show:a"));
    library.apply(&Update::Show(show_detail("spotify:show:a")));
    library.set_show_order(ShowOrder::Newest);

    library.open(show("spotify:show:b"));
    library.apply(&Update::Show(show_detail("spotify:show:b")));

    assert_eq!(
        library.show_order,
        ShowOrder::Newest,
        "the order is a view of one show's own list"
    );
}

#[test]
fn a_show_that_intends_the_oldest_first_opens_that_way() {
    let mut library = Library::default();
    library.open(show("spotify:show:a"));
    let mut detail = show_detail("spotify:show:a");
    let mut first = episode("spotify:episode:1");
    first.published = PublishedAt::from_millis(1_700_000_000_000);
    let mut last = episode("spotify:episode:2");
    last.published = PublishedAt::from_millis(1_700_000_900_000);
    detail.episodes = vec![first, last];
    library.apply(&Update::Show(detail));

    assert_eq!(
        library.show_order,
        ShowOrder::Oldest,
        "the chip opens naming the order the wire sent"
    );
}

#[test]
fn back_from_another_show_keeps_the_first_shows_sort() {
    let mut library = Library::default();
    library.open(show("spotify:show:a"));
    library.apply(&Update::Show(show_detail("spotify:show:a")));
    library.set_show_order(ShowOrder::Oldest);

    library.open(show("spotify:show:b"));
    library.apply(&Update::Show(show_detail("spotify:show:b")));
    let _ = library.back();

    assert_eq!(library.page(), Some(&show("spotify:show:a")));
    assert_eq!(
        library.show_order,
        ShowOrder::Oldest,
        "each show keeps the order it was left in"
    );
}

#[test]
fn back_to_the_same_show_keeps_the_sort() {
    let mut library = Library::default();
    library.open(show("spotify:show:a"));
    library.apply(&Update::Show(show_detail("spotify:show:a")));
    library.set_show_order(ShowOrder::Oldest);

    library.open(album("spotify:album:a"));
    assert_eq!(library.back(), None, "a ready show is already settled");

    assert_eq!(library.page(), Some(&show("spotify:show:a")));
    assert_eq!(
        library.show_order,
        ShowOrder::Oldest,
        "Back onto the show the choice was made on keeps it"
    );
}

#[test]
fn reopening_the_same_show_keeps_the_sort() {
    let mut library = Library::default();
    library.open(show("spotify:show:a"));
    library.apply(&Update::Show(show_detail("spotify:show:a")));
    library.set_show_order(ShowOrder::Oldest);

    library.open(show("spotify:show:a"));

    assert_eq!(
        library.show_order,
        ShowOrder::Oldest,
        "a reload of the same show is not a page change"
    );
}

#[test]
fn a_tracks_reply_for_the_same_playlist_keeps_the_filter() {
    let mut library = Library::default();
    library.select("a");
    library.track_filter.edit("a").push_str("midnight");

    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: None,
        access: None,
    });

    assert_eq!(
        library.track_filter.needle("a").as_deref(),
        Some("midnight")
    );
}

#[test]
fn a_filter_does_not_change_movable_or_removable() {
    let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
    assert!(library.movable() && library.removable());

    library.track_filter.edit("a").push_str("midnight");

    assert!(
        library.movable(),
        "the query is the view's gate, not this one"
    );
    assert!(library.removable());
}

#[test]
fn to_playlists_clears_the_selection_but_keeps_the_tracks() {
    let mut library = Library::default();
    library.select("a");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: None,
        access: None,
    });

    library.to_playlists();

    assert_eq!(library.selected(), None);
    assert_eq!(library.page(), None);
    assert!(library.can_back());
    assert_eq!(
        library.tracks,
        Load::Ready(tracks_for("a", vec![track("t")]))
    );
    assert_eq!(library.wanted, None, "the stale-reply guard is cleared");
}

#[test]
fn a_reply_after_to_playlists_is_discarded() {
    let mut library = Library::default();
    library.select("a");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t0")],
        cover: None,
        access: None,
    });
    library.to_playlists();
    let settled = Load::Ready(tracks_for("a", vec![track("t0")]));

    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t1")],
        cover: None,
        access: None,
    });
    assert_eq!(library.tracks, settled, "the late tracks are discarded");

    library.apply(&Update::TracksFailed {
        playlist: "a".to_owned(),
        failure: Failure::Load,
    });
    assert_eq!(library.tracks, settled, "the late failure is discarded");

    assert_eq!(
        library.back(),
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        }),
        "going back into the playlist reloads it"
    );
}
