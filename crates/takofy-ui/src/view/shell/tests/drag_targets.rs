use super::super::*;
use super::test_support::*;

#[test]
fn the_ghost_is_painted_on_the_frame_of_the_lift() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(5, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let pane = pane_rect_when_shown(&window);
    let from = row_line(&frame, pane, "t2");
    let to = below(from, 0.75);
    let size = Vec2::new(pane.width() - 2.0 * space::S3, QUEUE_ROW_H);

    let mut events = crate::view::test_support::drag_events(from, to);
    events.pop();
    let mut pointer = from;
    let mut lifted = None;
    for event in events {
        if let egui::Event::PointerMoved(at) = event {
            pointer = at;
        }
        let (_, frame) = window.frame(&mut transport, &mut queue, vec![event]);
        if carry::carried(&window.ctx).is_some() {
            lifted = Some(frame);
            break;
        }
    }

    let frame = lifted.expect("the drag lifts the row");
    let rect = ghost_rect(pointer, size);
    assert_eq!(
        ghost_fill(&frame, rect),
        Some(Tokens::DARK.palette.surface_raised),
        "the ghost is not drawn raised on the frame that lifted it"
    );
    assert!(
        shadows(&frame)
            .iter()
            .any(|cast| cast.contains(rect.center())),
        "a landed ghost casts a shadow: {:?}",
        shadows(&frame)
    );
    let mark = marker(&frame).expect("a carried block draws its marker");
    assert!(
        pane.contains_rect(mark),
        "the marker is not inside the pane: {mark:?}"
    );

    let release = window.release(to);
    let (_, frame) = window.frame(&mut transport, &mut queue, vec![release]);
    assert!(
        ghost_fill(&frame, ghost_rect(to, size)).is_none(),
        "a dropped block is gone on the frame that dropped it"
    );
    assert!(
        marker(&frame).is_none(),
        "a release marks nothing: it has landed"
    );
}

#[test]
fn a_refused_pointer_paints_a_flat_ghost_and_no_marker() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(5, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let pane = pane_rect_when_shown(&window);
    let from = row_line(&frame, pane, "t2");
    let to = view_rect(&window).center();
    let size = Vec2::new(pane.width() - 2.0 * space::S3, QUEUE_ROW_H);

    let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);
    let rect = ghost_rect(to, size);
    assert_eq!(
        ghost_fill(&frame, rect),
        Some(Tokens::DARK.palette.surface),
        "a refused ghost is flat"
    );
    assert!(
        shadows(&frame).is_empty(),
        "a refused ghost casts no shadow: {:?}",
        shadows(&frame)
    );
    assert!(marker(&frame).is_none(), "a refused pointer marks nothing");
}

#[test]
fn nothing_hovers_while_a_row_is_carried() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(5, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let pane = pane_rect_when_shown(&window);
    let from = row_line(&frame, pane, "t2");
    let hover = row_line(&frame, pane, "t4");
    let washes = |output: &egui::FullOutput| {
        let tokens = Tokens::DARK;
        output
            .shapes
            .iter()
            .filter(|clipped| {
                matches!(&clipped.shape, egui::Shape::Rect(shape)
                    if shape.fill == tokens.palette.row_hover && pane.contains_rect(shape.rect))
            })
            .count()
    };

    // The row's wash eases in, so the pointer rests on it for a few frames.
    let mut resting = None;
    for event in [
        Some(egui::Event::PointerMoved(hover)),
        None,
        None,
        None,
        None,
    ] {
        let events = event.into_iter().collect();
        resting = Some(window.frame(&mut transport, &mut queue, events).1);
    }
    assert_eq!(
        washes(&resting.expect("a frame ran")),
        1,
        "the row under the pointer washes"
    );

    let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, hover);
    assert_eq!(
        washes(&frame),
        0,
        "a row under a carried block must not wash"
    );
}

