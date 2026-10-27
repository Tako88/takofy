use super::super::*;
use super::test_support::*;
use crate::library::Load;
use crate::message::Update;
use crate::player::Transport;
use crate::queue::Queue;
use crate::selection::Press;
use crate::theme::space;
use takofy_model::{PlaylistAccess, PlaylistEntry, TrackEntry};

pub(super) const TRACK_B: &str = "spotify:track:7ouMYWpwJ422jRcDASZB7P";

#[test]
fn dropping_a_track_on_a_playlist_row_sends_one_add() {
    let mut transport = playing(cover("abc"));
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    page_of_tracks(&mut window, &[TRACK_A]);
    sidebar_playlists(
        &mut window,
        &[("A", Some(2)), ("B", Some(3))],
        Some(rootlist_access(true)),
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), TRACK_A);
    let to = sidebar_row_rect(&window, &frame, "B", "Playlist · 3 tracks").center();

    assert_eq!(
        window.drag(&mut transport, &mut queue, from, to),
        Some(Command::AddTracks {
            playlist: "playlist:B".to_owned(),
            revision: revision("root-1"),
            from_index: 3,
            tracks: vec![TRACK_A.to_owned()],
        }),
        "the length is the index an append names"
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        text_in(&frame, "Playlist · 3 tracks", sidebar_rect(&window)).is_some(),
        "the count waits for the reply"
    );
}

#[test]
fn a_confirmed_add_counts_one_more_on_the_row() {
    let mut transport = playing(cover("abc"));
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    page_of_tracks(&mut window, &[TRACK_A]);
    sidebar_playlists(
        &mut window,
        &[("A", Some(2)), ("B", Some(3))],
        Some(rootlist_access(true)),
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), TRACK_A);
    let to = sidebar_row_rect(&window, &frame, "B", "Playlist · 3 tracks").center();
    assert!(window.drag(&mut transport, &mut queue, from, to).is_some());

    window.library.apply(&Update::TrackAdded {
        playlist: "playlist:B".to_owned(),
        revision: revision("root-2"),
    });
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

    assert!(
        text_in(&frame, "Playlist · 4 tracks", sidebar_rect(&window)).is_some(),
        "the row answers the add"
    );
    assert_eq!(
        sidebar_order(&window),
        ["A", "B"],
        "an add reorders nothing"
    );
}

#[test]
fn a_failed_add_leaves_the_count_and_says_so() {
    let mut transport = playing(cover("abc"));
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    page_of_tracks(&mut window, &[TRACK_A]);
    sidebar_playlists(
        &mut window,
        &[("A", Some(2)), ("B", Some(3))],
        Some(rootlist_access(true)),
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), TRACK_A);
    let to = sidebar_row_rect(&window, &frame, "B", "Playlist · 3 tracks").center();
    assert!(window.drag(&mut transport, &mut queue, from, to).is_some());

    window.library.apply(&Update::AddFailed {
        playlist: "playlist:B".to_owned(),
    });
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

    assert!(text_in(&frame, "Playlist · 3 tracks", sidebar_rect(&window)).is_some());
    assert!(
        text_in(
            &frame,
            "Could not add those tracks. Try again.",
            sidebar_rect(&window)
        )
        .is_some(),
        "the list says what could not be done"
    );
}

#[test]
fn a_track_over_a_playlist_row_draws_the_fill_and_no_rule() {
    let tokens = Tokens::DARK;
    let mut transport = playing(cover("abc"));
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    page_of_tracks(&mut window, &[TRACK_A]);
    sidebar_playlists(
        &mut window,
        &[("A", Some(2)), ("B", Some(3))],
        Some(rootlist_access(true)),
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), TRACK_A);
    let row = sidebar_row_rect(&window, &frame, "B", "Playlist · 3 tracks");
    let to = row.center();

    let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);

    assert!(
        frame
            .shapes
            .iter()
            .any(|clipped| matches!(&clipped.shape, egui::Shape::Rect(shape)
            if shape.fill == tokens.palette.row_selected
                && (shape.rect.top() - row.top()).abs() < 1.0)),
        "the row under the pointer takes the fill"
    );
    assert!(marker(&frame).is_none(), "an add draws no rule");
}

