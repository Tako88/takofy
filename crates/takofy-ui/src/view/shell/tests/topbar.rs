use super::super::*;
use super::test_support::*;
use crate::library::Load;
use crate::library_filter::RootKind;
use crate::player::Transport;
use crate::queue::Queue;
use crate::route::Page;
use crate::theme::extent;

/// The bottom rule the topbar paints, which spells out the width it drew at.
pub(super) fn topbar_rule(frame: &egui::FullOutput, bar: Rect) -> Rect {
    frame
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape)
                if shape.stroke.width == 0.0 && (shape.rect.top() - bar.bottom()).abs() < 0.01 =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
        .expect("the topbar paints its bottom rule")
}

/// The middle of a three-page history: album A on screen, the playlist
/// behind it and album B ahead.
pub(super) fn history_of_three(window: &mut Window) {
    ready_playlist(window);
    window.library.open(Page::Album(ALBUM.to_owned()));
    window.library.open(Page::Album(ALBUM_B.to_owned()));
    window.library.back();
}

/// In drawer mode the layout hands the toggle a whole column; the button in
/// it is still a button, the same square as its neighbours.
#[test]
fn the_drawer_toggle_is_a_button_not_a_column() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(800.0, 900.0));
    ready_playlist(&mut window);

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let bar = topbar_rect(&window);
    let buttons = topbar_buttons(&frame);
    assert_eq!(
        buttons.len(),
        5,
        "the toggle, Home, Your Library, back and forward"
    );
    for button in &buttons {
        assert!(
            (button.height() - extent::BUTTON).abs() < 0.01,
            "a topbar button {button:?} is not one button tall"
        );
    }

    // Nothing drawn as a frame in the bar is taller than a button: a frame
    // stretched to the region came from placing a control on the column
    // rather than in it. The search field is the one box with its own height.
    for clipped in &frame.shapes {
        if let egui::Shape::Rect(shape) = &clipped.shape
            && shape.stroke.width > 0.0
            && bar.contains_rect(shape.rect)
            && (shape.rect.height() - extent::SEARCH_H).abs() > 0.01
        {
            assert!(
                shape.rect.height() <= extent::BUTTON + 0.01,
                "the topbar draws a {:?} frame, taller than a button",
                shape.rect
            );
        }
    }
}

#[test]
fn every_topbar_item_shares_one_baseline() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    ready_playlist(&mut window);

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let bar = topbar_rect(&window).center().y;
    let buttons = topbar_buttons(&frame);
    assert_eq!(buttons.len(), 4, "Home, Your Library, back and forward");
    for button in &buttons {
        assert!(
            (button.center().y - bar).abs() < 0.01,
            "a topbar button at {:?} is not on the bar's centre {bar}",
            button.center()
        );
    }
    let field = search_field(&frame).expect("the topbar paints the search field");
    assert!(
        (field.center().y - bar).abs() < 0.01,
        "the search field at {:?} is not on the bar's centre {bar}",
        field.center()
    );
}

#[test]
fn the_topbar_widens_when_the_pane_is_hidden() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    ready_playlist(&mut window);

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let shown = topbar_rect(&window);
    let shown_rule = topbar_rule(&frame, shown);
    assert!(
        (shown_rule.width() - (shown.width() - 2.0 * stroke::HAIRLINE)).abs() < 0.01,
        "the topbar drew {shown_rule:?}, not the region it solved {shown:?}"
    );

    window.pane = layout::PanePref::Hidden;
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let hidden = topbar_rect(&window);
    let hidden_rule = topbar_rule(&frame, hidden);

    assert!(
        hidden_rule.width() > shown_rule.width(),
        "the hidden pane left the topbar at the width it had beside it: \
         {hidden_rule:?} against {shown_rule:?}"
    );
    assert!(
        (hidden_rule.width() - (hidden.width() - 2.0 * stroke::HAIRLINE)).abs() < 0.01,
        "the topbar region {hidden:?} disagrees with the bar it drew {hidden_rule:?}"
    );
}

