use super::super::*;
use super::test_support::*;
use crate::player::{Transport, TransportState};
use crate::queue::Queue;
use crate::theme::extent;
use takofy_model::TrackEntry;

/// The playbar's fullscreen button: the last of the extras, which the
/// columns and the run tables both end with.
fn fullscreen_button_at(window: &Window) -> Rect {
    let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
    let regions = layout::regions_with(content.width(), window.pane);
    let shell = layout::shell_for(&regions);
    let bar = layout::grid_for(content, &shell)
        .playbar
        .shrink(stroke::HAIRLINE);
    let [_, _, extras] = layout::playbar_for(bar, regions.playbar).columns(bar);
    Rect::from_min_size(
        egui::pos2(
            extras.right() - extent::BUTTON,
            extras.center().y - extent::BUTTON / 2.0,
        ),
        Vec2::splat(extent::BUTTON),
    )
}

#[test]
fn fullscreen_click_sends_fullscreen_true_when_the_window_reports_false() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    window.fullscreen = Some(false);

    let commands = fullscreen_clicks(&mut window, &mut transport, &mut queue);

    assert!(
        commands.contains(&egui::ViewportCommand::Fullscreen(true)),
        "the click asked for no fullscreen: {commands:?}"
    );
}

#[test]
fn fullscreen_click_sends_fullscreen_false_when_the_window_reports_true() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    window.fullscreen = Some(true);

    let commands = fullscreen_clicks(&mut window, &mut transport, &mut queue);

    assert!(
        commands.contains(&egui::ViewportCommand::Fullscreen(false)),
        "the click asked for no exit: {commands:?}"
    );
}

#[test]
fn fullscreen_click_sends_nothing_when_the_state_is_unknown() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    window.fullscreen = None;

    let commands = fullscreen_clicks(&mut window, &mut transport, &mut queue);

    assert!(
        !commands
            .iter()
            .any(|command| matches!(command, egui::ViewportCommand::Fullscreen(_))),
        "a window that cannot say whether it is fullscreen was asked anyway: {commands:?}"
    );
}

#[test]
fn a_window_manager_flip_is_read_not_remembered() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    window.fullscreen = Some(false);
    let at = fullscreen_button_at(&window);
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        !glyph_colours_in(&window.frame(&mut transport, &mut queue, Vec::new()).1, at)
            .contains(&Tokens::DARK.palette.accent),
        "a windowed window lit the fullscreen button"
    );

    // The compositor's own key, with no click from the app.
    window.fullscreen = Some(true);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        glyph_colours_in(&frame, at).contains(&Tokens::DARK.palette.accent),
        "a fullscreen window left the button unlit"
    );
    assert_eq!(
        viewport_commands(&frame)
            .iter()
            .filter(|command| matches!(command, egui::ViewportCommand::Fullscreen(_)))
            .count(),
        0,
        "a window-manager change was answered with a request of our own"
    );

    window.fullscreen = Some(false);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        !glyph_colours_in(&frame, at).contains(&Tokens::DARK.palette.accent),
        "the button stayed lit after the window left fullscreen"
    );
}

#[test]
fn a_refused_request_leaves_the_button_unlit_and_the_next_click_asks_again() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    window.fullscreen = Some(false);
    let at = fullscreen_button_at(&window);

    let first = fullscreen_clicks(&mut window, &mut transport, &mut queue);
    assert!(
        first.contains(&egui::ViewportCommand::Fullscreen(true)),
        "the first click asked for nothing: {first:?}"
    );

    // The compositor refused, so the window still reports itself windowed.
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        !glyph_colours_in(&frame, at).contains(&Tokens::DARK.palette.accent),
        "a refused request lit the button anyway"
    );

    let second = fullscreen_clicks(&mut window, &mut transport, &mut queue);
    assert!(
        second.contains(&egui::ViewportCommand::Fullscreen(true)),
        "the second click did not ask again: {second:?}"
    );
}

#[test]
fn the_availability_flip_does_not_change_the_narrow_look() {
    for width in [900.0, 1100.0] {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(width, 900.0));
        let at = queue_button_at(&window);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

        let colours = glyph_colours_in(&frame, at);
        assert!(!colours.is_empty(), "{width}: the button drew no glyph");
        for colour in colours {
            assert_eq!(
                colour,
                Tokens::DARK.palette.text_disabled,
                "{width}: a button with no pane to toggle is not drawn disabled"
            );
        }
    }
}

