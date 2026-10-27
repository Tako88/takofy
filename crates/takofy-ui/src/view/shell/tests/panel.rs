use super::super::*;
use super::test_support::*;
use crate::message::Update;
use crate::player::Transport;
use crate::queue::Queue;
use crate::route::Page;
use crate::theme::space;
use takofy_model::{ArtistDetail, Release, ReleaseKind};

/// The action row's box once it pins: the page's surface fill at the height
/// it condenses to, inside the view.
fn action_row(output: &egui::FullOutput, window: &Window) -> Option<Rect> {
    let tokens = Tokens::DARK;
    let view = view_rect(window);
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape)
                if shape.fill == tokens.palette.surface
                    && (shape.rect.height() - crate::theme::actionbar::STUCK_H).abs() < 0.01
                    && view.contains_rect(shape.rect) =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
}

#[test]
fn the_panels_queue_reveal_does_not_inherit_the_pages_inset() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(8, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    long_playlist_page(&mut window);

    // A queue row in the panel takes the keyboard, so the panel's rows ask to
    // be seen through the same seam a page's rows do. t2 is far enough down
    // the pane that scrolling the page leaves it where it is.
    let pane = pane_rect_when_shown(&window);
    clicked_row(&mut window, &mut queue, &mut transport, pane, "t2");
    let focused = window
        .ctx
        .memory(|memory| memory.focused())
        .expect("the queue row took the keyboard");

    // The page pins its action row, and the panel draws its rows in the same
    // frame.
    let stuck = stuck_page_frame(&mut window, &mut transport, &mut queue);
    assert!(
        action_row(&stuck, &window).is_some(),
        "the page's action row is stuck before the panel draws"
    );
    let row = window
        .ctx
        .read_response(focused)
        .expect("the focused queue row is still drawn")
        .rect;
    assert!(
        pane.contains_rect(row),
        "the focused queue row left the panel: {row:?}"
    );
    assert_eq!(
        crate::view::pin::inset(&window.ctx),
        0.0,
        "the panel's queue rows were revealed clear of the page's bar"
    );
}

#[test]
fn a_page_without_a_bar_starts_the_frame_at_no_inset() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    // The panel forgets the seam for its own rows, so it is hidden here: no
    // region of this window would otherwise leave the page's inset standing
    // for the next frame to inherit.
    window.pane = layout::PanePref::Hidden;
    long_playlist_page(&mut window);

    let stuck = stuck_page_frame(&mut window, &mut transport, &mut queue);
    assert!(
        action_row(&stuck, &window).is_some(),
        "the page's action row is stuck"
    );
    assert!(
        crate::view::pin::inset(&window.ctx) > 0.0,
        "the stuck page left its height in the seam"
    );

    // The library root is the page with no action row at all.
    window.library.to_playlists();
    let (_, root) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        action_row(&root, &window).is_none(),
        "the library root has no action row"
    );
    assert_eq!(
        crate::view::pin::inset(&window.ctx),
        0.0,
        "a page with no bar inherited the last page's pinned height"
    );
}

/// An artist page with a discography long enough to push its chip row under
/// the bar.
fn long_artist_page(window: &mut Window) {
    let releases: Vec<Release> = (0..8)
        .map(|index| Release {
            uri: format!("spotify:release:{index}"),
            name: format!("Release {index}"),
            kind: if index % 2 == 0 {
                ReleaseKind::Album
            } else {
                ReleaseKind::Ep
            },
            year: Some(2024),
            cover: None,
            tracks: vec![format!("T{index}.1"), format!("T{index}.2")],
        })
        .collect();
    let _ = window.library.open(Page::Artist(ARTIST.to_owned()));
    window.library.apply(&Update::Artist(ArtistDetail {
        uri: ARTIST.to_owned(),
        name: "Vela Moreno".to_owned(),
        portrait: None,
        top_tracks: Vec::new(),
        releases,
        related: Vec::new(),
    }));
}