#[test]
fn the_history_buttons_move_between_pages() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    ready_playlist(&mut window);
    window.library.open(Page::Album(ALBUM.to_owned()));

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let buttons = topbar_buttons(&frame);
    assert_eq!(
        buttons.len(),
        4,
        "the topbar paints Home, Your Library, back and forward"
    );

    assert_eq!(
        window.click(&mut transport, &mut queue, buttons[2].center()),
        None,
        "back into a ready playlist needs no load"
    );
    assert_eq!(
        window.library.page(),
        Some(&Page::Playlist(PLAYLIST.to_owned()))
    );

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let buttons = topbar_buttons(&frame);
    assert_eq!(
        window.click(&mut transport, &mut queue, buttons[3].center()),
        None
    );
    assert_eq!(window.library.page(), Some(&Page::Album(ALBUM.to_owned())));
}

#[test]
fn the_history_buttons_do_nothing_with_no_history() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let buttons = topbar_buttons(&frame);
    assert_eq!(
        buttons.len(),
        4,
        "the topbar paints Home, Your Library, back and forward"
    );

    // Only the history pair: Home and Your Library are destinations, and a
    // press on either one moves the library.
    for button in &buttons[2..] {
        assert_eq!(
            window.click(&mut transport, &mut queue, button.center()),
            None
        );
        assert_eq!(window.library.page(), None);
    }
}

#[test]
fn the_back_side_button_goes_back() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    history_of_three(&mut window);

    let (command, frame) = window.frame(
        &mut transport,
        &mut queue,
        vec![side(egui::Pos2::ZERO, egui::PointerButton::Extra1, true)],
    );
    assert_eq!(command, None, "back onto a ready playlist needs no load");
    assert_eq!(
        window.library.page(),
        Some(&Page::Playlist(PLAYLIST.to_owned()))
    );
    assert!(
        text_in(&frame, "Mix", view_rect(&window)).is_some(),
        "the view paints the playlist in the frame the nav applies"
    );

    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![side(egui::Pos2::ZERO, egui::PointerButton::Extra1, false)],
    );
}

#[test]
fn the_forward_side_button_goes_forward() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    history_of_three(&mut window);
    window.library.back();
    assert_eq!(
        window.library.page(),
        Some(&Page::Playlist(PLAYLIST.to_owned()))
    );

    // The control frame: the predicate the next frame asserts on is live.
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(text_in(&frame, "Mix", view_rect(&window)).is_some());

    let (command, frame) = window.frame(
        &mut transport,
        &mut queue,
        vec![side(egui::Pos2::ZERO, egui::PointerButton::Extra2, true)],
    );
    assert_eq!(command, None);
    assert_eq!(window.library.page(), Some(&Page::Album(ALBUM.to_owned())));
    // The album page draws a spinner until its load settles, and a band and
    // a table after that. Its load has only just started here, so the
    // same-frame proof that the view moved is that the playlist band is no
    // longer painted in the view's own rect. The sidebar and topbar are
    // excluded: both draw before the nav is applied.
    assert!(
        text_in(&frame, "Mix", view_rect(&window)).is_none(),
        "the view leaves the playlist in the frame the nav applies"
    );

    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![side(egui::Pos2::ZERO, egui::PointerButton::Extra2, false)],
    );
}

#[test]
fn browser_back_and_forward_keys_do_the_same() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    history_of_three(&mut window);

    let (command, frame) = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::BrowserBack, true)],
    );
    assert_eq!(command, None, "back onto a ready playlist needs no load");
    assert_eq!(
        window.library.page(),
        Some(&Page::Playlist(PLAYLIST.to_owned()))
    );
    assert!(text_in(&frame, "Mix", view_rect(&window)).is_some());

    // A fresh window, so the two cases share no input state.
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    history_of_three(&mut window);
    window.library.back();

    // The control frame: the predicate the next frame asserts on is live.
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(text_in(&frame, "Mix", view_rect(&window)).is_some());

    let (command, frame) = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::BrowserForward, true)],
    );
    assert_eq!(command, None);
    assert_eq!(window.library.page(), Some(&Page::Album(ALBUM.to_owned())));
    assert!(text_in(&frame, "Mix", view_rect(&window)).is_none());
}

