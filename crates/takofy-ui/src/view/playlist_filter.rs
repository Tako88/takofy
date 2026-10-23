//! The playlist page's tools row: the filter field and its count.
//!
//! The field is drawn before the rows are chosen, so a keystroke, a clear click
//! or an Escape is already resolved when the list asks which rows to show. The
//! query itself lives in [`TrackFilter`], which keeps it across frames and drops
//! it when the page changes.
//!
//! The row pins to the top of the list's own viewport once the flow has scrolled
//! it off: [`row_rect`] still takes the row's space, and the row is drawn and
//! interacted at the clip top instead, so the list's geometry is unchanged and
//! there is never a second field.

use egui::{FontId, Pos2, Rect, Ui, vec2};
use takofy_model::TrackEntry;

use crate::format::filter_count_label;
use crate::theme::{MonoWeight, Tokens, extent, fonts, space, stroke, text, tracking};
use crate::track_filter::{self, QUERY_MAX, TrackFilter};

use super::filter_field;
use super::text as recipe;

/// The widest the field grows on a row with room to spare.
const FIELD_MAX: f32 = 240.0;

/// The narrowest the field is drawn. A row with less room than this plus the
/// count keeps the field at this size and drops the count.
const FIELD_MIN: f32 = 120.0;

/// The row the field and the count sit in.
///
/// The tests solve a row's place through this too, so the tools row and the
/// list under it cannot drift apart.
pub(super) fn row_rect(ui: &mut Ui) -> Rect {
    let row = ui
        .allocate_space(vec2(ui.available_width(), extent::BUTTON))
        .1;
    ui.add_space(space::S2);
    row
}

/// What one frame of the tools row leaves the arm under it.
pub(super) struct Shown {
    /// `None` when the row is unfiltered; `Some` carries the ascending true
    /// indices of the surviving rows, empty when nothing matches.
    pub(super) hits: Option<Vec<usize>>,
    /// The pinned row's box, when the row left its place in the flow and was
    /// drawn at the top of the list's own viewport this frame. `None` when it
    /// did not pin, so the arm's clip is untouched.
    pub(super) occupies: Option<Rect>,
}

/// Draws the tools row and answers which true rows the query leaves.
pub(super) fn show(
    ui: &mut Ui,
    filter: &mut TrackFilter,
    playlist: &str,
    entries: &[TrackEntry],
    tokens: &Tokens,
) -> Shown {
    let flow = row_rect(ui);
    if flow.width() <= 0.0 {
        return Shown {
            hits: filter
                .needle(playlist)
                .map(|needle| track_filter::hits(entries, &needle)),
            occupies: None,
        };
    }

    // The row pins to the top of the list's own viewport once the flow has
    // scrolled above it. Nothing else moves: the flow keeps the row's space, so
    // the list's geometry, the scrollbar and its culling are unchanged. It is
    // the same row, drawn and interacted at the clip top instead, so there is
    // never a second field, a second id or a second focus.
    let clip_top = ui.clip_rect().top();
    let pinned = flow.top() < clip_top;
    let row = if pinned {
        Rect::from_min_size(Pos2::new(flow.left(), clip_top), flow.size())
    } else {
        flow
    };

    // The rows and the band above this one slide under a pinned row, and
    // everything drawn before it stays on the canvas: what the field and the
    // count leave empty would show the page behind instead of the row. The
    // row's own surface is painted first, the row's full width and full
    // height, so the row's contents land on it rather than beside it.
    if pinned {
        ui.painter().rect_filled(row, 0.0, tokens.palette.surface);
        // The row is holding the list under it, so it wears an edge to say so:
        // a full-width hairline at its bottom, the row's edge and not the
        // field's underline, painted before the field keeps that underline.
        ui.painter().rect_filled(
            Rect::from_min_size(
                Pos2::new(row.left(), row.bottom() - stroke::HAIRLINE),
                vec2(row.width(), stroke::HAIRLINE),
            ),
            0.0,
            tokens.palette.border,
        );
    }

    // The count sits left of the field, so its box comes out of the row before
    // the field is sized: a 240-wide field on a narrow row would push the count
    // into the gutter. The widest label this list can print reserves it, so a
    // query that starts or grows on this frame cannot outgrow the gap.
    let widest = recipe::spaced(
        ui,
        &filter_count_label(entries.len(), entries.len()).to_uppercase(),
        FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
        tokens.palette.text_subtle,
        recipe::tracking_pts(text::XXS, tracking::CAPS),
    );
    let room = row.width() - space::S4 - widest.size().x;
    let width = room.clamp(FIELD_MIN, FIELD_MAX).min(row.width());
    // A field that had to take more than its share leaves no box for the count:
    // it would be painted outside the row, or over the field.
    let count_fits = room >= width;
    let field = Rect::from_min_size(
        Pos2::new(row.right() - width, row.top()),
        vec2(width, row.height()),
    );
    let id = ui.scope_id().with("playlist-filter");
    filter_field::show(
        ui,
        filter_field::Field {
            id,
            rect: field,
            text: filter.edit(playlist),
            limit: QUERY_MAX,
            placeholder: "FILTER",
            label: "Filter this playlist",
        },
        tokens,
    );

    let hits = filter
        .needle(playlist)
        .map(|needle| track_filter::hits(entries, &needle));
    if count_fits && let Some(hits) = &hits {
        let label = recipe::spaced(
            ui,
            &filter_count_label(hits.len(), entries.len()).to_uppercase(),
            FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
            tokens.palette.text_subtle,
            recipe::tracking_pts(text::XXS, tracking::CAPS),
        );
        ui.painter().galley(
            Pos2::new(
                field.left() - space::S4 - label.size().x,
                field.center().y - label.size().y / 2.0,
            ),
            label,
            tokens.palette.text_subtle,
        );
    }
    Shown {
        hits,
        occupies: pinned.then_some(row),
    }
}
