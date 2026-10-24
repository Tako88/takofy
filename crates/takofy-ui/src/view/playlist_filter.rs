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

use egui::{Rect, Ui};
use takofy_model::TrackEntry;

use crate::theme::Tokens;
use crate::track_filter::{self, QUERY_MAX, TrackFilter};

use super::filter_field;

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
