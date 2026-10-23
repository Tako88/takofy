//! One menu's box: the raised card every menu in the app is drawn in, and the
//! two lit tiers its rows answer to.
//!
//! The sheet makes this shared rather than copied: a field's results and a row's
//! commands are the same object — one `border_strong` hairline, `radius::MD`, the
//! app's largest shadow — and a change to any of it lands on both at once. What
//! is *not* shared is the row grammar: a result row has a cover and two lines, a
//! command row has a label and a trailing slot. Each view draws its own.
//!
//! Paint order is part of the contract: the shadow before the clip (a clipped
//! shadow is no shadow), then the surface's interaction, then the fill, then the
//! rows, then the hairline last so no row's wash paints over it.

use egui::{CornerRadius, Id, Rect, Response, Sense, Stroke, Ui, vec2};

use crate::menu::Tier;
use crate::theme::{Tokens, radius, space, stroke};

/// What a card needs to draw itself.
pub struct Card<'a> {
    /// The box, in screen points.
    pub rect: Rect,
    /// The surface's widget id. It is the one hit-opaque surface under the rows,
    /// so a click on the card's padding is the card's and never the page's.
    pub id: Id,
    /// How the surface senses: a menu that holds the keyboard passes
    /// `Sense::click()`, one whose keyboard lives elsewhere passes
    /// `Sense::CLICK` and stays out of the tab order.
    pub sense: Sense,
    pub tokens: &'a Tokens,
}

/// Draws the box, the rows inside it, and the hairline over them.
///
/// The rows are a closure because only the caller knows its grammar, and they
/// are drawn between the fill and the hairline so a row's wash never covers the
/// menu's own edge.
pub fn card<R>(ui: &mut Ui, card: Card<'_>, rows: impl FnOnce(&mut Ui) -> R) -> (Response, R) {
    // Before the clip: a clipped shadow is no shadow.
    ui.painter().add(
        card.tokens
            .palette
            .shadow_lg
            .as_shape(card.rect, radius::MD),
    );
    ui.set_clip_rect(card.rect);

    // One widget under the rows: a click anywhere on the card is the card's,
    // never the track row or the hero button underneath. The rows interact after
    // this, so they win on their own rects.
    let response = ui.interact(card.rect, card.id, card.sense);
    ui.painter()
        .rect_filled(card.rect, radius::MD, card.tokens.palette.surface_raised);

    let inner = rows(ui);

    ui.painter().rect_stroke(
        card.rect,
        radius::MD,
        Stroke::new(stroke::HAIRLINE, card.tokens.palette.border_strong),
        egui::StrokeKind::Inside,
    );
    (response, inner)
}

/// The wash one row takes, and the pointer's tick when the pointer owns it.
///
/// The keyboard's tier is the app-wide selected-row wash; the pointer's is the
/// row hover plus a `space::S4` tick at the margin. Never both: the tier is one
/// value, so the two can never be drawn over each other.
pub fn paint_highlight(ui: &Ui, tokens: &Tokens, row: Rect, corners: CornerRadius, tier: Tier) {
    match tier {
        Tier::Keyboard => {
            ui.painter()
                .rect_filled(row, corners, tokens.palette.row_selected);
        }
        Tier::Pointer => {
            ui.painter()
                .rect_filled(row, corners, tokens.palette.row_hover);
            let tick = Rect::from_center_size(
                egui::pos2(row.left() + stroke::RULE / 2.0, row.center().y),
                vec2(stroke::RULE, space::S4),
            );
            ui.painter()
                .rect_filled(tick, radius::NONE, tokens.palette.text_muted);
        }
        Tier::None => {}
    }
}

/// The 2px rule at the row's margin, rounded with the menu so the first and the
/// last row's marker are not cut by its corners.
pub fn paint_marker(ui: &Ui, row: Rect, corners: CornerRadius, color: egui::Color32) {
    let marker = Rect::from_min_size(row.min, vec2(stroke::RULE, row.height()));
    ui.painter().rect_filled(marker, corners, color);
}

/// The corners a row's wash and marker may use, so the menu's `radius::MD`
/// corners are not painted over by a square row.
pub fn row_corners(first: bool, last: bool) -> CornerRadius {
    CornerRadius {
        nw: if first { radius::MD.nw } else { 0 },
        ne: if first { radius::MD.ne } else { 0 },
        sw: if last { radius::MD.sw } else { 0 },
        se: if last { radius::MD.se } else { 0 },
    }
}

/// Whether a click both pressed and released on this row.
///
/// egui reports a click against the widget the press landed on, even when the
/// release is over another one. A menu's row grammar is the browser's: a press
/// on one row and a release on another is no click at all, so the release's own
/// position decides.
pub fn clicked_on(ui: &Ui, hit: &Response, row: Rect) -> bool {
    hit.clicked()
        && ui
            .input(|input| input.pointer.interact_pos())
            .is_some_and(|pos| row.contains(pos))
}

#[cfg(test)]
mod tests {
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
}
