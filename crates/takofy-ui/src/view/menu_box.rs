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
mod tests;