/// Auto-repeat is intended, the way a browser behaves: a held Back key
/// steps back once per repeat, so each frame moves exactly one page rather
/// than draining the history in one go.
#[test]
fn a_held_browser_key_steps_back_once_per_frame() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    ready_playlist(&mut window);
    window.library.open(Page::Album(ALBUM.to_owned()));
    window.library.open(Page::Album(ALBUM_B.to_owned()));

    let (command, _frame) = window.frame(
        &mut transport,
        &mut queue,
        vec![key_repeat(egui::Key::BrowserBack)],
    );
    assert_eq!(
        command,
        Some(Command::LoadAlbum {
            album: ALBUM.to_owned()
        }),
        "album B's want superseded album A's, so Back asks for A again"
    );
    assert_eq!(
        window.library.page(),
        Some(&Page::Album(ALBUM.to_owned())),
        "one repeat takes one step back, not two"
    );

    let (command, _frame) = window.frame(
        &mut transport,
        &mut queue,
        vec![key_repeat(egui::Key::BrowserBack)],
    );
    assert_eq!(command, None, "back onto a ready playlist needs no load");
    assert_eq!(
        window.library.page(),
        Some(&Page::Playlist(PLAYLIST.to_owned())),
        "the next repeat takes the next step, one page per frame"
    );
}

#[test]
fn side_button_back_into_a_stale_playlist_loads_it() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    stale_playlist_behind_an_album(&mut window.library);

    let (command, _frame) = window.frame(
        &mut transport,
        &mut queue,
        vec![side(egui::Pos2::ZERO, egui::PointerButton::Extra1, true)],
    );
    assert_eq!(
        command,
        Some(Command::LoadTracks {
            playlist: PLAYLIST.to_owned()
        })
    );
    assert_eq!(window.library.tracks, Load::Loading);

    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![side(egui::Pos2::ZERO, egui::PointerButton::Extra1, false)],
    );
}

#[test]
fn back_beats_forward_in_one_frame() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    history_of_three(&mut window);

    let (command, frame) = window.frame(
        &mut transport,
        &mut queue,
        vec![
            side(egui::Pos2::ZERO, egui::PointerButton::Extra2, true),
            side(egui::Pos2::ZERO, egui::PointerButton::Extra1, true),
        ],
    );
    assert_eq!(command, None);
    assert_eq!(
        window.library.page(),
        Some(&Page::Playlist(PLAYLIST.to_owned())),
        "Back is checked first, so one frame gives one action"
    );
    assert!(text_in(&frame, "Mix", view_rect(&window)).is_some());

    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![
            side(egui::Pos2::ZERO, egui::PointerButton::Extra1, false),
            side(egui::Pos2::ZERO, egui::PointerButton::Extra2, false),
        ],
    );
}

#[test]
fn side_buttons_do_nothing_with_no_history() {
    let cases = [
        (
            side(egui::Pos2::ZERO, egui::PointerButton::Extra1, true),
            Some(side(egui::Pos2::ZERO, egui::PointerButton::Extra1, false)),
        ),
        (
            side(egui::Pos2::ZERO, egui::PointerButton::Extra2, true),
            Some(side(egui::Pos2::ZERO, egui::PointerButton::Extra2, false)),
        ),
        (key(egui::Key::BrowserBack, true), None),
        (key(egui::Key::BrowserForward, true), None),
    ];

    for (input, release) in cases {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        let (command, _frame) = window.frame(&mut transport, &mut queue, vec![input]);
        assert_eq!(command, None, "there is nowhere to go");
        assert_eq!(window.library.page(), None);
        assert_eq!(window.library.tracks, Load::Idle);

        if let Some(release) = release {
            let _ = window.frame(&mut transport, &mut queue, vec![release]);
        }
    }
}

#[test]
fn releases_and_other_buttons_are_ignored() {
    let cases = [
        side(egui::Pos2::ZERO, egui::PointerButton::Extra1, false),
        side(egui::Pos2::ZERO, egui::PointerButton::Primary, true),
        side(egui::Pos2::ZERO, egui::PointerButton::Secondary, true),
        side(egui::Pos2::ZERO, egui::PointerButton::Middle, true),
        key(egui::Key::Escape, true),
        key(egui::Key::ArrowLeft, true),
        key(egui::Key::BrowserBack, false),
    ];

    for input in cases {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        history_of_three(&mut window);

        let (command, _frame) = window.frame(&mut transport, &mut queue, vec![input]);
        assert_eq!(command, None);
        assert_eq!(
            window.library.page(),
            Some(&Page::Album(ALBUM.to_owned())),
            "only a press of a nav input moves the page"
        );
    }
}