#[test]
fn the_drawn_fill_and_the_drop_area_share_one_rect() {
    // The production area comes from `queue_button(bar)` and the fill from
    // the draw, so an offset between them flips one of the four edges.
    for width in [900.0, 1100.0] {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(width, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), "p1");
        let (_, frame) = window.hold_drag(
            &mut transport,
            &mut queue,
            from,
            queue_button_at(&window).center(),
        );
        let fill = claimed_fill(&frame, &window)
            .unwrap_or_else(|| panic!("the queue button does not fill at {width}"));
        assert_eq!(fill.size(), Vec2::splat(extent::BUTTON), "at {width}");

        for edge in [
            Vec2::new(22.0, 0.0),
            Vec2::new(-22.0, 0.0),
            Vec2::new(0.0, 22.0),
            Vec2::new(0.0, -22.0),
        ] {
            let (_, frame) = window.frame(
                &mut transport,
                &mut queue,
                vec![egui::Event::PointerMoved(fill.center() + edge)],
            );
            assert_eq!(
                claimed_fill(&frame, &window),
                Some(fill),
                "the inside edge {edge:?} at {width} is outside the drop area"
            );
        }
        for edge in [Vec2::new(22.5, 0.0), Vec2::new(0.0, 22.5)] {
            let (_, frame) = window.frame(
                &mut transport,
                &mut queue,
                vec![egui::Event::PointerMoved(fill.center() + edge)],
            );
            assert_eq!(
                claimed_fill(&frame, &window),
                None,
                "past the edge {edge:?} at {width} still fills"
            );
        }

        let corner = fill.center() + Vec2::new(22.0, 22.0);
        let (command, _) = window.frame(&mut transport, &mut queue, vec![window.release(corner)]);
        assert_eq!(command, None);
        assert_eq!(
            entries(&queue),
            ["t0", "t1", "t2", "t3", "list:p1"],
            "the inclusive corner at {width} is outside the drop area"
        );
    }
}

#[test]
fn a_page_list_lift_resolves_its_target_on_the_frame_it_lifts() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(900.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), "p1");
    let to = queue_button_at(&window).center();

    let mut pointer = from;
    let mut lifted = None;
    for event in carried_frame_events(from, to) {
        if let egui::Event::PointerMoved(at) = &event {
            pointer = *at;
        }
        let (_, frame) = window.frame(&mut transport, &mut queue, vec![event]);
        if carry::carried(&window.ctx).is_some() {
            lifted = Some(frame);
            break;
        }
    }

    let frame = lifted.expect("the page list lifts on the jump");
    assert_eq!(pointer, to, "the lift frame is the frame the pointer jumps");
    let fill = claimed_fill(&frame, &window).expect("the button fills on the lift frame");
    assert_eq!(
        fill.size(),
        Vec2::splat(extent::BUTTON),
        "the fill keeps the glyph's box"
    );
    assert_eq!(
        ghost_at(&frame, to),
        Some(Tokens::DARK.palette.surface_raised),
        "the ghost is raised on the lift frame"
    );
    assert!(
        !shadows(&frame).is_empty(),
        "a raised ghost casts a shadow: {:?}",
        shadows(&frame)
    );
    assert!(marker(&frame).is_none(), "an add draws no marker");

    let (_, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);
    assert_eq!(entries(&queue), ["t0", "t1", "t2", "t3", "list:p1"]);
}

#[test]
fn at_900_a_track_dropped_on_the_queue_button_is_appended_and_nothing_plays() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(900.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), "p1");
    let to = queue_button_at(&window).center();

    assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
    assert_eq!(entries(&queue), ["t0", "t1", "t2", "t3", "list:p1"]);
    assert_eq!(
        transport.now_playing.as_ref().map(|now| now.uri.as_str()),
        Some("t0"),
        "an add must not start anything"
    );
}

#[test]
fn at_1100_the_extras_queue_button_takes_the_drop() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1100.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), "p1");
    let to = queue_button_at(&window).center();

    assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
    assert_eq!(entries(&queue), ["t0", "t1", "t2", "t3", "list:p1"]);
}

