use egui::{Event, Pos2, RawInput, Rect, Shape, pos2, vec2};

use super::*;
use crate::menu::Tier;
use crate::theme::{fonts, style};
use crate::view::test_support::button;

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
        events,
        ..Default::default()
    }
}

fn card_rect() -> Rect {
    Rect::from_min_size(pos2(100.0, 80.0), vec2(320.0, 176.0))
}

fn row_of(index: usize) -> Rect {
    crate::layout::search_menu::row_rect(card_rect(), index)
}

/// The fills one menu row's highlight paints.
fn highlight_fills(tier: Tier) -> Vec<egui::Color32> {
    let ctx = context();
    let tokens = Tokens::DARK;
    let row = row_of(0);
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        paint_highlight(ui, &tokens, row, radius::MD, tier);
    });
    output.textures_delta.clear();
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(shape) => Some(shape.fill),
            _ => None,
        })
        .collect()
}

#[test]
fn a_keyboard_row_takes_the_selection_fill_and_a_pointer_row_the_row_hover() {
    let tokens = Tokens::DARK;
    assert_eq!(
        highlight_fills(Tier::Keyboard),
        vec![tokens.palette.row_selected],
        "the keyboard's row painted more than the selection"
    );

    let pointer = highlight_fills(Tier::Pointer);
    assert_eq!(
        pointer.first(),
        Some(&tokens.palette.row_hover),
        "the pointer's row washed with something other than the row hover"
    );
    assert!(
        pointer.contains(&tokens.palette.text_muted),
        "the pointer's tick went with the wash: {pointer:?}"
    );
    assert_eq!(pointer.len(), 2, "the pointer's tier is a wash and a tick");

    assert!(
        highlight_fills(Tier::None).is_empty(),
        "a row with no tier painted"
    );
}

#[test]
fn only_the_first_and_last_rows_round_their_corners() {
    assert_eq!(
        row_corners(true, false),
        CornerRadius {
            nw: radius::MD.nw,
            ne: radius::MD.ne,
            sw: 0,
            se: 0,
        }
    );
    assert_eq!(
        row_corners(false, true),
        CornerRadius {
            nw: 0,
            ne: 0,
            sw: radius::MD.sw,
            se: radius::MD.se,
        }
    );
    assert_eq!(row_corners(false, false), radius::NONE);
}

#[test]
fn a_click_is_the_press_and_the_release_on_the_same_row() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let row = row_of(0);
    let inside = row.center();
    let outside = pos2(row.right() + 40.0, row.center().y);

    // Warm up, so the hit test has a previous pass to read.
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let _ = card(
            ui,
            Card {
                rect: card_rect(),
                id: Id::unique("card"),
                sense: Sense::CLICK,
                tokens: &tokens,
            },
            |ui| {
                ui.interact(row, Id::unique("row"), Sense::CLICK);
            },
        );
    });
    output.textures_delta.clear();

    let answer = |events: Vec<Event>| {
        let mut clicked = None;
        let mut output = ctx.run_ui(input(events), |ui| {
            let (_, hit) = card(
                ui,
                Card {
                    rect: card_rect(),
                    id: Id::unique("card"),
                    sense: Sense::CLICK,
                    tokens: &tokens,
                },
                |ui| ui.interact(row, Id::unique("row"), Sense::CLICK),
            );
            clicked = Some(clicked_on(ui, &hit, row));
        });
        output.textures_delta.clear();
        clicked.expect("a frame ran")
    };

    let down_then_up = |from: Pos2, to: Pos2| {
        answer(vec![Event::PointerMoved(from), button(from, true)]);
        answer(vec![Event::PointerMoved(to), button(to, false)])
    };

    // The card's surface is under the row, so the press lands on the row and
    // the release on the same row is a click.
    assert!(down_then_up(inside, inside), "a click on the row was lost");
    assert!(
        !down_then_up(inside, outside),
        "a release off the row still counted as a click"
    );
}

#[test]
fn the_card_paints_shadow_then_fill_then_hairline_last() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let _ = card(
            ui,
            Card {
                rect: card_rect(),
                id: Id::unique("card"),
                sense: Sense::CLICK,
                tokens: &tokens,
            },
            |ui| {
                ui.interact(row_of(0), Id::unique("row"), Sense::CLICK);
            },
        );
    });
    output.textures_delta.clear();

    let rects: Vec<&egui::epaint::RectShape> = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(shape) => Some(shape),
            _ => None,
        })
        .collect();

    let shadow = rects
        .iter()
        .position(|shape| shape.blur_width > 0.0)
        .expect("the card has a shadow");
    let fill = rects
        .iter()
        .position(|shape| shape.fill == tokens.palette.surface_raised)
        .expect("the card has a fill");
    let hairline = rects
        .iter()
        .position(|shape| shape.stroke.color == tokens.palette.border_strong)
        .expect("the card has a hairline");

    assert!(shadow < fill, "the fill is painted under the shadow");
    assert!(fill < hairline, "the hairline is painted under the fill");
    assert_eq!(
        hairline,
        rects.len() - 1,
        "the hairline is the last thing the card paints"
    );
    assert_eq!(rects[hairline].fill.a(), 0, "the hairline is not a fill");
}