/// The end-to-end pin of click-wins: one frame carries a primary press, a
/// primary release and an `Extra2` press, and the click's Back must win over
/// the side button's Forward.
///
/// The `Extra2` press lands at the click's own position, which is where it
/// would arrive on real hardware, so the frame is shaped like a real one. The
/// second window is the control: the same events with the click on the inert
/// search field, so only the `Extra2` press can act, and it must move
/// Forward. That proves the side press is live in the frame where Back won,
/// so the test cannot pass with the side buttons unwired.
#[test]
fn a_topbar_click_beats_a_side_button() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    history_of_three(&mut window);

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let back = topbar_buttons(&frame)[2];
    assert!(window.library.can_back(), "history exists, so Back is live");

    // The hover frame, so frame 3's hit test sees the button there.
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![egui::Event::PointerMoved(back.center())],
    );

    let (command, frame) = window.frame(
        &mut transport,
        &mut queue,
        vec![
            crate::view::test_support::button(back.center(), true),
            crate::view::test_support::button(back.center(), false),
            side(back.center(), egui::PointerButton::Extra2, true),
        ],
    );
    assert_eq!(command, None);
    assert_eq!(
        window.library.page(),
        Some(&Page::Playlist(PLAYLIST.to_owned())),
        "the click's Back wins over the side button's Forward"
    );
    assert!(text_in(&frame, "Mix", view_rect(&window)).is_some());

    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![side(back.center(), egui::PointerButton::Extra2, false)],
    );

    // The liveness control: no click to win, so the same-frame `Extra2`
    // press must move Forward. The click lands on the topbar's right edge,
    // where the search field paints no widget.
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    history_of_three(&mut window);
    let inert = topbar_rect(&window).right_center() - Vec2::new(40.0, 0.0);

    let (command, _frame) = window.frame(
        &mut transport,
        &mut queue,
        vec![
            crate::view::test_support::button(inert, true),
            crate::view::test_support::button(inert, false),
            side(inert, egui::PointerButton::Extra2, true),
        ],
    );
    assert_eq!(
        command,
        Some(Command::LoadAlbum {
            album: ALBUM_B.to_owned()
        }),
        "album A's want superseded album B's, so Forward asks for B again"
    );
    assert_eq!(
        window.library.page(),
        Some(&Page::Album(ALBUM_B.to_owned())),
        "with no click to win, the same-frame side press moves Forward"
    );
}

/// Home is a page of its own, so leaving it and returning draws a new shelf.
#[test]
fn home_from_a_page_draws_a_new_shelf() {
    let mut library = Library::starting_home(NOW_MS);
    let seed = library.home.seed();
    library.open(Page::Album(ALBUM.to_owned()));
    assert_eq!(library.page(), Some(&Page::Album(ALBUM.to_owned())));

    assert_eq!(
        apply_nav(&mut library, Some(topbar::Nav::Home), None, NOW_MS + 1_000),
        None,
        "Home is local state and asks for no read"
    );
    assert_eq!(library.page(), Some(&Page::Home));
    assert_ne!(library.home.seed(), seed, "a new open is a new draw");
}

/// Home on Home changes nothing, so the cards already on screen stay put.
#[test]
fn home_on_home_keeps_the_shelf_it_drew() {
    let mut library = Library::starting_home(NOW_MS);
    let seed = library.home.seed();

    assert_eq!(
        apply_nav(&mut library, Some(topbar::Nav::Home), None, NOW_MS + 1_000),
        None
    );
    assert_eq!(library.page(), Some(&Page::Home));
    assert_eq!(library.home.seed(), seed, "the draw is kept");
}

/// Your Library returns to the root as it was left, chip and all.
#[test]
fn your_library_from_home_returns_to_the_root_on_its_chip() {
    let mut library = Library::starting_home(NOW_MS);
    library.root_filter.choose(RootKind::Albums);

    assert_eq!(
        apply_nav(&mut library, Some(topbar::Nav::Library), None, NOW_MS),
        None
    );
    assert_eq!(library.page(), None, "the root is on screen");
    assert_eq!(library.root_filter.chosen(), RootKind::Albums);
}

/// A page opened from Home has Home behind it, so Back is a step home.
#[test]
fn back_from_a_page_opened_on_home_returns_to_home() {
    let mut library = Library::starting_home(NOW_MS);
    let seed = library.home.seed();
    library.open(Page::Album(ALBUM.to_owned()));

    assert_eq!(
        apply_nav(&mut library, Some(topbar::Nav::Back), None, NOW_MS + 1_000),
        None
    );
    assert_eq!(library.page(), Some(&Page::Home));
    assert_eq!(library.home.seed(), seed, "Back keeps the draw Home had");
}