#[test]
fn over_the_queue_button_it_fills_the_ghost_rises_and_no_marker_is_drawn() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(900.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), "p1");
    let to = queue_button_at(&window).center();

    let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);
    let fill = claimed_fill(&frame, &window).expect("the button fills under the carry");
    assert_eq!(fill.size(), Vec2::splat(extent::BUTTON));
    assert_eq!(
        ghost_at(&frame, to),
        Some(Tokens::DARK.palette.surface_raised),
        "an add draws the ghost raised"
    );
    assert!(marker(&frame).is_none(), "an add draws no marker");
}

#[test]
fn the_drop_area_reaches_past_the_glyph() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(900.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), "p1");
    let to = queue_button_at(&window).center() + Vec2::new(19.0, 19.0);

    assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
    assert_eq!(entries(&queue), ["t0", "t1", "t2", "t3", "list:p1"]);
}

#[test]
fn the_empty_column_below_a_short_queue_takes_the_row_at_the_end() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(2, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let pane = pane_rect_when_shown(&window);
    let last = row_line(&frame, pane, "t1");
    let from = row_line(&frame, view_rect(&window), "p0");
    let to = Pos2::new(pane.center().x, pane.bottom() - 4.0);
    assert!(
        to.y > last.y + QUEUE_ROW_H,
        "the pointer must sit below the list, not on a row: {to:?}"
    );

    assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
    assert_eq!(entries(&queue), ["t0", "t1", "list:p0"]);
    assert_eq!(
        queue.first_upcoming() + queue.upcoming().len(),
        3,
        "the row lands at the end"
    );
}

#[test]
fn an_empty_queue_takes_the_queue_buttons_add() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(900.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), "p1");
    let to = queue_button_at(&window).center();

    let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);
    assert!(
        claimed_fill(&frame, &window).is_some(),
        "the button claims the add"
    );
    let (_, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);
    assert_eq!(entries(&queue), ["list:p1"]);
    assert!(
        !queue.can_previous(),
        "the button queues the row, it does not start it"
    );
}

#[test]
fn a_queue_row_carried_into_a_narrow_window_is_not_moved_by_the_button() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, pane_rect_when_shown(&window), "t2");
    let _ = window.hold_drag(
        &mut transport,
        &mut queue,
        from,
        view_rect(&window).center(),
    );
    let before = entries(&queue);

    window.size = Vec2::new(900.0, 900.0);
    let to = queue_button_at(&window).center();
    let (_, frame) = window.frame(
        &mut transport,
        &mut queue,
        vec![egui::Event::PointerMoved(to)],
    );
    assert!(
        claimed_fill(&frame, &window).is_none(),
        "the button never moves a queue row"
    );
    assert_eq!(
        ghost_at(&frame, to),
        Some(Tokens::DARK.palette.surface),
        "the ghost is flat"
    );
    let (_, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);
    assert_eq!(entries(&queue), before, "a queue row stays put");
}

#[test]
fn a_page_list_carry_lifted_wide_and_dropped_narrow_is_appended() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), "p1");
    let (_, frame) = window.hold_drag(
        &mut transport,
        &mut queue,
        from,
        view_rect(&window).center(),
    );
    assert!(
        claimed_fill(&frame, &window).is_none(),
        "the pane is on screen, so the button is not a target"
    );

    window.size = Vec2::new(900.0, 900.0);
    let to = queue_button_at(&window).center();
    let (_, frame) = window.frame(
        &mut transport,
        &mut queue,
        vec![egui::Event::PointerMoved(to)],
    );
    assert!(
        claimed_fill(&frame, &window).is_some(),
        "the narrow button takes the add"
    );
    let (command, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);
    assert_eq!(command, None);
    assert_eq!(entries(&queue), ["t0", "t1", "t2", "t3", "list:p1"]);
}

