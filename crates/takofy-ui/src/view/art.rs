//! The box a cover goes in.
//!
//! Every piece that draws a cover's box goes through this one seam. The [cover]
//! pipeline hands it a `Cover`, which is `Ready` with a texture, `Loading`,
//! `Missing` or `Failed`.
//!
//! Two looks, three meanings. A `Ready` tile draws its image, cropped to cover
//! the rect and rounded to it. `Loading` and `Failed` share the raised slot: from
//! the seat of someone at the window a slow cover and a broken one are the same
//! thing, and the store answers both without them — a failure retries itself,
//! and a cover that arrives fills the very box the placeholder was in. `Missing`
//! alone reads as a hole, because no cover exists and none is coming. None of
//! the four blanks the row.
//!
//! Nothing fades in. A fade is the age of a texture, and a `Cover` is a plain
//! value with no age — built by the store, held across frames by the panel and
//! compared in tests. Carrying one would reshape all of that to buy an effect
//! shorter than a frame of a scroll, so a cover swaps in whole.
//!
//! A row, a card and a header band differ in their radius and their border,
//! which is why those are arguments and the fill is not. Callers keep the rects
//! and radii they chose; only this body changes.

use egui::emath::GuiRounding;
use egui::epaint::RectShape;
use egui::{Color32, CornerRadius, Painter, Rect, Stroke, StrokeKind, TextureId};

use crate::cover::Cover;
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
        Cover::Loading | Cover::Failed => {
            block(painter, rect, radius, border, palette.surface_raised);
        }
        Cover::Missing => block(painter, rect, radius, border, palette.surface_sunken),
    }
}