/// A pick of one real track, which an add can take whole.
fn a_pick(uri: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: "A Title".to_owned(),
        artists: "A Band".to_owned(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

/// The column's offer of two drawn rows standing for true rows 3 and 7.
fn narrowed_column(drawn: f32) -> ColumnOffer {
    let zone = crate::landing::Zone {
        hit: Rect::from_min_size(Pos2::new(10.0, 200.0), Vec2::new(200.0, 8.0 * drawn)),
        list: Rect::from_min_size(Pos2::new(10.0, 200.0), Vec2::new(200.0, 8.0 * drawn)),
        row_h: drawn,
        first: 0,
        count: 2,
    };
    ColumnOffer {
        zone,
        drawn,
        rows: Some(vec![3, 7]),
    }
}

/// Eight playlists, all of them able to take a track.
fn eight_playlists(window: &mut Window) {
    sidebar_playlists(
        window,
        &[
            ("a", Some(0)),
            ("b", Some(1)),
            ("c", Some(2)),
            ("d", Some(3)),
            ("e", Some(4)),
            ("f", Some(5)),
            ("g", Some(6)),
            ("h", Some(7)),
        ],
        Some(rootlist_access(true)),
    );
}

#[test]
fn a_narrowed_offer_adds_to_the_true_playlist() {
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    eight_playlists(&mut window);
    let column = narrowed_column(sidebar::ROW_H);
    let carry = Carry {
        entries: vec![a_pick(TRACK_A)],
        rows: vec![0],
        ..carried_rows(egui::Id::unique("list"), vec![0])
    };
    // The pointer is over the second row the column drew.
    let pointer = Some(Pos2::new(
        column.zone.list.left() + 20.0,
        column.zone.list.top() + column.zone.row_h * 1.5,
    ));

    let target = add_target(&carry, Some(&column), &window.library, pointer)
        .expect("the row under the pointer takes the add");

    let Target::Add(slot) = target else {
        panic!("a drop on a row is an add, not {target:?}");
    };
    assert_eq!(
        slot.row, 7,
        "the drawn slot names the playlist it stands for"
    );
    let drawn =
        crate::landing::slot_at(pointer, Some(&column.zone), column.drawn).expect("the drawn row");
    assert_eq!(
        slot.rect, drawn.rect,
        "the highlight stays on the row that was drawn"
    );
}

#[test]
fn a_narrowed_offer_refuses_a_rootlist_move() {
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    eight_playlists(&mut window);
    let column = narrowed_column(sidebar::ROW_H);
    let carry = Carry {
        rows: vec![3],
        playlist: Some("playlist:d".to_owned()),
        ..carried_rows(egui::Id::unique("list"), vec![0])
    };
    // Between the two drawn rows: a boundary a reorder would land on.
    let pointer = Some(Pos2::new(
        column.zone.list.left() + 20.0,
        column.zone.list.top() + column.zone.row_h + 1.0,
    ));

    assert_eq!(
        rootlist_target(&carry, Some(&column), &window.library, pointer),
        None,
        "a narrowed column would move the wrong playlist"
    );
}

#[test]
fn a_row_that_can_take_a_track_is_offered_even_when_the_rootlist_is_not() {
    let tokens = Tokens::DARK;
    let mut transport = playing(cover("abc"));
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    page_of_tracks(&mut window, &[TRACK_A]);
    // The rows name addable playlists of their own; the rootlist's read says
    // the order is not the list's to change, which only a move needs.
    let entry = |name: &str, tracks: usize| PlaylistEntry {
        composed: Vec::new(),
        uri: format!("playlist:{name}"),
        name: name.to_owned(),
        named: true,
        cover: None,
        description: None,
        owner: None,
        tracks: Some(tracks),
        access: Some(rootlist_access(true)),
    };
    window.library.playlists = Load::Ready(vec![entry("A", 2), entry("B", 3)]);
    window
        .library
        .apply(&Update::RootlistAccess(rootlist_access(false)));
    assert!(!window.library.rootlist_movable());
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), TRACK_A);
    let row = sidebar_row_rect(&window, &frame, "B", "Playlist · 3 tracks");
    let to = row.center();

    let (_, held) = window.hold_drag(&mut transport, &mut queue, from, to);
    assert!(
        held.shapes
            .iter()
            .any(|clipped| matches!(&clipped.shape, egui::Shape::Rect(shape)
            if shape.fill == tokens.palette.row_selected
                && (shape.rect.top() - row.top()).abs() < 1.0)),
        "the row under the pointer takes the fill"
    );
    assert!(marker(&held).is_none(), "an add draws no rule");

    let (command, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);
    assert_eq!(
        command,
        Some(Command::AddTracks {
            playlist: "playlist:B".to_owned(),
            revision: revision("root-1"),
            from_index: 3,
            tracks: vec![TRACK_A.to_owned()],
        }),
        "a row that can take the track takes it"
    );

    window.library.apply(&Update::TrackAdded {
        playlist: "playlist:B".to_owned(),
        revision: revision("root-2"),
    });
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        text_in(&frame, "Playlist · 4 tracks", sidebar_rect(&window)).is_some(),
        "the row answers the add"
    );

    // A playlist row still does not lift here: a carry only means a move.
    let from = row_line(&frame, sidebar_rect(&window), "A");
    let (_, held) = window.hold_drag(&mut transport, &mut queue, from, to);
    assert!(
        carry::carried(&window.ctx).is_none(),
        "a row order that is not the wire's does not lift"
    );
    assert!(marker(&held).is_none(), "and no rule is drawn");
}