#[test]
fn above_the_now_playing_width_the_queue_button_takes_no_drop() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), "p1");
    let to = queue_button_at(&window).center();
    let before = entries(&queue);

    let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);
    assert!(
        claimed_fill(&frame, &window).is_none(),
        "the pane is on screen, so the button is not a target"
    );
    assert_eq!(
        ghost_at(&frame, to),
        Some(Tokens::DARK.palette.surface),
        "the ghost is flat"
    );
    assert!(marker(&frame).is_none(), "no target, no marker");
    let (_, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);
    assert_eq!(entries(&queue), before, "a refused drop changes nothing");
}

#[test]
fn escape_over_the_queue_button_adds_nothing() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(900.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), "p1");
    let to = queue_button_at(&window).center();
    let _ = window.hold_drag(&mut transport, &mut queue, from, to);
    assert!(
        carry::carried(&window.ctx).is_some(),
        "the drag has to lift"
    );
    let before = entries(&queue);

    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::Escape, true)],
    );
    assert!(
        carry::carried(&window.ctx).is_none(),
        "Escape drops what is carried"
    );
    let _ = window.frame(&mut transport, &mut queue, vec![window.release(to)]);
    assert_eq!(entries(&queue), before, "a cancelled add changes nothing");
}

/// A queued window whose pane the user hid: the view takes the width the
/// panel had, and the panel draws nothing.
#[test]
fn a_hidden_pane_widens_the_view_and_a_shown_one_narrows_it() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1240.0, 900.0));

    let (_, shown) = window.frame(&mut transport, &mut queue, Vec::new());
    let pane = pane_rect_when_shown(&window);
    let narrow = view_rect(&window);
    assert!(
        text_in(&shown, "t2", pane).is_some(),
        "the pane is not drawing its queue"
    );

    window.pane = layout::PanePref::Hidden;
    let (_, hidden) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        text_in(&hidden, "t2", pane).is_none(),
        "a hidden pane still drew a queue row"
    );
    let wide = view_rect(&window);
    assert!(
        wide.width() > narrow.width() + pane.width(),
        "the view took {} of the pane's {}",
        wide.width() - narrow.width(),
        pane.width()
    );
}

#[test]
fn at_900_and_at_1100_a_click_on_the_queue_button_changes_nothing() {
    for width in [900.0, 1100.0] {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(width, 900.0));
        let at = queue_button_at(&window).center();
        let before = entries(&queue);
        assert_eq!(
            layout::pane_button(width, window.pane),
            layout::PaneButton::Unavailable,
            "{width}"
        );

        assert_eq!(window.click(&mut transport, &mut queue, at), None);
        assert_eq!(
            entries(&queue),
            before,
            "{width}: a click on the queue button does nothing"
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("t0"),
            "{width}: a click must not move the transport"
        );
        assert_eq!(
            window.pane,
            layout::PanePref::Auto,
            "{width}: a click where the pane cannot exist changed the choice"
        );
    }
}

#[test]
fn at_1400_a_click_on_the_queue_button_hides_the_pane_and_a_second_shows_it() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        text_in(&frame, "t2", pane_rect_when_shown(&window)).is_some(),
        "the pane is not drawn to begin with"
    );

    let at = queue_button_at(&window).center();
    assert_eq!(window.click(&mut transport, &mut queue, at), None);
    assert_eq!(
        window.pane,
        layout::PanePref::Hidden,
        "the click did not hide the pane"
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        painted_at(&frame, "t2").is_none(),
        "a hidden pane still drew a queue row"
    );

    let at = queue_button_at(&window).center();
    assert_eq!(window.click(&mut transport, &mut queue, at), None);
    assert_eq!(
        window.pane,
        layout::PanePref::Auto,
        "the second click did not show the pane"
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        text_in(&frame, "t2", pane_rect_when_shown(&window)).is_some(),
        "a shown pane drew no queue row"
    );
}