/// Paints a cover's box as a picture and nothing else: no stroke, no rounded
/// edge of its own.
///
/// This is the artist hero's photograph, which the band clips at its own rect
/// and fades under its scrim. The three non-ready states fall back to the same
/// slot and hole [`tile`] draws, so a hero with no portrait is a flat band
/// rather than a blank one.
pub fn fill(painter: &Painter, rect: Rect, cover: Cover, tokens: &Tokens) {
    let rect = rect.round_to_pixels(painter.pixels_per_point());
    let palette = &tokens.palette;
    match cover {
        Cover::Ready(texture) => match texture_size(painter, texture) {
            Some(size) => {
                painter.add(
                    RectShape::filled(rect, CornerRadius::ZERO, art::TINT)
                        .with_texture(texture, cover_fit(rect.size(), size)),
                );
            }
            None => {
                painter.rect_filled(rect, 0.0, palette.surface_raised);
            }
        },
        Cover::Loading | Cover::Failed => {
            painter.rect_filled(rect, 0.0, palette.surface_raised);
        }
        Cover::Missing => {
            painter.rect_filled(rect, 0.0, palette.surface_sunken);
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

#[cfg(test)]
mod tests {
    use egui::{Shape, Vec2, pos2};

    use super::*;
    use crate::theme::Tokens;

    const SQUARE: Rect = Rect {
        min: pos2(0.0, 0.0),
        max: pos2(64.0, 64.0),
    };

    fn a_context_holding_a_cover() -> (egui::Context, egui::TextureHandle) {
        let ctx = egui::Context::default();
        let texture = ctx.load_texture(
            "a cover",
            egui::ColorImage::filled([4, 4], Color32::WHITE),
            art::TEXTURE,
        );
        // A load is queued and only reaches the texture manager in a pass.
        let mut output = ctx.run_ui(Default::default(), |_| {});
        output.textures_delta.clear();
        (ctx, texture)
    }

    fn painted(ctx: &egui::Context, cover: Cover, tokens: &Tokens) -> Vec<Shape> {
        let mut output = ctx.run_ui(Default::default(), |ui| {
            tile(
                ui.painter(),
                SQUARE,
                CornerRadius::ZERO,
                tokens.palette.border,
                cover,
                tokens,
            );
        });
        output.textures_delta.clear();
        output
            .shapes
            .into_iter()
            .map(|clipped| clipped.shape)
            .collect()
    }

    fn filled(ctx: &egui::Context, cover: Cover, tokens: &Tokens) -> Vec<Shape> {
        let mut output = ctx.run_ui(Default::default(), |ui| {
            fill(ui.painter(), SQUARE, cover, tokens);
        });
        output.textures_delta.clear();
        output
            .shapes
            .into_iter()
            .map(|clipped| clipped.shape)
            .collect()
    }

    fn rects(shapes: &[Shape]) -> Vec<&RectShape> {
        shapes
            .iter()
            .filter_map(|shape| match shape {
                Shape::Rect(rect) => Some(rect),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_filled_cover_is_the_picture_alone() {
        let tokens = Tokens::DARK;
        let (ctx, texture) = a_context_holding_a_cover();
        let shapes = filled(&ctx, Cover::Ready(texture.id()), &tokens);
        let rects = rects(&shapes);
        assert_eq!(rects.len(), 1, "the photograph and no hairline");
        assert_eq!(
            rects[0].brush.as_ref().expect("a texture").fill_texture_id,
            texture.id()
        );
    }

    #[test]
    fn a_missing_fill_is_the_sunken_well() {
        let tokens = Tokens::DARK;
        let (ctx, _) = a_context_holding_a_cover();
        let shapes = filled(&ctx, Cover::Missing, &tokens);
        assert_eq!(rects(&shapes)[0].fill, tokens.palette.surface_sunken);
    }

    #[test]
    fn a_loading_or_failed_fill_is_the_raised_slot() {
        let tokens = Tokens::DARK;
        let (ctx, _) = a_context_holding_a_cover();
        for state in [Cover::Loading, Cover::Failed] {
            assert_eq!(
                rects(&filled(&ctx, state, &tokens))[0].fill,
                tokens.palette.surface_raised,
                "{state:?} should read as a slot, not a hole"
            );
        }
    }

    #[test]
    fn every_state_draws_something() {
        let tokens = Tokens::DARK;
        let (ctx, texture) = a_context_holding_a_cover();
        for cover in [
            Cover::Ready(texture.id()),
            Cover::Loading,
            Cover::Missing,
            Cover::Failed,
        ] {
            assert!(
                !painted(&ctx, cover, &tokens).is_empty(),
                "{cover:?} blanked the box"
            );
        }
    }

    #[test]
    fn a_missing_cover_is_the_only_hole() {
        let tokens = Tokens::DARK;
        let (ctx, _) = a_context_holding_a_cover();
        let holes = painted(&ctx, Cover::Missing, &tokens);
        assert_eq!(rects(&holes)[0].fill, tokens.palette.surface_sunken);
        for state in [Cover::Loading, Cover::Failed] {
            assert_eq!(
                rects(&painted(&ctx, state, &tokens))[0].fill,
                tokens.palette.surface_raised,
                "{state:?} should read as a slot, not a hole"
            );
        }
    }

    #[test]
    fn a_ready_cover_is_the_image_under_its_hairline() {
        let tokens = Tokens::DARK;
        let (ctx, texture) = a_context_holding_a_cover();
        let shapes = painted(&ctx, Cover::Ready(texture.id()), &tokens);
        let rects = rects(&shapes);
        assert_eq!(rects.len(), 2, "the image and its edge");
        assert_eq!(
            rects[0].brush.as_ref().expect("a texture").fill_texture_id,
            texture.id()
        );
        assert_eq!(rects[1].stroke.color, tokens.palette.border);
    }

    #[test]
    fn a_square_cover_fills_its_box_whole() {
        assert_eq!(
            cover_fit(Vec2::new(64.0, 64.0), [300, 300]),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))
        );
        assert_eq!(
            cover_fit(Vec2::new(80.0, 40.0), [300, 150]),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))
        );
    }

    #[test]
    fn a_wide_cover_crops_its_sides_centred() {
        let uv = cover_fit(Vec2::new(64.0, 64.0), [2660, 1496]);
        assert!((uv.width() - 0.5624).abs() < 1e-3, "{uv:?}");
        assert!((uv.min.x - uv.max.x + uv.width()).abs() < 1e-6, "centred");
        assert_eq!(uv.min.y, 0.0);
        assert_eq!(uv.max.y, 1.0);
    }

    #[test]
    fn a_tall_cover_crops_its_ends_centred() {
        let uv = cover_fit(Vec2::new(64.0, 64.0), [750, 760]);
        assert_eq!(uv.min.x, 0.0);
        assert_eq!(uv.max.x, 1.0);
        assert!((uv.min.y - uv.max.y + uv.height()).abs() < 1e-6, "centred");
        assert!((uv.height() - 0.9868).abs() < 1e-3, "{uv:?}");
    }

    #[test]
    fn a_cover_fit_that_cannot_be_computed_is_the_whole_texture() {
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        assert_eq!(cover_fit(Vec2::new(64.0, 64.0), [0, 0]), full);
        assert_eq!(cover_fit(Vec2::ZERO, [300, 300]), full);
    }
}