#[test]
fn a_track_over_a_row_that_cannot_take_it_draws_nothing() {
    let tokens = Tokens::DARK;
    // The rootlist is movable in every case, so the column is offered and
    // only what the entry itself holds keeps it from taking the track.
    let cases: Vec<(&str, Option<usize>, Option<PlaylistAccess>)> = vec![
        ("not editable", Some(3), Some(rootlist_access(false))),
        ("no count", None, Some(rootlist_access(true))),
        ("no revision", Some(3), None),
    ];
    for (why, tracks, entry) in cases {
        let mut transport = playing(cover("abc"));
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        page_of_tracks(&mut window, &[TRACK_A]);
        window.library.playlists = Load::Ready(vec![PlaylistEntry {
            composed: Vec::new(),
            uri: "playlist:B".to_owned(),
            name: "B".to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks,
            access: entry,
        }]);
        window
            .library
            .apply(&Update::RootlistAccess(rootlist_access(true)));
        assert!(
            window.library.rootlist_movable(),
            "{why}: the column has to be offered"
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), TRACK_A);
        let to = sidebar_row_rect(&window, &frame, "B", "Playlist").center();

        let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);
        assert!(
            !frame
                .shapes
                .iter()
                .any(|clipped| matches!(&clipped.shape, egui::Shape::Rect(shape)
                if shape.fill == tokens.palette.row_selected
                    && sidebar_rect(&window).contains_rect(shape.rect))),
            "{why}: no fill"
        );
        assert!(marker(&frame).is_none(), "{why}: no rule");

        let (command, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);
        assert_eq!(command, None, "{why}: nothing is written");
    }
}

#[test]
fn dropping_on_the_open_playlists_own_row_adds_nothing() {
    let mut transport = playing(cover("abc"));
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    page_of_tracks(&mut window, &[TRACK_A]);
    sidebar_playlists(&mut window, &[("A", Some(2))], Some(rootlist_access(true)));
    // The sidebar's row is the page's own playlist, by its own URI.
    if let Load::Ready(rows) = &mut window.library.playlists {
        rows[0].uri = PLAYLIST.to_owned();
    }
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), TRACK_A);
    let to = sidebar_row_rect(&window, &frame, "A", "Playlist · 2 tracks").center();

    assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
}

#[test]
fn an_episode_dropped_on_a_row_is_refused() {
    let mut transport = playing(cover("abc"));
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    page_of_tracks(&mut window, &["spotify:episode:1"]);
    sidebar_playlists(&mut window, &[("A", Some(2))], Some(rootlist_access(true)));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), "spotify:episode:1");
    let to = sidebar_row_rect(&window, &frame, "A", "Playlist · 2 tracks").center();

    assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
}

