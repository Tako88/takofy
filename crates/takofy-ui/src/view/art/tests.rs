use egui::{Shape, Vec2, pos2};

use super::*;
use crate::theme::Tokens;
use takofy_model::{CoverSet, CoverUrl};

fn url(tail: &str) -> CoverUrl {
    CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
}

fn cover(tail: &str) -> CoverSet {
    CoverSet::single(url(tail))
}

const SQUARE: Rect = Rect {
    min: pos2(0.0, 0.0),
    max: pos2(64.0, 64.0),
};

fn a_context_holding_a_cover() -> (egui::Context, egui::TextureHandle) {
    let ctx = egui::Context::default();
    let texture = ctx.load_texture(
        "a cover",
        egui::ColorImage::filled([4, 4], art::TINT),
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

fn holed(ctx: &egui::Context, rect: Rect, radius: CornerRadius, tokens: &Tokens) -> Vec<Shape> {
    let mut output = ctx.run_ui(Default::default(), |ui| {
        hole(ui.painter(), rect, radius, tokens);
    });
    output.textures_delta.clear();
    output
        .shapes
        .into_iter()
        .map(|clipped| clipped.shape)
        .collect()
}

fn mosaicked(ctx: &egui::Context, covers: [Cover; 4], tokens: &Tokens) -> Vec<Shape> {
    let mut output = ctx.run_ui(Default::default(), |ui| {
        mosaic(
            ui.painter(),
            SQUARE,
            CornerRadius::same(2),
            tokens.palette.border,
            covers,
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

/// A mosaic painted into `rect` on a window that draws at `pixels_per_point`.
fn mosaicked_on(rect: Rect, pixels_per_point: f32, tokens: &Tokens) -> Vec<Shape> {
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(pixels_per_point);
    let texture = ctx.load_texture(
        "a cover on an odd box",
        egui::ColorImage::filled([4, 4], art::TINT),
        art::TEXTURE,
    );
    // The first pass applies the scale and reaches the texture manager.
    let mut output = ctx.run_ui(Default::default(), |_| {});
    output.textures_delta.clear();
    let mut output = ctx.run_ui(Default::default(), |ui| {
        mosaic(
            ui.painter(),
            rect,
            CornerRadius::same(2),
            tokens.palette.border,
            [Cover::Ready(texture.id()); QUADRANTS],
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

fn a_context_holding_a_wide_cover() -> (egui::Context, egui::TextureHandle) {
    let ctx = egui::Context::default();
    let texture = ctx.load_texture(
        "a wide cover",
        egui::ColorImage::filled([64, 36], art::TINT),
        art::TEXTURE,
    );
    let mut output = ctx.run_ui(Default::default(), |_| {});
    output.textures_delta.clear();
    (ctx, texture)
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
fn a_hole_is_the_sunken_fill_and_nothing_else() {
    let tokens = Tokens::DARK;
    let (ctx, _) = a_context_holding_a_cover();
    let shapes = holed(&ctx, SQUARE, CornerRadius::ZERO, &tokens);
    assert_eq!(shapes.len(), 1, "a hole is one shape: {shapes:?}");
    let rects = rects(&shapes);
    assert_eq!(rects.len(), 1, "a hole is a fill, not a stroke");
    assert_eq!(rects[0].rect, SQUARE);
    assert_eq!(rects[0].fill, tokens.palette.surface_sunken);
    assert!(rects[0].stroke.is_empty(), "a hole draws no hairline");
    assert!(rects[0].brush.is_none(), "a hole holds no texture");
}

#[test]
fn a_hole_honours_the_radius() {
    let tokens = Tokens::DARK;
    let (ctx, _) = a_context_holding_a_cover();
    let radius = CornerRadius::same(2);
    let shapes = holed(&ctx, SQUARE, radius, &tokens);
    assert_eq!(rects(&shapes)[0].corner_radius, radius);
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
fn loading_is_the_raised_slot_and_missing_and_failed_are_the_hole() {
    let tokens = Tokens::DARK;
    let (ctx, _) = a_context_holding_a_cover();
    let loading = painted(&ctx, Cover::Loading, &tokens);
    assert_eq!(
        rects(&loading)[0].fill,
        tokens.palette.surface_raised,
        "a cover with nothing failed yet is a slot"
    );

    let missing = painted(&ctx, Cover::Missing, &tokens);
    let failed = painted(&ctx, Cover::Failed, &tokens);
    let (missing, failed) = (rects(&missing), rects(&failed));
    assert_eq!(missing.len(), 1, "a hole is one fill with its edge");
    assert_eq!(failed.len(), 1, "a failure draws the same fill");
    assert_eq!(missing[0].fill, tokens.palette.surface_sunken);
    assert_eq!(failed[0].fill, tokens.palette.surface_sunken);
    assert_eq!(failed[0].rect, missing[0].rect);
    assert_eq!(failed[0].stroke, missing[0].stroke);
    assert_eq!(failed[0].corner_radius, missing[0].corner_radius);
    assert!(failed[0].brush.is_none(), "the hole holds no texture");
    assert_ne!(
        rects(&loading)[0].fill,
        missing[0].fill,
        "a slot is not the hole"
    );
}

#[test]
fn a_failed_tile_keeps_the_hairline_like_a_missing_one() {
    let tokens = Tokens::DARK;
    let (ctx, _) = a_context_holding_a_cover();
    let shapes = painted(&ctx, Cover::Failed, &tokens);
    let rects = rects(&shapes);
    assert_eq!(rects.len(), 1, "the hole is one fill with its edge");
    assert_eq!(rects[0].stroke.color, tokens.palette.border);
    assert_eq!(rects[0].stroke.width, stroke::HAIRLINE);
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
fn a_mosaic_draws_four_textured_quadrants_and_one_outer_hairline() {
    let tokens = Tokens::DARK;
    let (ctx, texture) = a_context_holding_a_cover();
    let shapes = mosaicked(&ctx, [Cover::Ready(texture.id()); 4], &tokens);
    let rects = rects(&shapes);
    assert_eq!(rects.len(), 5, "four quadrants and the box's own edge");
    let textured: Vec<&&RectShape> = rects.iter().filter(|rect| rect.brush.is_some()).collect();
    assert_eq!(textured.len(), 4, "every quadrant is a picture");
    for quadrant in textured {
        assert_eq!(
            quadrant.brush.as_ref().expect("a texture").fill_texture_id,
            texture.id()
        );
    }
    let outer = rects
        .iter()
        .find(|rect| rect.brush.is_none())
        .expect("the box has one edge");
    assert_eq!(outer.rect, SQUARE);
    assert_eq!(outer.stroke.color, tokens.palette.border);
}

#[test]
fn a_missing_quadrant_is_the_sunken_hole() {
    let tokens = Tokens::DARK;
    let (ctx, texture) = a_context_holding_a_cover();
    let shapes = mosaicked(
        &ctx,
        [
            Cover::Missing,
            Cover::Ready(texture.id()),
            Cover::Ready(texture.id()),
            Cover::Missing,
        ],
        &tokens,
    );
    let rects = rects(&shapes);
    let holes: Vec<&&RectShape> = rects
        .iter()
        .filter(|rect| rect.brush.is_none() && rect.fill == tokens.palette.surface_sunken)
        .collect();
    assert_eq!(holes.len(), 2, "two quadrants have nothing to draw");
    let quadrants = crate::layout::hero::quadrants(SQUARE);
    assert!(holes.iter().any(|hole| hole.rect == quadrants[0]));
    assert!(holes.iter().any(|hole| hole.rect == quadrants[3]));
}

#[test]
fn a_failed_quadrant_is_the_sunken_hole() {
    let tokens = Tokens::DARK;
    let (ctx, texture) = a_context_holding_a_cover();
    let quadrants = crate::layout::hero::quadrants(SQUARE);
    let shapes = mosaicked(
        &ctx,
        [
            Cover::Failed,
            Cover::Ready(texture.id()),
            Cover::Ready(texture.id()),
            Cover::Failed,
        ],
        &tokens,
    );
    let sunken: Vec<Rect> = rects(&shapes)
        .iter()
        .filter(|rect| rect.brush.is_none() && rect.fill == tokens.palette.surface_sunken)
        .map(|rect| rect.rect)
        .collect();
    assert_eq!(sunken.len(), 2, "two quadrants failed");
    assert!(sunken.contains(&quadrants[0]));
    assert!(sunken.contains(&quadrants[3]));

    // A quadrant still fetching keeps the raised slot.
    let shapes = mosaicked(
        &ctx,
        [
            Cover::Loading,
            Cover::Ready(texture.id()),
            Cover::Ready(texture.id()),
            Cover::Ready(texture.id()),
        ],
        &tokens,
    );
    let raised = rects(&shapes)
        .into_iter()
        .find(|rect| rect.rect == quadrants[0])
        .expect("the loading quadrant");
    assert_eq!(raised.fill, tokens.palette.surface_raised);
}

#[test]
fn a_mosaic_quadrant_has_no_stroke_of_its_own() {
    let tokens = Tokens::DARK;
    let (ctx, texture) = a_context_holding_a_cover();
    let shapes = mosaicked(&ctx, [Cover::Ready(texture.id()); 4], &tokens);
    let rects = rects(&shapes);
    for quadrant in rects.iter().filter(|rect| rect.brush.is_some()) {
        assert!(
            quadrant.stroke.is_empty(),
            "a quadrant draws its own seam if it strokes itself"
        );
    }
}

#[test]
fn a_mosaic_tiles_an_odd_box_on_the_pixel_grid() {
    let tokens = Tokens::DARK;
    // An odd number of device pixels puts the box's centre on a half pixel,
    // where rounding the box and then each quadrant could open a seam.
    for (pixels_per_point, rect) in [
        (1.0, Rect::from_min_max(pos2(10.0, 10.0), pos2(15.0, 15.0))),
        (2.0, Rect::from_min_max(pos2(10.0, 10.0), pos2(15.5, 15.5))),
    ] {
        let shapes = mosaicked_on(rect, pixels_per_point, &tokens);
        let rects = rects(&shapes);
        let box_ = rect.round_to_pixels(pixels_per_point);
        assert_ne!(
            (box_.center().x * pixels_per_point).fract(),
            0.0,
            "the case puts the centre between two pixels at {pixels_per_point}x"
        );

        let quadrants: Vec<Rect> = rects
            .iter()
            .filter(|rect| rect.brush.is_some())
            .map(|rect| rect.rect)
            .collect();
        assert_eq!(quadrants.len(), QUADRANTS);
        assert_eq!(quadrants[0].min, box_.min);
        assert_eq!(quadrants[3].max, box_.max);
        assert_eq!(quadrants[0].max.x, quadrants[1].min.x, "the top seam");
        assert_eq!(quadrants[2].max.x, quadrants[3].min.x, "the bottom seam");
        assert_eq!(quadrants[0].max.y, quadrants[2].min.y, "the left seam");
        assert_eq!(quadrants[1].max.y, quadrants[3].min.y, "the right seam");
        let covered: f32 = quadrants.iter().map(|quadrant| quadrant.area()).sum();
        assert_eq!(covered, box_.area(), "the quadrants cover the box exactly");

        for edge in quadrants.iter().flat_map(|quadrant| {
            [
                quadrant.min.x,
                quadrant.min.y,
                quadrant.max.x,
                quadrant.max.y,
            ]
        }) {
            assert_eq!(
                (edge * pixels_per_point).fract(),
                0.0,
                "{edge} is off the pixel grid at {pixels_per_point}x"
            );
        }

        let stroked: Vec<Rect> = rects
            .iter()
            .filter(|rect| !rect.stroke.is_empty())
            .map(|rect| rect.rect)
            .collect();
        assert_eq!(
            stroked,
            vec![box_],
            "the box carries the only hairline, so no seam is drawn twice"
        );
    }
}

#[test]
fn quadrant_textures_are_cropped_to_cover() {
    let tokens = Tokens::DARK;
    let (ctx, texture) = a_context_holding_a_wide_cover();
    let shapes = mosaicked(&ctx, [Cover::Ready(texture.id()); 4], &tokens);
    let rects = rects(&shapes);
    let expected = cover_fit(Vec2::new(32.0, 32.0), [64, 36]);
    assert!(expected.width() < 1.0, "a wide picture is cropped");
    for quadrant in rects.iter().filter(|rect| rect.brush.is_some()) {
        let brush = quadrant.brush.as_ref().expect("a texture");
        assert_eq!(brush.fill_texture_id, texture.id());
        assert!(
            (brush.uv.width() - expected.width()).abs() < 1e-6,
            "a quadrant crops like the whole box does"
        );
        assert!((brush.uv.min.x - expected.min.x).abs() < 1e-6);
    }
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

#[test]
fn an_unread_artist_draws_the_raised_slot_without_looking_at_the_record_cover() {
    let mut art = Art::default();
    art.begin_frame();
    let record = cover("record");
    let drawn = artist_cover(
        Portrait::Unread,
        Some(&record),
        &mut art,
        64,
        Instant::now(),
    );
    assert_eq!(drawn, Cover::Loading, "an unread row is the raised slot");
    assert!(
        art.requests(Instant::now()).is_empty(),
        "it must not look at the album cover first, or the row flashes"
    );
}

#[test]
fn an_absent_artist_looks_at_the_record_cover() {
    let mut art = Art::default();
    art.begin_frame();
    let record = cover("record");
    let drawn = artist_cover(
        Portrait::Absent,
        Some(&record),
        &mut art,
        64,
        Instant::now(),
    );
    assert_eq!(drawn, Cover::Loading, "a new cover is fetched");
    assert_eq!(
        art.requests(Instant::now()),
        vec![crate::message::Command::Fetch { url: url("record") }],
        "the fallback is the record's own cover"
    );
}

#[test]
fn a_known_artist_looks_at_its_portrait() {
    let mut art = Art::default();
    art.begin_frame();
    let portrait = cover("portrait");
    let drawn = artist_cover(
        Portrait::Known(&portrait),
        Some(&cover("record")),
        &mut art,
        64,
        Instant::now(),
    );
    assert_eq!(drawn, Cover::Loading);
    assert_eq!(
        art.requests(Instant::now()),
        vec![crate::message::Command::Fetch {
            url: url("portrait")
        }],
        "the portrait wins over the record cover"
    );
}
