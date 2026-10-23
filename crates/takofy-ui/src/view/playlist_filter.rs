//! The playlist page's tools row: the filter field and its count.
//!
//! The field is drawn before the rows are chosen, so a keystroke, a clear click
//! or an Escape is already resolved when the list asks which rows to show. The
//! query itself lives in [`TrackFilter`], which keeps it across frames and drops
//! it when the page changes.

use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, Ui, WidgetInfo, vec2};
use takofy_model::TrackEntry;

use crate::format::filter_count_label;
use crate::icon::{self, Icon};
use crate::theme::icon::Size;
use crate::theme::{
    MonoWeight, Tokens, Weight, extent, fonts, radius, space, stroke, text, tracking,
};
use crate::track_filter::{self, QUERY_MAX, TrackFilter};

use super::name_field;
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

/// Draws the tools row and answers which true rows the query leaves.
///
/// `None` means the row is unfiltered; `Some` carries the ascending true indices
/// of the surviving rows, including an empty list when nothing matches.
pub(super) fn show(
    ui: &mut Ui,
    filter: &mut TrackFilter,
    playlist: &str,
    entries: &[TrackEntry],
    tokens: &Tokens,
) -> Option<Vec<usize>> {
    let row = row_rect(ui);
    if row.width() <= 0.0 {
        return filter
            .needle(playlist)
            .map(|needle| track_filter::hits(entries, &needle));
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

    let glyph = Rect::from_min_size(
        Pos2::new(
            field.left() + space::S1,
            field.center().y - Size::SM.side / 2.0,
        ),
        vec2(Size::SM.side, Size::SM.side),
    );
    icon::paint(
        ui.painter(),
        Icon::Search,
        glyph,
        Size::SM,
        tokens.palette.text_subtle,
    );

    {
        let text = filter.edit(playlist);
        // The clear control's box: the field's right padding and the 12px glyph.
        let clear = Rect::from_min_max(
            Pos2::new(
                field.right() - space::S3 - Size::SM.side - space::S1,
                field.top(),
            ),
            Pos2::new(field.right() - space::S1, field.bottom()),
        );
        // The click is read before the edit, because the edit takes the box back
        // the moment the text empties. The box is laid out every frame and it is
        // not a tab stop: while the text is empty the edit covers it and wins the
        // pointer, so a click there still focuses the field.
        let hit = ui.interact(clear, id.with("clear"), Sense::CLICK);
        let clear_clicked = !text.is_empty() && hit.clicked();
        if clear_clicked {
            text.clear();
        }

        // The clear control keeps its own box whenever the field can be typed
        // into, so a character typed on this frame never runs under the glyph.
        let pressed = ui.input(|input| {
            input.pointer.primary_pressed()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|at| field.contains(at))
        });
        let typable = !text.is_empty() || pressed || ui.memory(|memory| memory.has_focus(id));
        let edit_rect = if typable {
            Rect::from_min_max(
                field.min,
                Pos2::new(clear.left() - space::S2, field.bottom()),
            )
        } else {
            field
        };
        let out = name_field::edit(
            ui,
            name_field::Spec {
                id,
                rect: edit_rect,
                text,
                writing: false,
                wants_focus: clear_clicked || pressed,
                font: FontId::new(text::XS, fonts::sans(Weight::Regular)),
                hint: "",
                inset: egui::Margin {
                    left: (Size::SM.side + space::S2) as i8,
                    right: (Size::SM.side + space::S2) as i8,
                    top: 0,
                    bottom: 0,
                },
                text_color: tokens.palette.text,
                weak_color: tokens.palette.text_subtle,
                cursor: Stroke::new(stroke::HAIRLINE, tokens.palette.text),
                align: Align2::LEFT_CENTER,
                limit: QUERY_MAX,
            },
        );
        ui.ctx().accesskit_node_builder(id, |node| {
            node.set_role(egui::Role::SearchInput);
            node.set_label("Filter this playlist");
        });
        if clear_clicked {
            // The click lands on the clear control, not the edit, so the edit's
            // own request is undone by the pointer. Take the keyboard back with
            // the filter the field's first Escape and arrows need.
            ui.memory_mut(|memory| {
                memory.request_focus(id);
                memory.set_focus_lock_filter(id, super::keys::FIELD_FILTER);
            });
        }
        if out.escape {
            text.clear();
            ui.memory_mut(|memory| memory.surrender_focus(id));
        }

        // The chrome is decided from the text the frame ends with, so no frame
        // paints a placeholder over the character just typed, or a clear control
        // over the field it has just emptied.
        if text.is_empty() {
            let placeholder = recipe::spaced(
                ui,
                "FILTER",
                FontId::new(text::XXS, fonts::mono(MonoWeight::Regular)),
                tokens.palette.text_subtle,
                recipe::tracking_pts(text::XXS, tracking::CAPS),
            );
            ui.painter().galley(
                Pos2::new(
                    field.left() + Size::SM.side + space::S2,
                    field.center().y - placeholder.size().y / 2.0,
                ),
                placeholder,
                tokens.palette.text_subtle,
            );
        } else {
            if hit.hovered() {
                ui.painter()
                    .rect_filled(clear, radius::XS, tokens.palette.surface_hover);
            }
            let ink = if hit.hovered() {
                tokens.palette.text
            } else {
                tokens.palette.text_subtle
            };
            let mark = Rect::from_center_size(clear.center(), vec2(Size::SM.side, Size::SM.side));
            icon::paint(ui.painter(), Icon::Close, mark, Size::SM, ink);
            hit.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, "Clear filter"));
        }
    }

    // Border only, and the bottom hairline is the whole border: no fill, no
    // glow, and no width change. The colour is the focus the frame ends with, so
    // a clear click that took the keyboard, and an Escape that gave it back,
    // both show on their own frame.
    let focused = ui.memory(|memory| memory.has_focus(id));
    let border = if focused {
        tokens.palette.text
    } else {
        tokens.palette.border
    };
    ui.painter().rect_filled(
        Rect::from_min_size(
            Pos2::new(field.left(), field.bottom() - stroke::HAIRLINE),
            vec2(field.width(), stroke::HAIRLINE),
        ),
        0.0,
        border,
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
    hits
}
