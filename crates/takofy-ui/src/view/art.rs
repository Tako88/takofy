//! The box a cover goes in.
//!
//! Every piece that draws a cover's box goes through this one seam. The [cover]
//! pipeline hands it a `Cover`, which is `Ready` with a texture, `Loading`,
//! `Missing` or `Failed`.
//!
//! Three looks, four states. A `Ready` tile draws its image, cropped to cover
//! the rect and rounded to it. `Loading` is the raised slot. `Missing` and
//! `Failed` are the sunken hole: no cover exists and none is coming, or the
//! fetch failed and its retries have not brought one yet. A failed cover keeps
//! retrying, and an image that arrives fills the very box the hole was in. None
//! of the four blanks the row.
//!
//! Nothing fades in. A fade is the age of a texture, and a `Cover` is a plain
//! value with no age — built by the store, held across frames by the panel and
//! compared in tests. Carrying one would reshape all of that to buy an effect
//! shorter than a frame of a scroll, so a cover swaps in whole.
//!
//! A row, a card and a header band differ in their radius and their border,
//! which is why those are arguments and the fill is not. Callers keep the rects
//! and radii they chose; only this body changes.

use std::time::Instant;

use egui::emath::GuiRounding;
use egui::epaint::RectShape;
use egui::{Color32, CornerRadius, Painter, Rect, Stroke, StrokeKind, TextureId};

use crate::cover::{Art, Cover};
use crate::layout::hero::{quadrant_corners, quadrants};
use crate::mosaic::QUADRANTS;
use crate::portraits::Portrait;
use crate::theme::{Tokens, art, stroke};

/// Paints a cover's box in one of four states.
pub fn tile(
    painter: &Painter,
    rect: Rect,
    radius: CornerRadius,
    border: Color32,
    cover: Cover,
    tokens: &Tokens,
) {
    let rect = rect.round_to_pixels(painter.pixels_per_point());
    let palette = &tokens.palette;
    match cover {
        Cover::Ready(texture) => match texture_size(painter, texture) {
            Some(size) => {
                painter.add(
                    RectShape::filled(rect, radius, art::TINT)
                        .with_texture(texture, cover_fit(rect.size(), size)),
                );
                painter.rect_stroke(
                    rect,
                    radius,
                    Stroke::new(stroke::HAIRLINE, border),
                    StrokeKind::Inside,
                );
            }
            // The texture went away under us; the slot is still the right box.
            None => block(painter, rect, radius, border, palette.surface_raised),
        },
        Cover::Loading => block(painter, rect, radius, border, palette.surface_raised),
        Cover::Missing | Cover::Failed => {
            block(painter, rect, radius, border, palette.surface_sunken);
        }
    }
}

/// Paints a box as a hole and nothing else: the sunken well at the caller's
/// radius, with no stroke of its own.
///
/// The idle Now Playing art box, where no cover exists and none is coming, so an
/// edge around it would frame nothing.
pub fn hole(painter: &Painter, rect: Rect, radius: CornerRadius, tokens: &Tokens) {
    let rect = rect.round_to_pixels(painter.pixels_per_point());
    painter.rect_filled(rect, radius, tokens.palette.surface_sunken);
}

/// Paints four covers quartering `rect`, then one hairline around the whole box.
///
/// Each quadrant rounds only its outside corner, so the four join without a
/// notch, and only the box carries a stroke: a quadrant that stroked itself would
/// draw a seam down the middle of the picture.
pub fn mosaic(
    painter: &Painter,
    rect: Rect,
    radius: CornerRadius,
    border: Color32,
    covers: [Cover; QUADRANTS],
    tokens: &Tokens,
) {
    let rect = rect.round_to_pixels(painter.pixels_per_point());
    let corners = quadrant_corners(radius);
    for ((quadrant, corner), cover) in quadrants(rect).into_iter().zip(corners).zip(covers) {
        fill_rounded(painter, quadrant, corner, cover, tokens);
    }
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(stroke::HAIRLINE, border),
        StrokeKind::Inside,
    );
}

/// A cover's pixels in `rect`, rounded to `radius`, with no stroke.
///
/// The non-ready states fall back to the same slot and hole [`tile`] draws, so a
/// quadrant with nothing to draw is still a box rather than a gap.
pub(crate) fn fill_rounded(
    painter: &Painter,
    rect: Rect,
    radius: CornerRadius,
    cover: Cover,
    tokens: &Tokens,
) {
    let rect = rect.round_to_pixels(painter.pixels_per_point());
    let palette = &tokens.palette;
    match cover {
        Cover::Ready(texture) => match texture_size(painter, texture) {
            Some(size) => {
                painter.add(
                    RectShape::filled(rect, radius, art::TINT)
                        .with_texture(texture, cover_fit(rect.size(), size)),
                );
            }
            None => {
                painter.rect_filled(rect, radius, palette.surface_raised);
            }
        },
        Cover::Loading => {
            painter.rect_filled(rect, radius, palette.surface_raised);
        }
        Cover::Missing | Cover::Failed => {
            painter.rect_filled(rect, radius, palette.surface_sunken);
        }
    }
}

/// A bare slot: a fill inside the piece's own hairline.
fn block(painter: &Painter, rect: Rect, radius: CornerRadius, border: Color32, fill: Color32) {
    painter.rect(
        rect,
        radius,
        fill,
        Stroke::new(stroke::HAIRLINE, border),
        StrokeKind::Inside,
    );
}

/// The pixel size of an uploaded texture, or `None` if it is gone.
fn texture_size(painter: &Painter, texture: TextureId) -> Option<[usize; 2]> {
    painter
        .ctx()
        .tex_manager()
        .read()
        .meta(texture)
        .map(|meta| meta.size)
}

/// The part of a `size` texture that fills `target`, centred.
///
/// A cover is scaled to cover, never stretched: the overflowing axis is cropped
/// evenly at both ends, so a wide banner keeps its middle rather than showing
/// bars. The crop is the whole reason a band's art looks right: a playlist's
/// desktop banner is 2660×1496 and an album cover is square.
fn cover_fit(target: egui::Vec2, size: [usize; 2]) -> Rect {
    let (width, height) = (size[0] as f32, size[1] as f32);
    let full = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    if width <= 0.0 || height <= 0.0 || target.x <= 0.0 || target.y <= 0.0 {
        return full;
    }
    let image = width / height;
    let rect = target.x / target.y;
    if image > rect {
        let visible = rect / image;
        let inset = (1.0 - visible) / 2.0;
        Rect::from_min_max(egui::pos2(inset, 0.0), egui::pos2(1.0 - inset, 1.0))
    } else {
        let visible = image / rect;
        let inset = (1.0 - visible) / 2.0;
        Rect::from_min_max(egui::pos2(0.0, inset), egui::pos2(1.0, 1.0 - inset))
    }
}

/// The artist row's cover rule: a portrait when there is one, the record's own
/// cover when the artist has no portrait group, and the raised slot while the
/// read has not answered so the row cannot flash from album art to a face.
pub(super) fn artist_cover(
    portrait: Portrait<'_>,
    record_cover: Option<&takofy_model::CoverSet>,
    art: &mut Art,
    want: u32,
    now: Instant,
) -> Cover {
    match portrait {
        Portrait::Known(set) => art.look(Some(set), want, now),
        Portrait::Absent => art.look(record_cover, want, now),
        Portrait::Unread => Cover::Loading,
    }
}

#[cfg(test)]
mod tests;