#[test]
fn below_the_now_playing_width_a_drop_off_the_queue_button_is_refused() {
    // The queue column is hidden below this width, so only the playbar's
    // queue button takes a drop here.
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1100.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), "p1");
    // Above the rows: off the queue button and off the page's own list.
    let to = egui::pos2(window.size.x - 80.0, view_rect(&window).top() + 8.0);
    let before = entries(&queue);

    let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);
    assert_eq!(
        ghost_at(&frame, to),
        Some(Tokens::DARK.palette.surface),
        "a narrow window paints the ghost flat"
    );
    assert!(
        shadows(&frame).is_empty(),
        "a flat ghost casts no shadow: {:?}",
        shadows(&frame)
    );
    assert!(marker(&frame).is_none(), "no target, no marker");
    assert!(
        claimed_fill(&frame, &window).is_none(),
        "the pointer is off the queue button"
    );

    let release = window.release(to);
    let _ = window.frame(&mut transport, &mut queue, vec![release]);
    assert_eq!(entries(&queue), before, "a refused drop changes nothing");
}

#[test]
fn after_a_drop_the_landed_row_is_in_view_and_the_list_does_not_jump() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(24, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let pane = pane_rect_when_shown(&window);
    let from = row_line(&frame, pane, "t2");
    let to = below(row_line(&frame, pane, "t5"), -0.25);

    let _ = window.drag(&mut transport, &mut queue, from, to);
    assert_eq!(
        entries(&queue).iter().position(|uri| uri == "t2"),
        Some(4),
        "the row landed below t4"
    );
    let (_, first) = window.frame(&mut transport, &mut queue, Vec::new());
    let (_, second) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = text_in(&first, "t2", pane).expect("the landed row is drawn");
    let after = text_in(&second, "t2", pane).expect("the landed row is still drawn");
    let id = window
        .ctx
        .memory(|memory| memory.focused())
        .expect("the landed row takes the keyboard");
    let row = window
        .ctx
        .read_response(id)
        .expect("the landed row was laid out")
        .rect;
    assert!(
        row.contains(after.center()),
        "the keyboard went somewhere else: {row:?}"
    );
    assert!(
        ringed_row(&second).is_none(),
        "a pointer drop shows no ring"
    );
    assert!(
        pane.contains_rect(at),
        "the row just placed is not in view: {at:?}"
    );
    assert_eq!(
        at.min.y, after.min.y,
        "the list moved under a row that was already in view"
    );
}