#[test]
fn a_track_from_the_queue_can_be_added() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of_tracks(&[TRACK_A, TRACK_B], &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_playlists(&mut window, &[("A", Some(2))], Some(rootlist_access(true)));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, pane_rect_when_shown(&window), TRACK_B);
    let to = sidebar_row_rect(&window, &frame, "A", "Playlist · 2 tracks").center();

    assert_eq!(
        window.drag(&mut transport, &mut queue, from, to),
        Some(Command::AddTracks {
            playlist: "playlist:A".to_owned(),
            revision: revision("root-1"),
            from_index: 2,
            tracks: vec![TRACK_B.to_owned()],
        })
    );
}

#[test]
fn a_block_of_three_rows_is_one_add_in_row_order() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of_tracks(
        &[TRACK_A, TRACK_B, "spotify:track:c", TRACK_A, TRACK_B],
        &mut transport,
    );
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_playlists(&mut window, &[("A", Some(2))], Some(rootlist_access(true)));
    // Rows 1, 2 and 3 are one picked block: the first entry is playing, so
    // it is never lifted.
    queue.press(1, Press::Replace);
    queue.press(2, Press::Toggle);
    queue.press(3, Press::Toggle);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let pane = pane_rect_when_shown(&window);
    let from = row_line(&frame, pane, TRACK_B);
    let to = sidebar_row_rect(&window, &frame, "A", "Playlist · 2 tracks").center();

    assert_eq!(
        window.drag(&mut transport, &mut queue, from, to),
        Some(Command::AddTracks {
            playlist: "playlist:A".to_owned(),
            revision: revision("root-1"),
            from_index: 2,
            tracks: vec![
                TRACK_B.to_owned(),
                "spotify:track:c".to_owned(),
                TRACK_A.to_owned(),
            ],
        }),
        "one command, and the pick in the order the rows were taken"
    );
}

#[test]
fn a_pick_with_one_episode_marks_no_target_and_sends_nothing() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of_tracks(&[TRACK_A, "spotify:episode:1", TRACK_B], &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_playlists(&mut window, &[("A", Some(2))], Some(rootlist_access(true)));
    queue.press(1, Press::Replace);
    queue.press(2, Press::Toggle);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let pane = pane_rect_when_shown(&window);
    let from = row_line(&frame, pane, "spotify:episode:1");
    let to = sidebar_row_rect(&window, &frame, "A", "Playlist · 2 tracks").center();

    assert_eq!(
        window.drag(&mut transport, &mut queue, from, to),
        None,
        "a mixed pick is refused whole, so nothing is offered"
    );
}

#[test]
fn a_track_between_two_rows_is_not_a_reorder() {
    let mut transport = playing(cover("abc"));
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    page_of_tracks(&mut window, &[TRACK_A]);
    sidebar_playlists(
        &mut window,
        &[("A", Some(2)), ("B", Some(3))],
        Some(rootlist_access(true)),
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), TRACK_A);
    // The hairline between two rows: a row above it, a row below it.
    let row = sidebar_row_rect(&window, &frame, "A", "Playlist · 2 tracks");
    let to = egui::pos2(row.center().x, row.bottom() + space::PX / 2.0);

    let command = window.drag(&mut transport, &mut queue, from, to);

    assert_eq!(
        command,
        Some(Command::AddTracks {
            playlist: "playlist:A".to_owned(),
            revision: revision("root-1"),
            from_index: 2,
            tracks: vec![TRACK_A.to_owned()],
        }),
        "the gap belongs to the row above, so it is an add and no reorder"
    );
    assert_eq!(sidebar_order(&window), ["A", "B"], "the list did not move");
}

