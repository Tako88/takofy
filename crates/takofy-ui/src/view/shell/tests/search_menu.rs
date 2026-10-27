use super::super::*;
use super::test_support::*;

/// A caret with a zero stroke width paints nothing: epaint treats a
/// zero-width stroke as empty, so the marked row's rule is the only thing
/// saying where Enter lands.
#[test]
fn a_marked_row_hides_the_caret() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    // The caret blinks; a paused cursor is what a single frame can assert.
    window
        .ctx
        .all_styles_mut(|style| style.visuals.text_cursor.blink = false);
    let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);

    let caret = |frame: &egui::FullOutput| {
        frame
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::LineSegment { stroke, .. } => Some(stroke.width),
                _ => None,
            })
    };
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let visible = caret(&frame).expect("a focused field paints its caret");
    assert!(visible > 0.0, "an unmarked field shows the caret");

    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::ArrowDown, true)],
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        window.ctx.text_edit_focused(),
        "the field still has the keyboard while a row is marked"
    );
    assert_eq!(
        caret(&frame),
        Some(0.0),
        "a marked row hides the caret, so the rule is the only target"
    );
}

#[test]
fn typing_focuses_the_field_and_arms_one_read() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));

    type_query(&mut window, &mut transport, &mut queue, "a");
    assert!(
        window.ctx.text_edit_focused(),
        "typing reaches the field only when the field has the keyboard"
    );
    assert_eq!(window.library.search.text, "a");
    assert_eq!(
        search_read(&mut window),
        Some(Command::Search {
            query: "a".to_owned()
        }),
        "the first character is a read"
    );

    // Focus alone is not a query: the click opens nothing and asks nothing.
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let at = field_box(&mut window, &mut transport, &mut queue).center();
    let _ = window.click(&mut transport, &mut queue, at);
    assert!(window.ctx.text_edit_focused());
    assert!(
        !window.library.search.is_open(),
        "focus alone opens nothing"
    );
    assert_eq!(search_read(&mut window), None);
}

#[test]
fn an_answer_draws_its_capped_track_rows_with_no_mark() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (menu, frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);

    assert_eq!(
        window.library.search.mark(),
        None,
        "the menu opens with its first row unmarked"
    );
    for index in 0..3 {
        assert!(
            text_in(&frame, &format!("t{index}"), menu).is_some(),
            "row {index} is drawn in the menu"
        );
    }
    assert!(
        text_in(&frame, "t3", menu).is_none(),
        "the tracks band shows three of the answer"
    );
}

#[test]
fn down_and_enter_play_the_marked_row_and_queue_the_whole_answer() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);

    // A non-zero row, so a zeroed index cannot pass for the marked one.
    for _ in 0..2 {
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );
    }
    assert_eq!(window.library.search.mark(), Some(1));

    assert_eq!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
        Some(Command::Play {
            uri: "t1".to_owned()
        })
    );
    assert_eq!(
        transport.now_playing.as_ref().map(|now| now.uri.as_str()),
        Some("t1")
    );
    assert_eq!(
        queue.upcoming().len(),
        6,
        "the queue is the whole answer, entered at the hit"
    );
    assert_eq!(
        queue.next(&mut transport),
        Some(Command::Play {
            uri: "t2".to_owned()
        }),
        "next walks the answer from the marked row"
    );
    assert!(
        !window.library.search.is_open(),
        "an applied action closes it"
    );
}

#[test]
fn up_from_the_field_marks_the_last_drawn_row() {
    for (count, expected) in [(3, 2), (4, 3)] {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", count);
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowUp, true)],
        );
        assert_eq!(
            window.library.search.mark(),
            Some(expected),
            "with {count} results the last row is {expected}"
        );
    }
}

