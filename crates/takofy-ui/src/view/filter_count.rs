//! The count a list filter paints and announces.
//!
//! The playlist's action row and the discography's chip row show the same
//! reference readout: a mono, upper-cased `n of m` line while a query is on,
//! and nothing at rest. The painter and the polite status node live here so the
//! two rows cannot drift apart about either.

use std::sync::Arc;

use egui::{FontId, Id, Pos2, Rect, Ui};

use crate::theme::{MonoWeight, Tokens, fonts, text, tracking};

use super::text as recipe;

/// Lays the count out, so a row can reserve its width before painting it.
pub(super) fn layout(ui: &Ui, label: &str, tokens: &Tokens) -> Arc<egui::Galley> {
    recipe::spaced(
        ui,
        &label.to_uppercase(),
        FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
        tokens.palette.text_subtle,
        recipe::tracking_pts(text::XXS, tracking::CAPS),
    )
}

/// Paints the count in `rect`, right-aligned on the row's own centre line.
pub(super) fn paint(ui: &Ui, rect: Rect, label: Arc<egui::Galley>, tokens: &Tokens) {
    ui.painter().galley(
        Pos2::new(
            rect.right() - label.size().x,
            rect.center().y - label.size().y / 2.0,
        ),
        label,
        tokens.palette.text_subtle,
    );
}

/// Announces the count in the field's own words.
///
/// egui has no live region, so this is the smallest honest mechanism: a
/// `Role::Status` node the platform adapters treat as a polite live region. The
/// key belongs to the list, so the playlist's node and the discography's are
/// never confused for one another.
pub(super) fn announce(ui: &Ui, id: Id, label: &str) {
    let _ = ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(egui::Role::Status);
        node.set_live(egui::accesskit::Live::Polite);
        node.set_label(label);
    });
}