#[test]
fn one_pointer_position_never_draws_a_rule_and_a_fill() {
    let tokens = Tokens::DARK;
    let mut transport = playing(cover("abc"));
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    page_of_tracks(&mut window, &[TRACK_A]);
    sidebar_playlists(
        &mut window,
        &[("A", Some(2)), ("B", Some(3)), ("C", Some(4))],
        Some(rootlist_access(true)),
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let area = sidebar_rect(&window);
    let pitch = sidebar::ROW_H + space::PX;
    let at = egui::pos2(
        area.left() + 100.0,
        sidebar_row_top(&frame, area, "B", "Playlist · 3 tracks") + pitch,
    );

    let playlist_from = row_line(&frame, area, "A");
    let (_, playlist_frame) = window.hold_drag(&mut transport, &mut queue, playlist_from, at);
    // Let the playlist drag go somewhere it lands nowhere, so the next drag
    // starts from a clean frame.
    let nowhere = view_rect(&window).center();
    let _ = window.frame(&mut transport, &mut queue, vec![window.release(nowhere)]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let track_from = row_line(&frame, view_rect(&window), TRACK_A);
    let (_, track_frame) = window.hold_drag(&mut transport, &mut queue, track_from, at);

    for (what, frame) in [("a playlist", &playlist_frame), ("a track", &track_frame)] {
        let rules = marker(frame).is_some();
        let filled = frame.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, egui::Shape::Rect(shape)
                if shape.fill == tokens.palette.row_selected
                    && sidebar_rect(&window).contains_rect(shape.rect))
        });
        assert!(
            rules ^ filled,
            "{what} must draw exactly one of a rule or a fill"
        );
    }
    assert!(
        marker(&playlist_frame).is_some(),
        "a playlist draws the rule"
    );
    assert!(
        !marker(&track_frame).is_some(),
        "a track draws the fill and no rule"
    );
}

#[test]
fn a_sidebar_drop_outside_the_column_writes_nothing() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_playlists(
        &mut window,
        &[("A", None), ("B", None)],
        Some(rootlist_access(true)),
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, sidebar_rect(&window), "A");
    let to = view_rect(&window).center();

    assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);

    assert_eq!(sidebar_order(&window), ["A", "B"]);
    assert!(window.library.rootlist_movable());
}

#[test]
fn a_drop_on_a_frame_another_region_took_moves_nothing() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1", "p2"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let view = view_rect(&window);
    let from = row_line(&frame, view, "p0");
    let to = page_boundary(&frame, view, &["p0", "p1", "p2"], 2);
    let _ = window.hold_drag(&mut transport, &mut queue, from, to);

    // Enter plays the row the drag left focused, so the drop finds the one
    // command taken.
    let (command, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![window.release(to), key(egui::Key::Enter, true)],
    );
    assert!(
        matches!(command, Some(Command::Play { .. })),
        "another region took the frame: {command:?}"
    );
    assert_eq!(
        page_rows(&window),
        ["list:p0", "list:p1", "list:p2"],
        "a drop behind another region's command moves nothing"
    );
    assert!(window.library.movable(), "and writes nothing");
}

#[test]
fn escape_cancels_a_drag_and_does_not_close_the_drawer() {
    // Only the rail has a drawer, so this window is narrow and the drag
    // happens over its view.
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(800.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let toggle = topbar_buttons(&frame)[0];
    let _ = window.click(&mut transport, &mut queue, toggle.center());
    assert_eq!(window.drawer, Drawer::Open);

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    // The drawer covers the left of the view, so the press lands on the row
    // further along it.
    let view = view_rect(&window);
    let from = egui::pos2(view.center().x, row_line(&frame, view, "p1").y);
    let to = below(from, 2.0);
    let _ = window.hold_drag(&mut transport, &mut queue, from, to);
    assert!(
        carry::carried(&window.ctx).is_some(),
        "the drag has to lift before Escape can cancel it"
    );

    let (_, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::Escape, true)],
    );
    assert!(
        carry::carried(&window.ctx).is_none(),
        "Escape drops what is carried"
    );
    let release = window.release(to);
    let _ = window.frame(&mut transport, &mut queue, vec![release]);
    assert_eq!(
        window.drawer,
        Drawer::Open,
        "the Escape that cancelled the drag is not the drawer's"
    );
}

