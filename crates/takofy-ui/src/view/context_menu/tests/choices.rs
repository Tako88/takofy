use super::super::*;
use super::test_support::*;
use crate::layout::search_menu::row_rect;
use crate::menu::{Anchor, Device, Door, Gate, Listing, Menu, Request, Verb};
use crate::theme::{fonts, style};
use crate::view::test_support::{button, click_events};
use egui::{Event, Key, Modifiers, Pos2, RawInput, Rect, pos2, vec2};

#[test]
fn an_outside_click_closes_and_returns_the_opener() {
    let ctx = context();
    let mut menu = opened(Device::Pointer);
    warm(&ctx, &mut menu);
    let away = pos2(20.0, 560.0);
    frame(&ctx, &mut menu, true, vec![Event::PointerMoved(away)]);
    let mut closed = None;
    let mut focused = false;
    for event in click_events(away) {
        let (drawn, has) = draw(&ctx, &mut menu, true, vec![event], |ui, _| {
            ui.memory(|memory| memory.has_focus(opener()))
        });
        closed = drawn.closed;
        if closed.is_some() {
            focused = has;
        }
    }
    assert_eq!(
        closed,
        Some(Closed {
            opener: Some(opener()),
            chose: None,
        })
    );
    assert!(focused, "the outside click did not return the keyboard");
}

#[test]
fn choosing_a_row_returns_the_opener() {
    let ctx = context();
    let mut menu = opened(Device::Pointer);
    warm(&ctx, &mut menu);
    let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
    let card = card(&drawn.output);
    let row = row_rect(card, 0);
    frame(
        &ctx,
        &mut menu,
        true,
        vec![Event::PointerMoved(row.center())],
    );
    let mut closed = None;
    let mut focused = false;
    for event in click_events(row.center()) {
        let (drawn, has) = draw(&ctx, &mut menu, true, vec![event], |ui, _| {
            ui.memory(|memory| memory.has_focus(opener()))
        });
        closed = drawn.closed;
        if closed.is_some() {
            focused = has;
        }
    }
    assert_eq!(
        closed.map(|closed| (closed.opener, closed.chose)),
        Some((Some(opener()), Some(Verb::AddToQueue)))
    );
    assert!(focused, "the choice did not return the keyboard");
}

#[test]
fn a_press_on_one_row_and_a_release_on_another_chooses_nothing() {
    let ctx = context();
    let mut menu = opened(Device::Pointer);
    warm(&ctx, &mut menu);
    let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
    let card = card(&drawn.output);
    let from = row_rect(card, 0).center();
    let to = row_rect(card, 1).center();

    let mut closed = None;
    for event in [
        Event::PointerMoved(from),
        button(from, true),
        Event::PointerMoved(to),
        button(to, false),
    ] {
        closed = frame(&ctx, &mut menu, true, vec![event]).closed;
    }
    assert_eq!(closed, None, "a press on one row chose the one released on");
    assert!(menu.is_open());
}

#[test]
fn an_outside_click_closes_but_not_on_the_frame_a_request_opened_it() {
    let ctx = context();
    // The first open sizes the area; the second is the one a door opens, and
    // its release is the click that opened it.
    let mut menu = opened(Device::Pointer);
    warm(&ctx, &mut menu);
    let _ = menu.close(Reason::Escape);

    let mut menu = opened(Device::Pointer);
    let away = pos2(20.0, 560.0);
    let drawn = frame(
        &ctx,
        &mut menu,
        true,
        vec![Event::PointerMoved(away), button(away, false)],
    );
    assert_eq!(drawn.closed, None, "the opening click dismissed the menu");
    assert!(menu.is_open());

    let mut closed = None;
    for event in click_events(away) {
        closed = frame(&ctx, &mut menu, true, vec![event]).closed;
    }
    assert!(closed.is_some(), "an outside click did not close the menu");
    assert!(!menu.is_open());
}

#[test]
fn a_choice_is_not_offered_while_the_frame_has_no_command_to_spare() {
    let ctx = context();
    let mut menu = opened(Device::Keyboard);
    warm(&ctx, &mut menu);
    press(&ctx, &mut menu, Key::ArrowDown);
    let drawn = frame(
        &ctx,
        &mut menu,
        false,
        vec![key_event(Key::Enter, Modifiers::NONE)],
    );
    assert_eq!(drawn.closed, None, "a choice was offered and dropped");
    assert!(menu.is_open(), "a dropped choice closed the menu");
}

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
