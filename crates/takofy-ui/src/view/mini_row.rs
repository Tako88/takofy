//! The reduced row: a cover, a track over its artist, and one mono value.
//!
//! A station's rows and a split discovery card's rows are the same row, so they
//! are drawn by one painter. The caller owns what a click means and what the
//! trailing value says; this module owns the shape.

use egui::{Pos2, Rect, Sense, Ui, Vec2, pos2};
use takofy_model::TrackEntry;

use crate::cover::{Cover, want_px};
use crate::theme::{Tokens, extent, radius, space};

use super::art;
use super::controls;
use super::playlist::root::MediaDraw;
use super::text as recipe;

/// One reduced row, as a station or a split card draws it.
pub(in crate::view) struct Row<'a> {
    /// What makes this row's identity unique among every row on the page.
    pub salt: &'a str,
    pub track: &'a TrackEntry,
    /// The trailing value, when there is one to name.
    pub duration: Option<String>,
}

/// Draws one reduced row and returns its interaction.
///
/// A track can sit in two rows, so its identity is the caller's salt as well as
/// its own URI: one widget cannot stand for two rows.
pub(in crate::view) fn show(
    ui: &mut Ui,
    rect: Rect,
    row: Row<'_>,
    draw: &mut MediaDraw<'_, '_>,
    tokens: &Tokens,
) -> egui::Response {
    let id = ui
        .scope_id()
        .with(("home-station-row", row.salt, row.track.uri.as_str()));
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            true,
            format!("{} by {}", row.track.title, row.track.artists),
        )
    });
    if response.clicked() {
        response.request_focus();
    }
    let over = response.hovered() || ui.rect_contains_pointer(rect);
    if over {
        ui.painter()
            .rect_filled(rect, radius::SM, tokens.palette.row_hover);
    }
    controls::focus_ring(ui, &response, rect, radius::SM, tokens);

    let want = want_px(extent::QUEUE_ART, ui.pixels_per_point());
    let cover: Cover = draw.art.look(row.track.cover.as_ref(), want, draw.now);
    let art_rect = Rect::from_min_size(
        pos2(
            rect.left() + space::S2,
            rect.center().y - extent::QUEUE_ART / 2.0,
        ),
        Vec2::splat(extent::QUEUE_ART),
    );
    art::tile(
        ui.painter(),
        art_rect,
        radius::XS,
        tokens.palette.border,
        cover,
        tokens,
    );

    // The length goes at the row's far end, and the words take what it leaves.
    let meta = row.duration.as_deref().map(|label| {
        recipe::line(
            ui,
            label,
            recipe::track_mono(crate::theme::text::XXS),
            tokens.palette.text_subtle,
            0.0,
            rect.width(),
        )
    });
    let meta_w = meta.as_ref().map_or(0.0, |line| line.size().x + space::S3);

    let body_left = art_rect.right() + space::S3;
    let body_right = (rect.right() - space::S2 - meta_w).max(body_left);
    let width = body_right - body_left;
    let name = recipe::line(
        ui,
        &row.track.title,
        recipe::queue_name(),
        tokens.palette.text,
        0.0,
        width,
    );
    let sub = recipe::line(
        ui,
        &row.track.artists,
        recipe::queue_artist(),
        tokens.palette.text_subtle,
        0.0,
        width,
    );
    let block = name.size().y + sub.size().y;
    let top = rect.center().y - block / 2.0;
    ui.painter()
        .galley(Pos2::new(body_left, top), name.clone(), tokens.palette.text);
    ui.painter().galley(
        Pos2::new(body_left, top + name.size().y),
        sub,
        tokens.palette.text_subtle,
    );
    if let Some(meta) = meta {
        ui.painter().galley(
            Pos2::new(
                rect.right() - space::S2 - meta.size().x,
                rect.center().y - meta.size().y / 2.0,
            ),
            meta,
            tokens.palette.text_subtle,
        );
    }

    response
}

/// The trailing value a track names, from its label or the wire.
pub(in crate::view) fn duration_of(track: &TrackEntry, draw: &MediaDraw<'_, '_>) -> Option<String> {
    draw.library
        .home
        .duration(&track.uri)
        .or(track.duration_ms)
        .map(crate::format::format_position)
}

#[cfg(test)]
mod tests;
