//! Home's foot: the page saying how it was assembled.
//!
//! There is no feed behind this client, so the one thing the page can always
//! promise is its own arithmetic — and the note is the whole of it, because the
//! counts a shelf would print are exactly what this page refuses to print.

use egui::{Sense, Ui, vec2};

use crate::format::HOME_COLOPHON;
use crate::theme::{Tokens, space, stroke};

use super::super::text as recipe;

/// Draws the rule, the note under it, and the air the page ends in.
pub(super) fn show(ui: &mut Ui, tokens: &Tokens) {
    let width = ui.available_width();
    let (rule, _) = ui.allocate_exact_size(vec2(width, stroke::HAIRLINE), Sense::hover());
    ui.painter().rect_filled(rule, 0.0, tokens.palette.border);
    ui.add_space(space::S4);

    // The note wraps: a foot is prose, and a page too narrow for one line draws
    // two rather than an elision nobody can finish reading.
    ui.add(
        egui::Label::new(
            egui::RichText::new(HOME_COLOPHON)
                .font(recipe::colophon())
                .color(tokens.palette.text_muted),
        )
        .wrap(),
    );
    ui.add_space(space::S5);
}

#[cfg(test)]
mod tests;
