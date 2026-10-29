//! What a shelf the page could not draw says instead.
//!
//! The words and the control that asks again: one shelf failing must not take
//! the page with it, and a failure the user can act on must never be drawn
//! silently.

use egui::Ui;

use crate::home::Slot;
use crate::message::Failure;
use crate::theme::Tokens;

use super::super::{section, text as recipe};
use super::Pressed;

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