#[test]
fn typing_over_a_marked_row_returns_to_the_field() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::ArrowDown, true)],
    );
    assert_eq!(window.library.search.mark(), Some(0));

    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![egui::Event::Text("b".to_owned())],
    );
    assert_eq!(window.library.search.text, "ab");
    assert_eq!(
        window.library.search.mark(),
        None,
        "typing puts the rule back on the field"
    );
    assert!(window.library.search.is_open());
}

#[test]
fn escape_is_two_steps_and_the_second_one_is_the_searchs() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::ArrowDown, true)],
    );
    assert_eq!(window.library.search.mark(), Some(0));

    assert_eq!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Escape),
        None
    );
    assert_eq!(window.library.search.text, "a", "the query is kept");
    assert!(window.library.search.is_open(), "and the menu stays up");
    assert_eq!(window.library.search.mark(), None);

    assert_eq!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Escape),
        None
    );
    assert_eq!(window.library.search.text, "");
    assert!(!window.library.search.is_open());
}

#[test]
fn the_drawers_ring_never_crosses_the_rails_edge() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(800.0, 900.0));
    window.library.apply(&Update::Playlists(vec![
        PlaylistEntry {
            composed: Vec::new(),
            uri: "spotify:playlist:a".to_owned(),
            name: "A".to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: None,
            access: None,
        },
        PlaylistEntry {
            composed: Vec::new(),
            uri: "spotify:playlist:b".to_owned(),
            name: "B".to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: None,
            access: None,
        },
    ]));

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let toggle = topbar_buttons(&frame)[0];
    let _ = window.click(&mut transport, &mut queue, toggle.center());
    assert_eq!(window.drawer, Drawer::Open);

    let drawer = drawer_rect(&window);
    let mut row = None;
    for _ in 0..32 {
        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Tab);
        let Some(focused) = window.ctx.memory(|memory| memory.focused()) else {
            continue;
        };
        let Some(rect) = window.ctx.read_response(focused).map(|drawn| drawn.rect) else {
            continue;
        };
        if drawer.contains_rect(rect) && (rect.height() - sidebar::ROW_H).abs() < 0.6 {
            row = Some(rect);
            break;
        }
    }
    let row = row.expect("Tab reached a rail row in the drawer");

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let rings = crate::view::test_support::rings(&frame, &Tokens::DARK);
    assert!(!rings.is_empty(), "the tabbed rail row wears the ring");
    for ring in &rings {
        assert!(
            drawer.contains_rect(*ring),
            "a ring crossed the drawer's edge: {ring:?} in {drawer:?}"
        );
    }
    assert!(drawer.contains_rect(row), "the ringed row left the drawer");
}

#[test]
fn escape_belongs_to_the_drawer_only_when_search_has_nothing_to_undo() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(800.0, 900.0));

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let toggle = topbar_buttons(&frame)[0];
    let _ = window.click(&mut transport, &mut queue, toggle.center());
    assert_eq!(window.drawer, Drawer::Open);
    assert!(
        !window.ctx.text_edit_focused(),
        "the toggle, not the field, has the keyboard"
    );

    // A held key is a repeat in egui, so every press below is released: the
    // field ignores a repeat's Escape, the way it ignores a held Enter.
    assert_eq!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Escape),
        None
    );
    assert_eq!(
        window.drawer,
        Drawer::Closed,
        "an empty, closed search leaves Escape to the drawer"
    );

    // The other way round: an open drawer and an active search at once,
    // reached by the keyboard so no click closes the drawer on the way.
    let _ = window.click(&mut transport, &mut queue, toggle.center());
    assert_eq!(window.drawer, Drawer::Open);
    for _ in 0..24 {
        if window.ctx.text_edit_focused() {
            break;
        }
        let _ = window.frame(&mut transport, &mut queue, vec![key(egui::Key::Tab, true)]);
    }
    assert!(
        window.ctx.text_edit_focused(),
        "Tab reaches the search field"
    );
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![egui::Event::Text("a".to_owned())],
    );
    assert!(window.library.search.is_open());

    assert_eq!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Escape),
        None
    );
    assert_eq!(window.library.search.text, "");
    assert!(!window.library.search.is_open());
    assert_eq!(
        window.drawer,
        Drawer::Open,
        "search consumed the Escape it acted on"
    );

    // With nothing left to undo, the next Escape is the drawer's again.
    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);
    assert_eq!(window.drawer, Drawer::Closed);
}