#[test]
fn the_choice_survives_navigation() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_playlists(&mut window, &[("Mix", None)], None);

    let at = queue_button_at(&window).center();
    assert_eq!(window.click(&mut transport, &mut queue, at), None);
    assert_eq!(window.pane, layout::PanePref::Hidden);

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let row = sidebar_row_rect(&window, &frame, "Mix", "Playlist").center();
    let some = window.library.page().cloned();
    assert_eq!(some, None, "the library starts on the root");
    assert_eq!(
        window.click(&mut transport, &mut queue, row),
        Some(Command::LoadTracks {
            playlist: "playlist:Mix".to_owned()
        })
    );
    assert_eq!(
        window.library.page(),
        Some(&Page::Playlist("playlist:Mix".to_owned())),
        "the sidebar row did not navigate"
    );
    assert_eq!(
        window.pane,
        layout::PanePref::Hidden,
        "navigation reset the choice about the pane"
    );

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        painted_at(&frame, "t2").is_none(),
        "the pane came back on the way to another page"
    );
}

#[test]
fn above_1120_with_the_pane_hidden_the_queue_button_takes_a_list_drop() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    window.pane = layout::PanePref::Hidden;
    playlist_page(&mut window, &["p0", "p1"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), "p1");
    let to = queue_button_at(&window).center();

    assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
    assert_eq!(entries(&queue), ["t0", "t1", "t2", "t3", "list:p1"]);
}

#[test]
fn a_drop_that_ends_on_the_button_does_not_toggle_the_pane() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    window.pane = layout::PanePref::Hidden;
    playlist_page(&mut window, &["p0", "p1"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), "p1");
    let to = queue_button_at(&window).center();

    let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);
    assert!(
        claimed_fill(&frame, &window).is_some(),
        "a hidden pane's button is not the drop target"
    );
    let (_, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);

    assert_eq!(entries(&queue), ["t0", "t1", "t2", "t3", "list:p1"]);
    assert_eq!(
        window.pane,
        layout::PanePref::Hidden,
        "the release that ended the drop toggled the pane"
    );
}

#[test]
fn a_resize_to_a_wider_window_shows_the_pane_under_auto_and_still_hides_it_under_hidden() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);

    let mut window = Window::new(Vec2::new(1100.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        painted_at(&frame, "t2").is_none(),
        "the pane is on screen below its own threshold"
    );
    window.size = Vec2::new(1400.0, 900.0);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        text_in(&frame, "t2", pane_rect_when_shown(&window)).is_some(),
        "a wider window under auto kept the pane hidden"
    );

    window.pane = layout::PanePref::Hidden;
    window.size = Vec2::new(1100.0, 900.0);
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    assert_eq!(window.pane, layout::PanePref::Hidden);
    window.size = Vec2::new(1400.0, 900.0);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        painted_at(&frame, "t2").is_none(),
        "a wider window under a hidden choice showed the pane"
    );
}

/// The passes a page list's lift takes: the pointer arrives, presses, and
/// jumps past egui's threshold in one move.
fn carried_frame_events(from: egui::Pos2, to: egui::Pos2) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(from),
        crate::view::test_support::button(from, true),
        egui::Event::PointerMoved(to),
    ]
}

/// The rect the ghost is drawn in: the pointer offset, and the carried row's
/// own size less that offset.
fn ghost_rect(pointer: egui::Pos2, size: Vec2) -> Rect {
    Rect::from_min_size(
        pointer + Vec2::new(space::S3, space::S4),
        Vec2::new((size.x - space::S3).max(0.0), size.y),
    )
}

/// The fill painted into the box the ghost occupies.
fn ghost_fill(output: &egui::FullOutput, rect: Rect) -> Option<Color32> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape)
                if (shape.rect.min - rect.min).length() < 0.5
                    && (shape.rect.size() - rect.size()).length() < 0.5 =>
            {
                Some(shape.fill)
            }
            _ => None,
        })
}
