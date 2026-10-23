//! The playlist's filter field, as the action row draws it.
//!
//! The field is drawn before the rows are chosen, so a keystroke, a clear click
//! or an Escape is already resolved when the list asks which rows to show. The
//! query itself lives in [`TrackFilter`], which keeps it across frames and drops
//! it when the page changes.
//!
//! What is left here is the field's participation in the page: where it goes,
//! how wide it is and whether it is drawn at all belong to the action row, and
//! the tiles beside it to the row's own solver.

use std::sync::Arc;

use egui::{FontId, Pos2, Rect, Ui};
use takofy_model::TrackEntry;

use crate::format::filter_count_label;
use crate::theme::{MonoWeight, Tokens, fonts, text, tracking};
use crate::track_filter::{self, QUERY_MAX, TrackFilter};

use super::filter_field;
use super::text as recipe;

/// What the page hands the row's field for one frame.
pub(super) struct Filter<'a> {
    /// The query, kept across frames.
    pub(super) state: &'a mut TrackFilter,
    /// The playlist the query belongs to.
    pub(super) playlist: &'a str,
    /// The rows the query reads.
    pub(super) entries: &'a [TrackEntry],
}

/// Draws the field into `field` and answers which true rows the query leaves.
///
/// `None` is the unfiltered list: no query at all, or one of nothing but spaces.
/// A caller that did not draw the field must treat the list as whole, because a
/// query nobody can see must not narrow it.
pub(super) fn show(
    ui: &mut Ui,
    filter: &mut Filter<'_>,
    field: Rect,
    rule: filter_field::Rule,
    tokens: &Tokens,
) -> Option<Vec<usize>> {
    filter_field::show(
        ui,
        filter_field::Field {
            id: ui.scope_id().with("playlist-filter"),
            rect: field,
            text: filter.state.edit(filter.playlist),
            limit: QUERY_MAX,
            placeholder: "FILTER",
            label: "Filter this playlist",
            rule,
        },
        tokens,
    );
    let needle = filter.state.needle(filter.playlist)?;
    Some(track_filter::hits(filter.entries, &needle))
}

/// The count's label, laid out so the row can reserve its width before painting.
///
/// The row places the field first and the count after, so it has to know how
/// wide the count will be before it can say whether it fits. Measuring is that
/// answer; the same galley is painted, so the box cannot lie about the words.
pub(super) fn count_label(
    ui: &Ui,
    shown: usize,
    total: usize,
    tokens: &Tokens,
) -> Arc<egui::Galley> {
    recipe::spaced(
        ui,
        &filter_count_label(shown, total).to_uppercase(),
        FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
        tokens.palette.text_subtle,
        recipe::tracking_pts(text::XXS, tracking::CAPS),
    )
}

/// Paints the count in `rect`, on the row's own centre line.
pub(super) fn paint_count(ui: &Ui, rect: Rect, label: Arc<egui::Galley>, tokens: &Tokens) {
    ui.painter().galley(
        Pos2::new(
            rect.right() - label.size().x,
            rect.center().y - label.size().y / 2.0,
        ),
        label,
        tokens.palette.text_subtle,
    );
}

/// Announces how many rows the query leaves.
///
/// egui has no live region, so this is the smallest honest mechanism: a
/// `Role::Status` node the platform adapters treat as a polite live region,
/// carrying the count in the field's own words rather than the painted
/// upper-cased glyphs. It is spoken when the name changes and only while a query
/// narrows the list: with no query there is no count to say.
pub(super) fn announce(ui: &Ui, shown: usize, total: usize) {
    let label = filter_count_label(shown, total);
    let _ = ui
        .ctx()
        .accesskit_node_builder(ui.scope_id().with("track-count"), |node| {
            node.set_role(egui::Role::Status);
            node.set_live(egui::accesskit::Live::Polite);
            node.set_label(label.as_str());
        });
}