#[test]
fn a_space_typed_in_the_field_is_not_the_transports() {
    let mut transport = Transport::default();
    let mut queue = two_tracks(&mut transport);
    transport.state = TransportState::Playing;
    let mut window = Window::new(Vec2::new(1400.0, 900.0));

    let at = field_box(&mut window, &mut transport, &mut queue).center();
    let _ = window.click(&mut transport, &mut queue, at);
    assert!(window.ctx.text_edit_focused());

    let (command, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![
            key(egui::Key::Space, true),
            egui::Event::Text(" ".to_owned()),
        ],
    );
    assert_eq!(command, None, "a space in the field is a character");
    assert_eq!(
        transport.state,
        TransportState::Playing,
        "and it does not pause the transport"
    );
    assert_eq!(window.library.search.text, " ");
}

#[test]
fn the_floor_appears_only_when_the_answer_is_longer_than_the_menu() {
    for (count, floor) in [(3, false), (4, true)] {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (menu, frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", count);
        let label = "See more results for “a”";
        assert_eq!(
            text_in(&frame, label, menu).is_some(),
            floor,
            "an answer of {count} draws the floor: {floor}"
        );
        let (rows, floor) = window
            .library
            .search
            .listing()
            .map(|listing| (listing.shape.rows_per_band(), listing.shape.floor))
            .expect("the answered menu has a shape");
        assert_eq!(
            menu.height(),
            layout::search_menu::bands_height(&rows, floor),
            "and the frame is exactly its bands"
        );
    }
}

/// Press a row, then release it: the menu must survive the frame in which
/// the press surrenders the field's focus. Both outcomes are staged:
/// the natural one, and the opposite one forced between press and release.
#[test]
fn a_row_click_plays_under_both_focus_outcomes() {
    for keep_focus in [false, true] {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (menu, frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let at = text_in(&frame, "t1", menu)
            .expect("row t1 is drawn")
            .center();
        let field = window.ctx.memory(|memory| memory.focused());

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::PointerMoved(at)],
        );
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![crate::view::test_support::button(at, true)],
        );

        match (keep_focus, field) {
            (true, Some(id)) => window.ctx.memory_mut(|memory| memory.request_focus(id)),
            (false, _) => {
                if let Some(id) = window.ctx.memory(|memory| memory.focused()) {
                    window.ctx.memory_mut(|memory| memory.surrender_focus(id));
                }
            }
            (true, None) => panic!("the field had the keyboard before the press"),
        }
        assert!(window.library.search.is_open(), "the press keeps it open");

        let (command, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![crate::view::test_support::button(at, false)],
        );
        assert_eq!(
            command,
            Some(Command::Play {
                uri: "t1".to_owned()
            }),
            "a click plays the row with keep_focus = {keep_focus}"
        );
        assert_eq!(
            queue.upcoming().len(),
            6,
            "the queue is the whole answer, entered at the clicked row ({keep_focus})"
        );
        assert_eq!(
            queue.next(&mut transport),
            Some(Command::Play {
                uri: "t2".to_owned()
            }),
            "next walks the answer from the clicked row ({keep_focus})"
        );
    }
}

#[test]
fn a_press_on_one_row_and_a_release_on_another_play_nothing() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (menu, frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
    let one = text_in(&frame, "t1", menu).expect("row t1").center();
    let other = text_in(&frame, "t2", menu).expect("row t2").center();

    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![egui::Event::PointerMoved(one)],
    );
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![crate::view::test_support::button(one, true)],
    );
    let (command, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![crate::view::test_support::button(other, false)],
    );
    assert_eq!(command, None, "two rows are not one click");
    assert!(transport.now_playing.is_none());
    assert!(
        window.library.search.is_open(),
        "the release is still engaged"
    );

    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        !window.library.search.is_open(),
        "the next idle frame closes it"
    );
}