#[test]
fn a_three_point_twitch_in_a_double_click_does_not_lift() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = row_line(&frame, pane_rect_when_shown(&window), "t2");
    let before = entries(&queue);
    // The second press of a double click, three points off the first.
    let _ = window.click(&mut transport, &mut queue, at);

    for event in [
        egui::Event::PointerMoved(at),
        crate::view::test_support::button(at, true),
        egui::Event::PointerMoved(at + Vec2::new(3.0, 0.0)),
        crate::view::test_support::button(at + Vec2::new(3.0, 0.0), false),
    ] {
        let (_, _) = window.frame(&mut transport, &mut queue, vec![event]);
        assert!(
            carry::carried(&window.ctx).is_none(),
            "three points is a click, not a drag"
        );
    }
    assert_eq!(entries(&queue), before, "a click reorders nothing");
}

#[test]
fn a_secondary_release_cancels_the_drag_instead_of_landing_it() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(5, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let pane = pane_rect_when_shown(&window);
    let from = row_line(&frame, pane, "t2");
    let to = below(from, 1.75);
    let _ = window.hold_drag(&mut transport, &mut queue, from, to);
    assert!(carry::carried(&window.ctx).is_some());
    let before = entries(&queue);

    // A right press and release over the zone is not the primary release that
    // lands the block, even though egui drops the payload when the drag ends.
    let secondary = |pressed| side(to, egui::PointerButton::Secondary, pressed);
    let _ = window.frame(&mut transport, &mut queue, vec![secondary(true)]);
    let _ = window.frame(&mut transport, &mut queue, vec![secondary(false)]);
    assert!(
        carry::carried(&window.ctx).is_none(),
        "the right release ends the drag"
    );
    assert_eq!(entries(&queue), before, "a right release lands nothing");
}

#[test]
fn a_held_payload_with_no_button_down_is_cleared() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, pane_rect_when_shown(&window), "t2");
    let _ = window.hold_drag(&mut transport, &mut queue, from, below(from, 1.75));
    assert!(carry::carried(&window.ctx).is_some());

    // The guard against a release the window never delivered: the button is
    // forced up under a payload egui has not cleared.
    window
        .ctx
        .input_mut(|input| input.pointer = Default::default());
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        carry::carried(&window.ctx).is_none(),
        "a carry with no button down is dropped"
    );
    assert_eq!(
        entries(&queue),
        ["t0", "t1", "t2", "t3"],
        "a payload dropped by the guard lands nowhere"
    );
}

#[test]
fn a_queue_that_restarted_under_a_drag_takes_no_drop() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(5, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let pane = pane_rect_when_shown(&window);
    let from = row_line(&frame, pane, "t2");
    let to = below(from, 1.75);
    let _ = window.hold_drag(&mut transport, &mut queue, from, to);

    // Something else fills the queue while the block is still held.
    let others: Vec<TrackEntry> = (0..5).map(|index| track(index + 10, None)).collect();
    let _ = queue.start(&others, 0, None, &mut transport);
    let release = window.release(to);
    let _ = window.frame(&mut transport, &mut queue, vec![release]);

    assert_eq!(
        entries(&queue),
        ["t10", "t11", "t12", "t13", "t14"],
        "a drop must not move the rows of a queue that replaced its own"
    );
}

#[test]
fn enter_does_not_play_a_focused_row_while_a_block_is_carried() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(5, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let pane = pane_rect_when_shown(&window);
    clicked_row(&mut window, &mut queue, &mut transport, pane, "t2");
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, pane, "t3");
    let _ = window.hold_drag(&mut transport, &mut queue, from, below(from, 1.75));
    assert!(carry::carrying(&window.ctx), "a block is in flight");
    let before = entries(&queue);
    let upcoming = queue.first_upcoming();

    let (command, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::Enter, true)],
    );
    assert_eq!(command, None, "a jump would move the queue under the drop");
    assert_eq!(queue.first_upcoming(), upcoming);
    assert_eq!(entries(&queue), before);
}

/// A queue of entries whose URIs are real tracks, so one can be added.
fn queue_of_tracks(uris: &[&str], transport: &mut Transport) -> Queue {
    let tracks: Vec<TrackEntry> = uris
        .iter()
        .map(|uri| TrackEntry {
            uri: (*uri).to_owned(),
            title: (*uri).to_owned(),
            ..track(0, None)
        })
        .collect();
    let mut queue = Queue::default();
    queue.start(&tracks, 0, None, transport);
    queue
}
