//! What a shelf the page cannot draw yet, or could not draw at all, says instead.
//!
//! A waiting shelf holds the room its content will take, so nothing below it
//! moves when the content lands. A failed one says why and offers the control
//! that asks again: one shelf failing must not take the page with it, and a
//! failure the user can act on must never be drawn silently.

use egui::{Sense, Ui};

use crate::home::Slot;
use crate::home::episodes::DRAWN;
use crate::message::Failure;
use crate::theme::Tokens;
use crate::view::playlist::root;
use crate::view::skeleton;

use super::super::{section, text as recipe};
use super::Pressed;

/// The shape a waiting shelf's placeholder band takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Band {
    /// One row of cards, as the card shelves draw.
    Cards,
    /// Four episode rows, as the episodes shelf draws.
    Episodes,
}

/// A waiting shelf: its title, then a band of the height its content will take.
pub(super) fn waiting(ui: &mut Ui, title: &str, tokens: &Tokens, band: Band) {
    section::head(ui, title, "", tokens);
    match band {
        Band::Cards => card_band(ui, tokens),
        Band::Episodes => skeleton::episode_rows(ui, DRAWN, tokens),
    }
}

/// One row of card placeholders: the shape the card shelves draw.
pub(super) fn card_band(ui: &mut Ui, tokens: &Tokens) {
    let solved = root::media_grid(ui);
    let count = solved.columns;
    let list = root::grid_rect(ui, &solved, count);
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
