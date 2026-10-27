use super::super::*;
use super::test_support::*;
use crate::menu::{Anchor, Device, Door, Gate, Listing, Request};
use crate::theme::{fonts, style};
use egui::{Pos2, RawInput, Rect, pos2, vec2};

#[test]
fn the_menu_stays_inside_the_minimum_window_from_every_corner() {
    let window = Rect::from_min_size(
        Pos2::ZERO,
        vec2(crate::layout::WINDOW_MIN[0], crate::layout::WINDOW_MIN[1]),
    );
    let corners = [
        pos2(0.0, 0.0),
        pos2(window.width() - 28.0, 0.0),
        pos2(0.0, window.height() - 28.0),
        pos2(window.width() - 28.0, window.height() - 28.0),
    ];
    for corner in corners {
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&Tokens::DARK));
        let mut menu = Menu::default();
        assert!(menu.open(
            Request {
                door: Door {
                    anchor: Anchor::Button(Rect::from_min_size(corner, vec2(28.0, 28.0))),
                    opener: opener(),
                    device: Device::Pointer,
                },
                playlists: Vec::new(),
                items: items(),
                targets: vec![track()],
                rows: vec![0],
                listing: Listing::Page,
                page: None,
            },
            Gate::default()
        ));
        let tokens = Tokens::DARK;
        // Two passes: an `Area`'s first frame is its sizing pass.
        let mut output = None;
        for pass in 0..2 {
            let mut drawn = ctx.run_ui(
                RawInput {
                    screen_rect: Some(window),
                    ..Default::default()
                },
                |ui| {
                    let _ = show(
                        ui,
                        Frame {
                            menu: &mut menu,
                            free: true,
                            tokens: &tokens,
                        },
                    );
                },
            );
            drawn.textures_delta.clear();
            if pass == 1 {
                output = Some(drawn);
            }
        }
        let output = output.expect("two passes");
        let card = card(&output);
        assert!(
            card.left() >= window.left() - 0.01
                && card.right() <= window.right() + 0.01
                && card.top() >= window.top() - 0.01
                && card.bottom() <= window.bottom() + 0.01,
            "the menu at {corner:?} left the window: {card:?}"
        );
    }
}

#[test]
fn the_card_swallows_clicks_and_is_the_one_focus_stop() {
    let ctx = context();
    let mut menu = opened(Device::Pointer);
    warm(&ctx, &mut menu);
    let (_, (surface, rows)) = draw(&ctx, &mut menu, true, Vec::new(), |ui, area| {
        let surface = ui
            .ctx()
            .read_response(area.with("surface"))
            .expect("the card is a widget");
        let rows: Vec<egui::Response> = (0..4)
            .map(|index| {
                ui.ctx()
                    .read_response(area.with(("row", index)))
                    .expect("a row is a widget")
            })
            .collect();
        (surface, rows)
    });

    assert!(surface.sense.senses_click(), "the card takes the click");
    assert!(surface.sense.is_focusable(), "the card is the focus stop");
    for row in &rows {
        assert!(!row.sense.is_focusable(), "a row joined the tab order");
    }
}

#[test]
fn a_closed_menu_draws_nothing() {
    let ctx = context();
    let mut menu = Menu::default();
    let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
    assert_eq!(drawn.closed, None);
    assert!(drawn.output.shapes.is_empty(), "a closed menu painted");
}