#[test]
fn a_click_outside_both_closes_the_menu() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
    assert!(window.library.search.is_open());

    let _ = window.click(&mut transport, &mut queue, view_rect(&window).center());
    assert!(!window.library.search.is_open());
}

#[test]
fn tabbing_out_of_the_field_closes_the_menu() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
    assert!(window.ctx.text_edit_focused());

    let _ = window.frame(&mut transport, &mut queue, vec![key(egui::Key::Tab, true)]);
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(!window.ctx.text_edit_focused(), "Tab leaves the text edit");
    assert!(!window.library.search.is_open());
}

#[test]
fn enter_on_the_field_opens_the_results_page() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);

    assert_eq!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
        None,
        "a search page never settles with a command"
    );
    assert_eq!(window.library.page(), Some(&Page::Search("a".to_owned())));
    assert!(!window.library.search.is_open());
    assert_eq!(
        window.library.search.page_answer("a"),
        Some(&Load::Ready(
            (0..8).map(|index| track(index, None)).collect()
        )),
        "a ready menu seeds the page without a second read"
    );
    assert_eq!(search_read(&mut window), None);
}

#[test]
fn enter_after_a_failed_menu_asks_exactly_once() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));

    type_query(&mut window, &mut transport, &mut queue, "a");
    assert_eq!(
        search_read(&mut window),
        Some(Command::Search {
            query: "a".to_owned()
        })
    );
    window.library.apply(&Update::SearchFailed {
        query: "a".to_owned(),
        failure: Failure::Search,
    });
    let _ = window.frame(&mut transport, &mut queue, Vec::new());

    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
    assert_eq!(window.library.page(), Some(&Page::Search("a".to_owned())));
    assert_eq!(
        search_read(&mut window),
        Some(Command::Search {
            query: "a".to_owned()
        }),
        "Enter on a failed menu is the retry"
    );
    assert_eq!(search_read(&mut window), None, "asked exactly once");
}

#[test]
fn a_dropped_action_leaves_the_menu_open_and_the_queue_untouched() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    stale_playlist_behind_an_album(&mut window.library);
    let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::ArrowDown, true)],
    );
    assert_eq!(window.library.search.mark(), Some(0));

    // Enter plays the marked row, but a browser Back in the same frame
    // takes the frame's one command: the action must be dropped whole.
    let (command, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![
            key(egui::Key::Enter, true),
            key(egui::Key::BrowserBack, true),
        ],
    );
    assert_eq!(
        command,
        Some(Command::LoadTracks {
            playlist: PLAYLIST.to_owned()
        }),
        "the nav kept the frame"
    );
    assert!(transport.now_playing.is_none(), "the queue was not touched");
    assert_eq!(queue.upcoming().len(), 0);
    assert!(window.library.search.is_open(), "the menu stays open");
}

#[test]
fn a_held_enter_plays_once() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::ArrowDown, true)],
    );

    let (first, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::Enter, true)],
    );
    assert_eq!(
        first,
        Some(Command::Play {
            uri: "t0".to_owned()
        })
    );
    let (held, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![key_repeat(egui::Key::Enter)],
    );
    assert_eq!(held, None, "an auto-repeat is nobody's press");
    // If the repeat were read as a press it would be the field's Enter, which
    // opens the page: the two assertions below are what the inversion breaks.
    assert_eq!(
        window.library.page(),
        None,
        "a held Enter plays once and opens no page"
    );
    assert!(
        window.ctx.text_edit_focused(),
        "and it must not blur the field it was pressed in"
    );
}