#[test]
fn an_artist_page_leaves_the_chip_rows_reach_in_the_seam_and_the_panel_forgets_it() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(8, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    long_artist_page(&mut window);
    // The panel clears the seam for its own rows after the page draws, so it
    // is hidden while the page's own reach is read.
    window.pane = layout::PanePref::Hidden;

    let wheel = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -space::S4),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: egui::Modifiers::NONE,
    };
    let at = view_rect(&window).center();
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![egui::Event::PointerMoved(at)],
    );
    let mut output = window.frame(&mut transport, &mut queue, Vec::new()).1;
    for _ in 0..200 {
        if crate::view::pin::inset(&window.ctx) > crate::theme::actionbar::STUCK_H + 0.5 {
            break;
        }
        output = window
            .frame(&mut transport, &mut queue, vec![wheel.clone()])
            .1;
    }
    action_row(&output, &window).expect("the action row pins");
    let reach = crate::view::pin::inset(&window.ctx);
    assert!(
        reach > crate::theme::actionbar::STUCK_H + 0.5,
        "the chip row reaches further than the bar: {reach}"
    );
    assert!(
        reach < view_rect(&window).height(),
        "the chip row is under the bar, not the whole view: {reach}"
    );

    // The panel's rows are not under the page's chip row. A focused queue
    // row asks to be seen, and the panel clears the seam before it draws.
    window.pane = layout::PanePref::Auto;
    let pane = pane_rect_when_shown(&window);
    clicked_row(&mut window, &mut queue, &mut transport, pane, "t2");
    let focused = window
        .ctx
        .memory(|memory| memory.focused())
        .expect("the queue row took the keyboard");
    let (_, panel) = window.frame(&mut transport, &mut queue, Vec::new());
    // The page still held its chip row on the frame the panel drew, so the
    // seam the panel cleared is one the page raised this frame and not a
    // stale value from an earlier one.
    let bar = action_row(&panel, &window).expect("the action row is still pinned");
    let band = chip_band(&panel, &window).expect("the chip row is still pinned");
    assert!(
        (band.top() - bar.bottom()).abs() < 0.5,
        "the page did not raise its chip row on the frame the panel cleared: {band:?} against {bar:?}"
    );
    let row = window
        .ctx
        .read_response(focused)
        .expect("the focused queue row is still drawn")
        .rect;
    assert!(
        pane.contains_rect(row),
        "the focused queue row left the panel: {row:?}"
    );
    assert_eq!(
        crate::view::pin::inset(&window.ctx),
        0.0,
        "the panel's queue rows were revealed clear of the artist's chip row"
    );
}

/// The chip row's own surface once it pins: a band taller than the bar's
/// condensed height, so the two fills are told apart.
fn chip_band(output: &egui::FullOutput, window: &Window) -> Option<Rect> {
    let tokens = Tokens::DARK;
    let view = view_rect(window);
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape)
                if shape.fill == tokens.palette.surface
                    && (shape.rect.height() - crate::theme::actionbar::STUCK_H).abs() > 0.01
                    && view.contains_rect(shape.rect) =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
}

/// A playlist page long enough to scroll its band and action row away.
fn long_playlist_page(window: &mut Window) {
    let titles: Vec<String> = (0..60).map(|index| format!("p{index}")).collect();
    let titles: Vec<&str> = titles.iter().map(String::as_str).collect();
    playlist_page(window, &titles);
}

/// Wheels the view until its action row pins, answering the frame it pinned
/// on.
fn stuck_page_frame(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
) -> egui::FullOutput {
    let at = view_rect(window).center();
    let _ = window.frame(transport, queue, vec![egui::Event::PointerMoved(at)]);
    let wheel = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -600.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: egui::Modifiers::NONE,
    };
    let mut output = window.frame(transport, queue, Vec::new()).1;
    for _ in 0..8 {
        if action_row(&output, window).is_some() {
            return output;
        }
        output = window.frame(transport, queue, vec![wheel.clone()]).1;
    }
    panic!("the page's action row never pinned");
}
