//! What a shelf the page cannot draw yet, or could not draw at all, says instead.
//!
//! A waiting shelf holds the room its content will take, so nothing below it
//! moves when the content lands. A failed one says why and offers the control
//! that asks again: one shelf failing must not take the page with it, and a
//! failure the user can act on must never be drawn silently.

use egui::{Rect, Sense, Ui, pos2, vec2};

use crate::home::Slot;
use crate::home::episodes::DRAWN;
use crate::message::Failure;
use crate::theme::{Tokens, extent, radius, space};
use crate::view::skeleton;

use super::super::{section, text as recipe};
use super::Pressed;

/// The shape a waiting shelf's placeholder band takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Band {
    /// One row of cards, as the because shelf draws.
    Cards,
    /// The two-column band of stations, at the height its cells take.
    Stations,
    /// The counted shelf's compact tiles.
    Tiles,
    /// Four episode rows, as the episodes shelf draws.
    Episodes,
}

/// A waiting shelf: its title, then a band of the height its content will take.
pub(super) fn waiting(ui: &mut Ui, title: &str, tokens: &Tokens, band: Band) {
    section::head(ui, title, "", tokens);
    match band {
        Band::Cards => card_band(ui, tokens),
        Band::Stations => skeleton::compact_rows(
            ui,
            crate::home::stations::STATIONS,
            super::stations::COLUMNS,
            super::stations::cell(ui, tokens),
            space::GUTTER,
            tokens,
        ),
        Band::Tiles => {
            let width = ui.available_width();
            skeleton::compact_rows(
                ui,
                crate::home::counted::DRAWN,
                super::counted::columns(width),
                extent::HOME_ROW_H,
                space::S2,
                tokens,
            )
        }
        Band::Episodes => skeleton::episode_rows(ui, DRAWN, tokens),
    }
    // A shelf that has content has a door at its foot, so the waiting band holds
    // that row too: it is the difference between a page that settles and one
    // where the last shelf jumps by one row when the read lands.
    door_band(ui, tokens);
}

/// The room a shelf's foot door will take, marked but not drawn.
pub(super) fn door_band(ui: &mut Ui, tokens: &Tokens) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, extent::DROP_HIT), Sense::hover());
    let bar = Rect::from_min_size(
        pos2(
            rect.left() + space::S1,
            rect.center().y - extent::SKELETON_LINE_SM / 2.0,
        ),
        vec2(width * 0.22, extent::SKELETON_LINE_SM),
    );
    ui.painter()
        .rect_filled(bar, radius::XS, tokens.palette.skeleton_rule);
}

/// One row of card placeholders, and the foot the shelf will end in.
pub(super) fn card_band(ui: &mut Ui, tokens: &Tokens) {
    let solved = super::grid::solve(ui);
    let count = solved.columns;
    let list = super::grid::row_rect(ui, &solved, count);
    ui.allocate_rect(list, Sense::hover());
    skeleton::tiles(ui, &solved, count, tokens);
}

/// A failed shelf: its title, why it failed, and the Retry that asks again.
pub(super) fn failed(
    ui: &mut Ui,
    tokens: &Tokens,
    title: &str,
    failure: Failure,
    slot: Slot,
    out: &mut Pressed,
) {
    section::head(ui, title, "", tokens);
    ui.label(recipe::muted(
        crate::signin::failure_message(failure),
        tokens,
    ));
    if ui.button("Retry").clicked() {
        out.retry = Some(slot);
    }
}

#[cfg(test)]
mod tests;