#[test]
fn the_fields_width_never_moves() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));

    // The text edit's own rect, not the painted box: the box is drawn at a
    // width nothing can change, so it could never fail this.
    let at = field_box(&mut window, &mut transport, &mut queue).center();
    let _ = window.click(&mut transport, &mut queue, at);
    let empty = edit_box(&window);
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![egui::Event::Text("a".to_owned())],
    );
    let typed = edit_box(&window);

    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);
    let cleared = edit_box(&window);

    assert_eq!(empty, typed, "typing does not resize the field");
    assert_eq!(empty, cleared, "clearing does not resize it either");
}

#[test]
fn home_and_back_leave_no_stale_open_menu() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    ready_playlist(&mut window);
    let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
    assert!(window.library.search.is_open());

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let buttons = topbar_buttons(&frame);
    let _ = window.click(&mut transport, &mut queue, buttons[0].center());
    assert!(!window.library.search.is_open(), "Home leaves the field");

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let buttons = topbar_buttons(&frame);
    let _ = window.click(&mut transport, &mut queue, buttons[1].center());
    assert!(
        !window.library.search.is_open(),
        "and Back does not reopen it"
    );
    assert_eq!(window.library.search.text, "a");
}

#[test]
fn the_clear_control_empties_the_query_without_moving_the_field() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
    let field = search_field(&frame).expect("the field is painted");
    // Inside the padded hit rect, which spans the field's height at its right
    // padding.
    let at = egui::Pos2::new(field.right() - 16.0, field.center().y);
    // The text edit's own rect, which the clear control must not move.
    let before = edit_box(&window);

    let _ = window.click(&mut transport, &mut queue, at);
    assert_eq!(window.library.search.text, "");
    assert!(
        !window.library.search.is_open(),
        "clearing closes the menu the query kept open"
    );
    assert!(
        window.ctx.text_edit_focused(),
        "the keyboard goes back to the field"
    );
    assert_eq!(
        edit_box(&window),
        before,
        "clearing never changes the field's width"
    );
}

#[test]
fn the_menu_says_no_matches_and_a_failure_in_one_muted_line() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    type_query(&mut window, &mut transport, &mut queue, "a");
    assert_eq!(
        search_read(&mut window),
        Some(Command::Search {
            query: "a".to_owned()
        })
    );
    window.library.apply(&Update::Search {
        query: "a".to_owned(),
        tracks: Vec::new(),
    });
    // The first frame an `Area` is shown sizes it; the second draws it.
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let field = search_field(&frame).expect("the field is painted");
    let menu = menu_box(&window, field, layout::search_menu::line_height());
    assert!(
        text_in(&frame, "No matches for “a”", menu).is_some(),
        "an empty answer is one line naming the query"
    );

    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    type_query(&mut window, &mut transport, &mut queue, "a");
    let _ = search_read(&mut window);
    window.library.apply(&Update::SearchFailed {
        query: "a".to_owned(),
        failure: Failure::Search,
    });
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let field = search_field(&frame).expect("the field is painted");
    let menu = menu_box(&window, field, layout::search_menu::line_height());
    assert!(
        text_in(
            &frame,
            crate::signin::failure_message(Failure::Search),
            menu
        )
        .is_some(),
        "a failed read is one line with its own words"
    );
}

#[test]
fn enter_on_the_loaded_row_pauses_instead_of_starting_it_again() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);

    // Play row 1, then reopen the menu on the same query and mark it again.
    for _ in 0..2 {
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );
    }
    assert_eq!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
        Some(Command::Play {
            uri: "t1".to_owned()
        })
    );
    let queued = queue.upcoming().len();

    // The arrows re-open the closed menu and mark the rows it draws.
    for _ in 0..2 {
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );
    }
    assert!(window.library.search.is_open(), "an arrow re-opens it");
    assert_eq!(window.library.search.mark(), Some(1));
    assert_eq!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
        Some(Command::Pause),
        "the loaded row's action is pause, not a second start"
    );
    assert_eq!(queue.upcoming().len(), queued, "pausing keeps the queue");
    assert_eq!(window.library.search.text, "a", "and keeps the query");
}