#[test]
fn autoscroll_advances_the_boundary_while_the_marker_stays_at_the_edge() {
    // How far the dragged track lands when the pointer is held at the pane's
    // bottom edge for `frames` frames, and the markers that were drawn.
    let run = |frames: usize| {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(24, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let pane = pane_rect_when_shown(&window);
        let from = row_line(&frame, pane, "t2");
        let to = egui::pos2(pane.center().x, pane.bottom() - 4.0);
        let _ = window.hold_drag(&mut transport, &mut queue, from, to);

        let mut marks = Vec::new();
        for _ in 0..frames {
            let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
            marks.push(marker(&frame).expect("a held block draws its marker"));
        }
        let release = window.release(to);
        let _ = window.frame(&mut transport, &mut queue, vec![release]);

        let landed = entries(&queue)
            .iter()
            .position(|uri| uri == "t2")
            .expect("the dragged entry is still queued");
        (landed, marks, pane)
    };

    let (quick, _, _) = run(0);
    let (slow, marks, pane) = run(30);
    assert!(
        slow > quick,
        "holding at the edge must scroll the boundary further: {quick} then {slow}"
    );
    for mark in &marks {
        assert!(
            mark.top() >= pane.top() && mark.bottom() <= pane.bottom() + 0.5,
            "a marker left the pane: {mark:?} in {pane:?}"
        );
        assert!(
            mark.top() > pane.bottom() - QUEUE_ROW_H,
            "a held marker is not at the edge: {mark:?}"
        );
    }
}

#[test]
fn alt_down_and_alt_up_move_the_selected_rows() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(6, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let pane = pane_rect_when_shown(&window);
    clicked_row(&mut window, &mut queue, &mut transport, pane, "t2");
    window.held = egui::Modifiers::COMMAND;
    clicked_row(&mut window, &mut queue, &mut transport, pane, "t3");
    window.held = egui::Modifiers::NONE;
    assert!(queue.selected(2) && queue.selected(3));

    assert_eq!(
        chord(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::ArrowDown,
            egui::Modifiers::ALT
        ),
        None
    );
    assert_eq!(entries(&queue), ["t0", "t1", "t4", "t2", "t3", "t5"]);

    assert_eq!(
        chord(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::ArrowUp,
            egui::Modifiers::ALT
        ),
        None
    );
    assert_eq!(entries(&queue), ["t0", "t1", "t2", "t3", "t4", "t5"]);
}

#[test]
fn alt_arrows_move_the_focused_row_when_nothing_is_selected() {
    let mut transport = playing(cover("abc"));
    let tracks: Vec<TrackEntry> = (0..6).map(|index| track(index, None)).collect();
    let mut queue = Queue::default();
    queue.start(&tracks, 0, None, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let pane = pane_rect_when_shown(&window);

    clicked_row(&mut window, &mut queue, &mut transport, pane, "t2");
    // A click always picks its row, so the pick is cleared here to leave only
    // the keyboard on it.
    let _ = queue.start(&tracks, 0, None, &mut transport);
    assert!(!queue.selected(2), "nothing is picked");

    assert_eq!(
        chord(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::ArrowUp,
            egui::Modifiers::ALT
        ),
        None
    );
    assert_eq!(entries(&queue), ["t0", "t2", "t1", "t3", "t4", "t5"]);
}

#[test]
fn alt_arrows_do_nothing_when_a_queue_row_is_not_focused() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(5, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let before = entries(&queue);
    let field = search_field(&frame).expect("the field is painted");
    let _ = window.click(&mut transport, &mut queue, field.center());

    assert_eq!(
        chord(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::ArrowDown,
            egui::Modifiers::ALT
        ),
        None
    );
    assert_eq!(entries(&queue), before, "no row holds the keyboard");
}

#[test]
fn ctrl_alt_arrows_are_ignored() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(5, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let pane = pane_rect_when_shown(&window);
    clicked_row(&mut window, &mut queue, &mut transport, pane, "t2");
    let before = entries(&queue);

    let ctrl_alt = egui::Modifiers {
        alt: true,
        ctrl: true,
        ..egui::Modifiers::NONE
    };
    assert_eq!(
        chord(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::ArrowDown,
            ctrl_alt
        ),
        None
    );
    assert_eq!(entries(&queue), before, "Ctrl+Alt+↓ is somebody else's");
}

#[test]
fn focus_follows_the_moved_row_and_enter_plays_it() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(5, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let pane = pane_rect_when_shown(&window);
    clicked_row(&mut window, &mut queue, &mut transport, pane, "t2");

    let _ = chord(
        &mut window,
        &mut transport,
        &mut queue,
        egui::Key::ArrowDown,
        egui::Modifiers::ALT,
    );
    assert_eq!(entries(&queue), ["t0", "t1", "t3", "t2", "t4"]);

    assert_eq!(
        chord(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::Enter,
            egui::Modifiers::NONE
        ),
        Some(Command::Play {
            uri: "t2".to_owned()
        }),
        "the row the move landed on is the one Enter plays"
    );
}

#[test]
fn a_held_alt_arrow_moves_the_row_once_per_frame_and_keeps_the_keyboard_on_it() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(5, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let pane = pane_rect_when_shown(&window);
    clicked_row(&mut window, &mut queue, &mut transport, pane, "t2");

    // A held key: the press and its auto-repeat arrive on separate frames.
    let alt = egui::Modifiers::ALT;
    window.held = alt;
    for event in [
        holding(key(egui::Key::ArrowDown, true), alt),
        holding(key_repeat(egui::Key::ArrowDown), alt),
    ] {
        let _ = window.frame(&mut transport, &mut queue, vec![event]);
    }
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::ArrowDown, false)],
    );
    window.held = egui::Modifiers::NONE;

    assert_eq!(
        entries(&queue),
        ["t0", "t1", "t3", "t4", "t2"],
        "each frame moved the row one place"
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let ring = ringed_row(&frame).expect("the moved row holds the keyboard");
    assert!(
        ring.contains(row_line(&frame, pane, "t2")),
        "the keyboard stayed on the index the row left: {ring:?}"
    );
    assert_eq!(
        chord(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::Enter,
            egui::Modifiers::NONE
        ),
        Some(Command::Play {
            uri: "t2".to_owned()
        }),
        "Enter plays the row the repeat moved"
    );
}

