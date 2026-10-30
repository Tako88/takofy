//! The counted shelf: the artists the library holds the most of.
//!
//! The reference draws this shelf as `.tiles`, not as cards: a compact bordered
//! row that keeps its own box, filled across the page at a 15rem floor, a round
//! portrait and a name. The shelf counts a holding rather than showing one, so a
//! card would say more with its cover than the shelf has to say.

use egui::{Pos2, Rect, Sense, Ui, Vec2, pos2, vec2};

use crate::cover::want_px;
use crate::format::{HOME_COUNTED_TITLE, HOME_DOOR_ARTISTS};
use crate::home::counted::Tiles;
use crate::layout::chips;
use crate::route::Page;
use crate::theme::{Tokens, extent, radius, space, stroke};
use crate::view::playlist::root::MediaDraw;

use super::super::{art, section, text as recipe};
use super::Pressed;

/// The portrait's box inside a tile: the reference's 2.5rem.
const PORTRAIT: f32 = extent::QUEUE_ART;

/// Draws the shelf's head and its tiles, and nothing until it holds an artist.
pub(super) fn show(
    ui: &mut Ui,
    tiles: &Tiles<'_>,
    _now_ms: u64,
    tokens: &Tokens,
    draw: &mut MediaDraw<'_, '_>,
    out: &mut Pressed,
) {
    if tiles.artists.is_empty() {
        return;
    }
    section::head(ui, HOME_COUNTED_TITLE, "", tokens);
    let width = ui.available_width();
    let placed = chips::grid(
        tiles.artists.len(),
        width,
        columns(width),
        extent::HOME_ROW_H,
        space::S2,
    );
    let (list, _) = ui.allocate_exact_size(vec2(width, placed.height), Sense::hover());
    let origin = list.min.to_vec2();
    let visible = ui.clip_rect();
    let want = want_px(PORTRAIT, ui.pixels_per_point());

    for (artist, at) in tiles.artists.iter().zip(placed.rects) {
        let rect = at.translate(origin);
        if !rect.intersects(visible) {
            continue;
        }
        let response = ui.interact(
            rect,
            ui.scope_id().with(("home-counted-tile", artist.uri)),
            Sense::click(),
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, artist.name));
        if response.clicked() {
            response.request_focus();
            *draw.open = Some(Page::Artist(artist.uri.to_owned()));
        }
        let over = response.hovered() || ui.rect_contains_pointer(rect);
        ui.painter().rect(
            rect,
            radius::SM,
            if over {
                tokens.palette.surface_hover
            } else {
                tokens.palette.surface_raised
            },
            egui::Stroke::new(
                stroke::HAIRLINE,
                if over {
                    tokens.palette.border_strong
                } else {
                    tokens.palette.border
                },
            ),
            egui::StrokeKind::Inside,
        );
        super::super::controls::focus_ring(ui, &response, rect, radius::SM, tokens);

        let portrait = Rect::from_center_size(
            pos2(rect.left() + space::S2 + PORTRAIT / 2.0, rect.center().y),
            Vec2::splat(PORTRAIT),
        );
        art::tile(
            ui.painter(),
            portrait,
            radius::PILL,
            tokens.palette.border,
            draw.art.look(artist.portrait, want, draw.now),
            tokens,
        );

        let left = portrait.right() + space::S3;
        let name = recipe::line(
            ui,
            artist.name,
            recipe::queue_name(),
            tokens.palette.text,
            0.0,
            (rect.right() - space::S2 - left).max(0.0),
        );
        ui.painter().galley(
            Pos2::new(left, rect.center().y - name.size().y / 2.0),
            name,
            tokens.palette.text,
        );
    }
    if super::door::show(ui, HOME_DOOR_ARTISTS, tokens) {
        out.door = Some(super::Door::Artists);
    }
}

/// How many tiles fit across `width`: the reference's 15rem floor, and one tile
/// when even that does not fit.
pub(super) fn columns(width: f32) -> usize {
    let fit = ((width + space::S2) / (extent::HOME_TILE_MIN_W + space::S2)).floor();
    (fit as usize).max(1)
}

#[cfg(test)]
mod tests;