#[test]
fn an_arrow_reopens_a_closed_menu_on_the_row_it_would_have_marked() {
    // Down: the first row.
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::ArrowDown, true)],
    );
    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
    assert!(!window.library.search.is_open());
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::ArrowDown, true)],
    );
    assert!(window.library.search.is_open());
    assert_eq!(window.library.search.mark(), Some(0));

    // Up: the last drawn row, which is the floor when it is drawn.
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::ArrowDown, true)],
    );
    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
    assert!(!window.library.search.is_open());
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::ArrowUp, true)],
    );
    assert!(window.library.search.is_open());
    assert_eq!(window.library.search.mark(), Some(3), "the floor is last");
}

/// A click on the field returns the rule to it, and the whole painted box is
/// the field: the text edit is it, so a click on the magnifier or the padding
/// beside it focuses the field and re-opens the menu the query still holds.
/// Before the text edit covered the box, those clicks were "elsewhere": the
/// field blurred, `settle` closed the menu, and `clicked_field` never fired.
#[test]
fn a_click_on_the_magnifier_or_the_left_padding_is_a_click_on_the_field() {
    let pad = space::S3 + icon::Size::MD.side / 2.0;
    let cases = [("the magnifier", pad), ("the left padding", space::S1)];
    for (name, dx) in cases {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let field = search_field(&frame).expect("the field is painted");

        // Enter opens the page: the menu closes, the query stays, and the
        // keyboard leaves the field with the menu.
        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
        assert_eq!(window.library.page(), Some(&Page::Search("a".to_owned())));
        assert!(!window.library.search.is_open());
        assert!(!window.ctx.text_edit_focused());

        let at = egui::Pos2::new(field.left() + dx, field.center().y);
        let _ = window.click(&mut transport, &mut queue, at);
        assert!(
            window.ctx.text_edit_focused(),
            "a click on {name} focuses the field"
        );
        assert!(
            window.library.search.is_open(),
            "and a click on {name} re-opens the menu the query holds"
        );

        // The reason those points are the field's: the text edit is the
        // painted box, not a narrower widget inside it.
        let edit = edit_box(&window);
        assert_eq!(
            edit.left(),
            field.left(),
            "the text edit starts at the edge"
        );
        assert_eq!(edit.right(), field.right(), "and ends at the other one");
    }
}

/// The pointer's tick is a hint on a row that is not the mark: moving the
/// pointer makes it the last device, and it never takes the keyboard's rule
/// with it.
#[test]
fn the_pointer_tick_skips_the_marked_row() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (menu, _frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
    let tokens = Tokens::DARK;
    let ticks = |frame: &egui::FullOutput| {
        frame
            .shapes
            .iter()
            .filter(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape) => {
                    shape.fill == tokens.palette.text_muted
                        && (shape.rect.width() - stroke::RULE).abs() < 0.01
                        && (shape.rect.height() - space::S4).abs() < 0.01
                        && menu.contains_rect(shape.rect)
                }
                _ => false,
            })
            .count()
    };

    // The pointer arrives over the floor: it owns the highlight, so the
    // floor takes the row hover wash and the tick.
    let third = layout::search_menu::row_rect(menu, 3).center();
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![egui::Event::PointerMoved(third)],
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert_eq!(window.library.search.device(), SearchDevice::Pointer);
    assert_eq!(ticks(&frame), 1, "the pointed-at row takes the tick");

    // The keyboard marks the row the pointer is on: the mark's rule is the
    // answer now, so the pointer stops drawing its tick there.
    for _ in 0..4 {
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );
    }
    assert_eq!(window.library.search.mark(), Some(3));
    assert_eq!(window.library.search.device(), SearchDevice::Keyboard);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert_eq!(
        ticks(&frame),
        0,
        "the marked row keeps the rule and takes no tick"
    );

    // The pointer moves on: the device switches back and the new row takes
    // the tick beside the mark, which does not move.
    let second = layout::search_menu::row_rect(menu, 2).center();
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![egui::Event::PointerMoved(second)],
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert_eq!(window.library.search.device(), SearchDevice::Pointer);
    assert_eq!(
        window.library.search.mark(),
        Some(3),
        "the pointer moves no mark"
    );
    assert_eq!(
        ticks(&frame),
        1,
        "a row that is not the mark takes the tick"
    );
}