#[test]
fn alt_arrow_at_the_boundary_row_does_not_move_focus_and_shows_no_ring() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(5, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let pane = pane_rect_when_shown(&window);
    // The pane lists what follows the current entry, so t1 is its first row
    // and the top of what Alt+↑ can move.
    clicked_row(&mut window, &mut queue, &mut transport, pane, "t1");

    let _ = chord(
        &mut window,
        &mut transport,
        &mut queue,
        egui::Key::ArrowUp,
        egui::Modifiers::ALT,
    );
    assert_eq!(
        entries(&queue),
        ["t0", "t1", "t2", "t3", "t4"],
        "the first row of the list has nowhere to go"
    );

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        ringed_row(&frame).is_none(),
        "focus did not move, so the ring stays hidden"
    );
    assert_eq!(
        chord(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::Enter,
            egui::Modifiers::NONE
        ),
        Some(Command::Play {
            uri: "t1".to_owned()
        }),
        "the keyboard is still on the row that was clicked"
    );
}

#[test]
fn alt_arrows_leave_the_playbar_and_the_search_field_alone() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(5, &mut transport);
    // A seek needs a length and a state, and starting the queue clears both.
    transport.state = TransportState::Playing;
    transport.duration_ms = Some(100_000);
    transport.position_ms = 10_000;
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let pane = pane_rect_when_shown(&window);
    clicked_row(&mut window, &mut queue, &mut transport, pane, "t2");

    // Tab reaches the seek slider, and only the slider turns Home into a seek.
    let mut reached = false;
    for _ in 0..24 {
        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Tab);
        if press(&mut window, &mut transport, &mut queue, egui::Key::Home)
            == Some(Command::Seek { position_ms: 0 })
        {
            reached = true;
            break;
        }
    }
    assert!(reached, "Tab never reached the seek slider");
    // The slider locks its arrows a frame after it takes focus.
    let (_, _) = window.frame(&mut transport, &mut queue, Vec::new());
    assert_eq!(
        chord(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::ArrowRight,
            egui::Modifiers::SHIFT
        ),
        Some(Command::Seek {
            position_ms: 30_000
        }),
        "Shift still makes one arrow press four seek steps"
    );

    // The shell leaves the seek's keyboard alone: an Alt chord there moves no
    // queue row.
    let held = entries(&queue);
    for arrow in [egui::Key::ArrowRight, egui::Key::ArrowDown] {
        let _ = chord(
            &mut window,
            &mut transport,
            &mut queue,
            arrow,
            egui::Modifiers::ALT,
        );
    }
    assert_eq!(
        entries(&queue),
        held,
        "an Alt chord on the seek moves no queue row"
    );

    // The field keeps its own arrows: Alt is not the shell's to take.
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let before = entries(&queue);
    let field = search_field(&frame).expect("the field is painted");
    let _ = window.click(&mut transport, &mut queue, field.center());
    assert_eq!(
        chord(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::ArrowDown,
            egui::Modifiers::ALT
        ),
        None
    );
    assert_eq!(entries(&queue), before);
}

/// Every click's passes, flattened, for the assertions below.
fn fullscreen_clicks(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
) -> Vec<egui::ViewportCommand> {
    let at = fullscreen_button_at(window).center();
    window.click_viewport(transport, queue, at)
}

/// The colours a frame painted inside `rect` in glyph shapes. The wash, the
/// fills and the focus ring are `Shape::Rect`, so the backgrounds are
/// excluded by kind.
fn glyph_colours_in(output: &egui::FullOutput, rect: Rect) -> Vec<Color32> {
    let mut colours = Vec::new();
    for clipped in &output.shapes {
        if !rect.contains(clipped.shape.visual_bounding_rect().center()) {
            continue;
        }
        match &clipped.shape {
            egui::Shape::Path(path) => {
                if path.fill != Color32::TRANSPARENT {
                    colours.push(path.fill);
                }
                if let egui::epaint::ColorMode::Solid(color) = &path.stroke.color
                    && path.stroke.width > 0.0
                {
                    colours.push(*color);
                }
            }
            egui::Shape::Circle(circle) => colours.push(circle.fill),
            _ => {}
        }
    }
    colours
}