#[test]
fn a_click_on_an_album_row_opens_the_album_page() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let menu = banded_menu(&mut window, &mut transport, &mut queue);
    let album = band_row(&window, menu, 1, 0).center();
    let _ = window.click(&mut transport, &mut queue, album);
    assert_eq!(
        window.library.page(),
        Some(&Page::Album("spotify:album:0".to_owned()))
    );
    assert!(
        !window.library.search.is_open(),
        "the album closes the menu"
    );
}

#[test]
fn a_click_on_an_artist_row_opens_the_artist_page() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let menu = banded_menu(&mut window, &mut transport, &mut queue);
    let artist = band_row(&window, menu, 2, 0).center();
    let _ = window.click(&mut transport, &mut queue, artist);
    assert_eq!(
        window.library.page(),
        Some(&Page::Artist("spotify:artist:0".to_owned()))
    );
    assert!(
        !window.library.search.is_open(),
        "the artist closes the menu"
    );
}

#[test]
fn enter_on_an_album_and_an_artist_opens_the_same_pages() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let _ = banded_menu(&mut window, &mut transport, &mut queue);
    for _ in 0..4 {
        let _ = press(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::ArrowDown,
        );
    }
    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
    assert_eq!(
        window.library.page(),
        Some(&Page::Album("spotify:album:0".to_owned()))
    );

    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let _ = banded_menu(&mut window, &mut transport, &mut queue);
    for _ in 0..6 {
        let _ = press(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::ArrowDown,
        );
    }
    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
    assert_eq!(
        window.library.page(),
        Some(&Page::Artist("spotify:artist:0".to_owned()))
    );
}

/// The floor row is an action, and a click on it opens the page — the
/// same page Enter on the floor opens.
#[test]
fn a_click_on_the_floor_opens_the_results_page() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (menu, frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
    let (rows, capped) = window
        .library
        .search
        .listing()
        .map(|listing| (listing.shape.rows_per_band(), listing.shape.floor))
        .expect("the answered menu has a shape");
    let floor = layout::search_menu::place(menu, &rows, capped, window.ctx.pixels_per_point())
        .floor
        .expect("the answer is long enough to draw the floor");
    assert!(
        text_in(&frame, "See more results for “a”", menu).is_some(),
        "the answer is long enough to draw the floor"
    );

    assert_eq!(
        window.click(&mut transport, &mut queue, floor.center()),
        None,
        "a search page never settles with a command"
    );
    assert_eq!(window.library.page(), Some(&Page::Search("a".to_owned())));
    assert!(
        !window.library.search.is_open(),
        "the floor closes the menu"
    );
}

/// Finding 6: a nav that takes the frame's command drops `OpenResults`, and
/// a dropped action leaves the menu open *and* the keyboard in the field.
#[test]
fn a_dropped_open_results_leaves_the_field_focused() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    stale_playlist_behind_an_album(&mut window.library);
    let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
    // Enter with the field marked, not on a row: it asks for the page.
    assert!(window.ctx.text_edit_focused());

    let (command, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![
            key(egui::Key::Enter, true),
            key(egui::Key::BrowserBack, true),
        ],
    );
    assert_eq!(
        command,
        Some(Command::LoadTracks {
            playlist: PLAYLIST.to_owned()
        }),
        "the nav kept the frame"
    );
    assert_ne!(
        window.library.page(),
        Some(&Page::Search("a".to_owned())),
        "the page was not opened"
    );
    assert!(window.library.search.is_open(), "the menu stays open");
    assert!(
        window.ctx.text_edit_focused(),
        "and a dropped OpenResults does not surrender the field"
    );
}

/// The loaded row's action follows the transport's state, exactly as its
/// glyph does. A paused track resumes; an ended one is restarted, because
/// pausing it would do nothing.
#[test]
fn enter_resumes_a_paused_row_and_restarts_a_stopped_one() {
    for (state, expected) in [
        (TransportState::Paused, Command::Resume),
        (
            TransportState::Stopped,
            Command::Play {
                uri: "t1".to_owned(),
            },
        ),
    ] {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);

        // Load row 1, then put the transport in the state under test and
        // mark the loaded row again from the closed menu.
        for _ in 0..2 {
            let _ = window.frame(
                &mut transport,
                &mut queue,
                vec![key(egui::Key::ArrowDown, true)],
            );
        }
        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("t1")
        );
        transport.state = state;
        for _ in 0..2 {
            let _ = window.frame(
                &mut transport,
                &mut queue,
                vec![key(egui::Key::ArrowDown, true)],
            );
        }
        assert_eq!(window.library.search.mark(), Some(1));

        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
            Some(expected.clone()),
            "{state:?}"
        );
        if state == TransportState::Stopped {
            assert_eq!(
                queue.upcoming().len(),
                6,
                "a restart enters the whole answer at the row"
            );
            assert_eq!(
                queue.next(&mut transport),
                Some(Command::Play {
                    uri: "t2".to_owned()
                }),
                "and next walks the answer from there"
            );
        }
    }
}

// -----------------------------------------------------------------------
// The queue's selection, its drags, the ghost, and the Alt keys.
// -----------------------------------------------------------------------

/// The text edit's own rect, read from egui's widget memory. The id comes
/// from the focused widget, so the field must have the keyboard: a painted
/// box is where the view draws, but the text edit's own response rect is
/// what the width contract and the pointer both read.
fn edit_box(window: &Window) -> Rect {
    let id = window
        .ctx
        .memory(|memory| memory.focused())
        .expect("the field has the keyboard");
    window
        .ctx
        .read_response(id)
        .expect("the text edit was laid out")
        .rect
}

/// Types `a`, answers it with three bands, and returns the drawn menu.
fn banded_menu(window: &mut Window, transport: &mut Transport, queue: &mut Queue) -> Rect {
    type_query(window, transport, queue, "a");
    window.library.apply(&Update::Search {
        query: "a".to_owned(),
        tracks: (0..8).map(band_track).collect(),
    });
    let _ = window.frame(transport, queue, Vec::new());
    let (_, frame) = window.frame(transport, queue, Vec::new());
    let field = search_field(&frame).expect("the topbar paints the field");
    let (rows, floor) = window
        .library
        .search
        .listing()
        .map(|listing| (listing.shape.rows_per_band(), listing.shape.floor))
        .expect("the answered menu has a shape");
    let height = layout::search_menu::bands_height(&rows, floor);
    menu_box(window, field, height)
}

fn band_row(window: &Window, menu: Rect, band: usize, row: usize) -> Rect {
    let (rows, floor) = window
        .library
        .search
        .listing()
        .map(|listing| (listing.shape.rows_per_band(), listing.shape.floor))
        .expect("the answered menu has a shape");
    layout::search_menu::place(menu, &rows, floor, window.ctx.pixels_per_point()).bands[band].rows
        [row]
}

/// A track on its own album, by its own artist.
fn band_track(index: usize) -> TrackEntry {
    let mut entry = track(index, None);
    entry.artists = format!("Artist{index}");
    entry.album = Some(format!("Album{index}"));
    entry.album_uri = Some(format!("spotify:album:{index}"));
    entry.artist_uris = vec![format!("spotify:artist:{index}")];
    entry.artist_names = vec![format!("Artist{index}")];
    entry
}
