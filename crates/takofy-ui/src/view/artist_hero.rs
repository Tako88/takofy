//! The artist page's band: the plate, the name and the shelf of releases.
//!
//! The band is a solid wash sampled from the portrait, a square plate at the
//! gutter, the name beside it over a byline, and the artist's oldest, middle and
//! newest release covers standing at the far edge. The shelf is masked at its
//! left edge and is not drawn at the phone tier, where the name needs the width.
//!
//! The band is all this view draws. Its action row is the shared one, which is
//! the only way the row can pin while the discography scrolls under it; the page
//! owns the presses that row reports.

use std::sync::Arc;
use std::time::Instant;

use egui::{Color32, FontId, Mesh, Rect, Sense, Shape, Ui, pos2};
use takofy_model::{CoverSet, Release};

use crate::cover::{Art, want_px};
use crate::discography;
use crate::format::discography_meta;
use crate::layout::hero::{artist_frame, balanced_width};
use crate::theme::{Tokens, Weight, fonts, hero, radius, space, tracking};

use super::{art, hero as band, text as recipe};

/// What the artist band says.
pub struct ArtistHero<'a> {
    pub name: &'a str,
    /// The portrait the plate draws, and the art the wash is sampled from.
    pub portrait: Option<&'a CoverSet>,
    /// The artist's releases, in the wire's order.
    pub releases: &'a [Release],
}

/// Draws the artist band.
///
/// The band takes the width it is given and starts at the cursor, so the page
/// decides where it sits; the painter decides only how it looks.
pub fn show(ui: &mut Ui, artist: &ArtistHero<'_>, art: &mut Art, now: Instant, tokens: &Tokens) {
    let band_w = ui.available_width();
    let shelf = discography::shelf(artist.releases);
    let frame = artist_frame(ui.ctx().content_rect().width(), shelf.len());
    let pixels_per_point = ui.pixels_per_point();

    let want = want_px(frame.plate, pixels_per_point);
    let plate = art.look(artist.portrait, want, now);
    let tint = art.tint_held(artist.portrait, want, &tokens.palette.tint);
    let fill = tint.unwrap_or(tokens.palette.hero_wash);

    let text_width = frame.text_width(band_w);
    let title =
        |width: f32, rows: usize| name(ui, artist.name, frame.title_size, tokens, width, rows);
    // The probe may take one row past the cap, so a name that outgrows it is
    // seen to do so; only the painted galley gets the cap.
    let balanced = balanced_width(
        text_width,
        |width| title(width, hero::ARTIST_TITLE_ROWS + 1).rows.len(),
        hero::ARTIST_TITLE_ROWS,
    );
    let title = title(balanced, hero::ARTIST_TITLE_ROWS);
    let words = discography_meta(
        artist.releases.len(),
        artist
            .releases
            .iter()
            .map(|release| release.tracks.len())
            .sum(),
    );
    let byline = (!words.is_empty()).then(|| band::byline_line(ui, &words, text_width, tokens));
    let text_h = title.size().y
        + byline
            .as_ref()
            .map_or(0.0, |line| space::S2 + line.size().y);

    let rects = frame.rects(ui.cursor().min, band_w, text_h);
    let (rect, _) = ui.allocate_exact_size(rects.band.size(), Sense::hover());
    band::wash(ui, rect, tint, tokens);

    art::tile(
        ui.painter(),
        rects.plate,
        radius::SM,
        tokens.palette.border_strong,
        plate,
        tokens,
    );

    let words_clip = rects.text.intersect(ui.clip_rect());
    if words_clip.width() > 0.0 && words_clip.height() > 0.0 {
        let painter = ui.painter().with_clip_rect(words_clip);
        painter.galley(rects.text.min, title, tokens.palette.text);
        if let Some(line) = byline {
            let top = rects.text.bottom() - line.size().y;
            painter.galley(
                pos2(rects.text.left(), top),
                line,
                tokens.palette.text_muted,
            );
        }
    }

    let Some(shelf_box) = frame.shelf else {
        return;
    };
    let (Some(shelf_rect), Some(fade)) = (rects.shelf, rects.fade) else {
        return;
    };
    let clip = shelf_rect.intersect(ui.clip_rect());
    if clip.width() <= 0.0 || clip.height() <= 0.0 {
        return;
    }
    let painter = ui.painter().with_clip_rect(clip);
    let want = want_px(shelf_box.side, pixels_per_point);
    for (rect, cover) in rects.covers.iter().zip(&shelf) {
        let drawn = art.look(Some(cover), want, now);
        art::fill_rounded(&painter, *rect, radius::NONE, drawn, tokens);
    }
    mask(&painter, fade, fill);
}

/// The name at `width`, capped at `rows` and elided past them.
fn name(
    ui: &Ui,
    name: &str,
    size: f32,
    tokens: &Tokens,
    width: f32,
    rows: usize,
) -> Arc<egui::Galley> {
    recipe::clamped(
        ui,
        name,
        FontId::new(size, fonts::sans(Weight::Black)),
        tokens.palette.text,
        recipe::tracking_pts(size, tracking::TIGHT),
        width,
        rows,
    )
}

/// The shelf's left mask: the band's own fill fading to nothing.
fn mask(painter: &egui::Painter, rect: Rect, fill: Color32) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.left_top(), fill);
    mesh.colored_vertex(rect.right_top(), Color32::TRANSPARENT);
    mesh.colored_vertex(rect.left_bottom(), fill);
    mesh.colored_vertex(rect.right_bottom(), Color32::TRANSPARENT);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 2, 3);
    painter.add(Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use egui::{Context, Pos2, RawInput, Vec2, epaint::TextShape, vec2};

    use super::*;
    use crate::cover::want_px;
    use crate::layout::hero::plate_side;
    use crate::message::Update;
    use crate::theme::{Tokens, fonts, stroke, style};
    use takofy_model::{CoverImage, CoverUrl, ReleaseKind};

    const PAGE: Vec2 = Vec2::new(1240.0, 800.0);

    fn context() -> Context {
        let tokens = Tokens::DARK;
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));
        ctx
    }

    fn input(size: Vec2) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            ..Default::default()
        }
    }

    fn url(tail: &str) -> CoverUrl {
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
    }

    fn cover(tail: &str) -> CoverSet {
        CoverSet::single(url(tail))
    }

    fn release(name: &str, year: Option<u16>, tracks: &[&str], cover: Option<CoverSet>) -> Release {
        Release {
            uri: format!("spotify:release:{name}"),
            name: name.to_owned(),
            kind: ReleaseKind::Album,
            year,
            cover,
            tracks: tracks.iter().map(|track| (*track).to_owned()).collect(),
        }
    }

    fn hero<'a>(
        name: &'a str,
        portrait: Option<&'a CoverSet>,
        releases: &'a [Release],
    ) -> ArtistHero<'a> {
        ArtistHero {
            name,
            portrait,
            releases,
        }
    }

    /// One frame of the band, with the store the test owns.
    fn draw(
        ctx: &Context,
        artist: &ArtistHero<'_>,
        art: &mut Art,
        now: Instant,
        size: Vec2,
    ) -> egui::FullOutput {
        let tokens = Tokens::DARK;
        let mut output = ctx.run_ui(input(size), |ui| {
            show(ui, artist, art, now, &tokens);
        });
        output.textures_delta.clear();
        output
    }

    /// One frame of the band in a pane narrower than the window.
    fn draw_in(
        ctx: &Context,
        artist: &ArtistHero<'_>,
        art: &mut Art,
        now: Instant,
        window: Vec2,
        band_w: f32,
    ) -> egui::FullOutput {
        let tokens = Tokens::DARK;
        let mut output = ctx.run_ui(input(window), |ui| {
            ui.allocate_ui_with_layout(
                vec2(band_w, window.y),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    show(ui, artist, art, now, &tokens);
                },
            );
        });
        output.textures_delta.clear();
        output
    }

    /// The height the band takes when it is the only thing in the page.
    fn band_height(ctx: &Context, artist: &ArtistHero<'_>, size: Vec2, band_w: f32) -> f32 {
        let tokens = Tokens::DARK;
        let now = Instant::now();
        let mut art = Art::default();
        let mut height = 0.0;
        let mut output = ctx.run_ui(input(size), |ui| {
            ui.allocate_ui_with_layout(
                vec2(band_w, size.y),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    show(ui, artist, &mut art, now, &tokens);
                    height = ui.min_rect().height();
                },
            );
        });
        output.textures_delta.clear();
        height
    }

    /// Holds `set`'s pick for `want`, as the fetches would have by the next frame.
    fn hold(ctx: &Context, art: &mut Art, set: &CoverSet, want: u32, now: Instant) {
        hold_colored(ctx, art, set, want, now, [255, 0, 0, 255]);
    }

    /// Holds `set`'s pick as a texture of `rgba`, so held covers can be told apart.
    fn hold_colored(
        ctx: &Context,
        art: &mut Art,
        set: &CoverSet,
        want: u32,
        now: Instant,
        rgba: [u8; 4],
    ) {
        art.begin_frame();
        let _ = art.look(Some(set), want, now);
        while !art.requests(now).is_empty() {}
        let url = set.pick(want).expect("a picked cover").clone();
        art.apply(
            &Update::Cover {
                url,
                image: CoverImage::new(1, 1, rgba.to_vec()).expect("a 1x1 image"),
            },
            now,
        );
        art.upload(ctx, crate::theme::TintParams::DARK.alpha_floor);
    }

    /// The rects the frame filled with `color`.
    fn fills(output: &egui::FullOutput, color: Color32) -> Vec<Rect> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape) if shape.fill == color => Some(shape.rect),
                _ => None,
            })
            .collect()
    }

    /// The textured rects whose centre is inside `area`, in paint order.
    fn textured(output: &egui::FullOutput, area: Rect) -> Vec<Rect> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape)
                    if shape.brush.is_some() && area.contains(shape.rect.center()) =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
            .collect()
    }

    /// The textures painted inside `area`, left to right.
    fn textures(output: &egui::FullOutput, area: Rect) -> Vec<egui::TextureId> {
        let mut drawn: Vec<(Rect, egui::TextureId)> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape)
                    if shape.brush.is_some() && area.contains(shape.rect.center()) =>
                {
                    Some((
                        shape.rect,
                        shape
                            .brush
                            .as_ref()
                            .expect("a textured rect")
                            .fill_texture_id,
                    ))
                }
                _ => None,
            })
            .collect();
        drawn.sort_by(|left, right| left.0.left().total_cmp(&right.0.left()));
        drawn.into_iter().map(|(_, texture)| texture).collect()
    }

    /// The box the band's wash fills: the full-width rect as tall as the band.
    fn band_rect(output: &egui::FullOutput, width: f32) -> Rect {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape)
                    if (shape.rect.width() - width).abs() < 0.5
                        && shape.rect.height() >= hero::ARTIST_MIN_H_NARROW =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
            .expect("the band paints its own wash")
    }

    /// The painted line carrying `label`, as the frame drew it.
    fn painted<'a>(output: &'a egui::FullOutput, label: &str) -> Option<&'a TextShape> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => Some(text),
                _ => None,
            })
    }

    /// The box a painted line occupies.
    fn rect_of(text: &TextShape) -> Rect {
        Rect::from_min_size(text.pos, text.galley.size())
    }

    /// The title's painted line: the one in full ink.
    fn title(output: &egui::FullOutput) -> &TextShape {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text)
                    if text.fallback_color == Tokens::DARK.palette.text =>
                {
                    Some(text)
                }
                _ => None,
            })
            .expect("the band paints its name")
    }

    /// Every text the frame painted.
    fn drawn_texts(output: &egui::FullOutput) -> Vec<&str> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            })
            .collect()
    }

    fn assert_close(left: f32, right: f32) {
        assert!((left - right).abs() < 0.01, "{left} is not {right}");
    }

    #[test]
    fn the_band_is_as_tall_as_its_frame_at_each_tier() {
        let ctx = context();
        let short = hero("Vela Moreno", None, &[]);
        let narrow = band_height(&ctx, &short, Vec2::new(900.0, 800.0), 900.0);
        assert_close(narrow, hero::ARTIST_MIN_H_NARROW);
        let wide = band_height(&ctx, &short, PAGE, PAGE.x);
        assert_close(wide, hero::ARTIST_MIN_H);

        // Points, not device pixels: a 2x window is the same band.
        let ctx = context();
        ctx.set_pixels_per_point(2.0);
        let retina = band_height(&ctx, &short, PAGE, PAGE.x);
        assert_close(retina, hero::ARTIST_MIN_H);
    }

    #[test]
    fn the_phone_tier_stacks_the_plate_above_a_full_width_title() {
        for window in [Vec2::new(640.0, 800.0), Vec2::new(760.0, 800.0)] {
            let ctx = context();
            let now = Instant::now();
            let artist = hero("Vela Moreno", None, &[]);
            let mut art = Art::default();
            let output = draw(&ctx, &artist, &mut art, now, window);

            let band = band_rect(&output, window.x);
            let drawn = title(&output);
            assert_close(drawn.galley.job.wrap.max_width, band.width());
            assert_close(rect_of(drawn).left(), band.left());

            let plate = fills(&output, Tokens::DARK.palette.surface_sunken);
            assert_eq!(plate.len(), 1, "the missing portrait is the one plate");
            assert!(
                (plate[0].bottom() + space::S4 - rect_of(drawn).top()).abs() < 0.5,
                "the plate sits one gap above the title"
            );
            let stacked =
                space::S5 + plate_side(window.x) + space::S4 + drawn.galley.size().y + space::S5;
            assert!(
                (band.height() - stacked).abs() < 0.5,
                "the band is {} tall, not the stacked {stacked}",
                band.height()
            );
        }
    }

    #[test]
    fn a_three_row_title_still_sits_inside_the_frames_height() {
        let ctx = context();
        let now = Instant::now();
        let name = "word ".repeat(24);
        let releases = vec![release("One", Some(2020), &["a", "b"], None)];
        let artist = hero(&name, None, &releases);
        let mut art = Art::default();
        let output = draw_in(&ctx, &artist, &mut art, now, PAGE, 680.0);

        let drawn = title(&output);
        assert_eq!(drawn.galley.rows.len(), hero::ARTIST_TITLE_ROWS);
        let byline = painted(&output, &discography_meta(1, 2)).expect("the byline is painted");
        let text_h = rect_of(drawn).height() + space::S2 + rect_of(byline).height();
        // The floor governs with the bundled face; the growth branch is the
        // frame's own, tested where the column height is an argument.
        let expected = (2.0 * space::S5 + plate_side(PAGE.x).max(text_h)).max(hero::ARTIST_MIN_H);
        let band = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape)
                    if (shape.rect.width() - 680.0).abs() < 0.5
                        && shape.rect.height() >= hero::ARTIST_MIN_H_NARROW =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
            .expect("the band paints its own wash");
        assert_close(band.height(), expected);
    }

    #[test]
    fn a_held_plate_is_a_square_of_art_that_tints_the_band() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let now = Instant::now();
        let portrait = cover("portrait");
        let artist = hero("Vela Moreno", Some(&portrait), &[]);
        let mut art = Art::default();
        let want = want_px(plate_side(PAGE.x), 1.0);
        hold(&ctx, &mut art, &portrait, want, now);
        let output = draw(&ctx, &artist, &mut art, now, PAGE);

        let band = band_rect(&output, PAGE.x);
        let some = textured(&output, band);
        assert_eq!(some.len(), 1, "the plate is the one held picture");
        let plate = some[0];
        assert_close(plate.width(), plate_side(PAGE.x));
        assert_close(plate.height(), plate_side(PAGE.x));
        assert_close(plate.left(), space::GUTTER);
        assert_close(plate.bottom(), band.bottom() - stroke::HAIRLINE - space::S5);

        assert!(
            output.shapes.iter().any(|clipped| matches!(
                &clipped.shape,
                egui::epaint::Shape::Rect(shape)
                    if shape.rect == plate
                        && (shape.stroke.width - stroke::HAIRLINE).abs() < 0.01
                        && shape.stroke.color == tokens.palette.border_strong
            )),
            "the plate wears the strong hairline"
        );
        assert_eq!(
            fills(&output, crate::theme::tint_color([116, 27, 27])),
            vec![band],
            "the portrait's own mean tints the band"
        );
        assert!(
            fills(&output, tokens.palette.hero_wash).is_empty(),
            "a tinted band does not also paint the fallback"
        );
    }

    #[test]
    fn an_unheld_plate_leaves_the_wash_and_a_raised_slot() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let now = Instant::now();
        let portrait = cover("portrait");
        let artist = hero("Vela Moreno", Some(&portrait), &[]);
        let mut art = Art::default();
        let output = draw(&ctx, &artist, &mut art, now, PAGE);

        let band = band_rect(&output, PAGE.x);
        assert_eq!(fills(&output, tokens.palette.hero_wash), vec![band]);
        let slots = fills(&output, tokens.palette.surface_raised);
        assert_eq!(slots.len(), 1, "the loading plate is a raised slot");
        assert_close(slots[0].width(), plate_side(PAGE.x));
    }

    #[test]
    fn a_missing_portrait_is_a_sunken_plate() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let now = Instant::now();
        let artist = hero("Vela Moreno", None, &[]);
        let mut art = Art::default();
        let output = draw(&ctx, &artist, &mut art, now, PAGE);
        let holes = fills(&output, tokens.palette.surface_sunken);
        assert_eq!(holes.len(), 1, "no portrait is a hole, not a slot");
        assert_close(holes[0].width(), plate_side(PAGE.x));
    }

    #[test]
    fn the_plate_is_fetched_at_its_own_size() {
        let portrait = CoverSet::from_sizes([
            (Some((160, 160)), url("small")),
            (Some((640, 640)), url("large")),
        ])
        .expect("a portrait set");
        let artist = hero("Vela Moreno", Some(&portrait), &[]);

        let now = Instant::now();
        let ctx = context();
        let mut art = Art::default();
        art.begin_frame();
        let _ = draw(&ctx, &artist, &mut art, now, Vec2::new(900.0, 800.0));
        let requested = art.requests(now);
        assert_eq!(
            requested,
            vec![crate::message::Command::Fetch {
                url: portrait
                    .pick(want_px(plate_side(900.0), 1.0))
                    .cloned()
                    .expect("a pick"),
            }],
            "a 144-point plate asks for the 160, not the 640"
        );

        let ctx = context();
        ctx.set_pixels_per_point(2.0);
        let mut art = Art::default();
        art.begin_frame();
        let _ = draw(&ctx, &artist, &mut art, now, Vec2::new(900.0, 800.0));
        assert_eq!(
            art.requests(now),
            vec![crate::message::Command::Fetch {
                url: portrait
                    .pick(want_px(plate_side(900.0), 2.0))
                    .cloned()
                    .expect("a pick"),
            }],
            "at 2x the same plate needs the 640"
        );
    }

    #[test]
    fn the_byline_counts_releases_and_tracks_under_the_name() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let now = Instant::now();
        let releases = vec![
            release("One", Some(2020), &["a", "b"], None),
            release("Two", None, &["c"], None),
        ];
        let artist = hero("Vela Moreno", None, &releases);
        let mut art = Art::default();
        let output = draw(&ctx, &artist, &mut art, now, PAGE);

        let words = discography_meta(2, 3);
        let byline = painted(&output, &words).expect("the byline is painted");
        let name = title(&output);
        assert_eq!(
            byline.fallback_color, tokens.palette.text_muted,
            "the byline is muted"
        );
        assert_close(rect_of(byline).top(), rect_of(name).bottom() + space::S2);
        assert_close(rect_of(byline).left(), rect_of(name).left());
        assert_close(
            rect_of(name).left(),
            space::GUTTER + plate_side(PAGE.x) + space::S5,
        );
        assert!(
            byline.galley.rows.len() == 1,
            "the byline is one plain line"
        );
    }

    #[test]
    fn an_artist_with_no_releases_has_no_byline_and_no_shelf() {
        let ctx = context();
        let now = Instant::now();
        let artist = hero("Vela Moreno", None, &[]);
        let mut art = Art::default();
        let output = draw(&ctx, &artist, &mut art, now, PAGE);

        assert_eq!(drawn_texts(&output), vec!["Vela Moreno"]);
        assert!(
            !output
                .shapes
                .iter()
                .any(|clipped| matches!(&clipped.shape, egui::epaint::Shape::Mesh(_))),
            "no shelf, no mask"
        );
        let band = band_rect(&output, PAGE.x);
        assert!(
            textured(&output, band).is_empty()
                && fills(&output, Tokens::DARK.palette.surface_raised).len() <= 1,
            "nothing stands at the band's edge"
        );
    }

    #[test]
    fn three_held_covers_stand_oldest_first_with_the_newest_flush_right() {
        let ctx = context();
        let now = Instant::now();
        let oldest = cover("oldest");
        let middle = cover("middle");
        let newest = cover("newest");
        let releases = vec![
            release("Newest", Some(2023), &[], Some(newest.clone())),
            release("Oldest", Some(2019), &[], Some(oldest.clone())),
            release("Middle", Some(2021), &[], Some(middle.clone())),
        ];
        let artist = hero("Vela Moreno", None, &releases);
        let mut art = Art::default();
        let side = plate_side(PAGE.x) * hero::SHELF_SCALE;
        let want = want_px(side, 1.0);
        for (set, rgba) in [&oldest, &middle, &newest].into_iter().zip([
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
        ]) {
            hold_colored(&ctx, &mut art, set, want, now, rgba);
        }
        let expected: Vec<egui::TextureId> = discography::shelf(&releases)
            .iter()
            .map(|set| match art.look(Some(*set), want, now) {
                crate::cover::Cover::Ready(texture) => texture,
                _ => panic!("the shelf's pick is held"),
            })
            .collect();
        let output = draw(&ctx, &artist, &mut art, now, PAGE);

        let band = band_rect(&output, PAGE.x);
        let floor = band.bottom() - stroke::HAIRLINE - space::S5;
        let shelf = Rect::from_min_max(
            pos2(band.right() - hero::SHELF_MAX, floor - side),
            pos2(band.right(), floor),
        );
        let covers = textured(&output, shelf);
        assert_eq!(covers.len(), 3, "three covers stand in the shelf");
        assert_eq!(
            textures(&output, shelf),
            expected,
            "the textures run left to right in shelf order"
        );
        for cover in &covers {
            assert_close(cover.width(), side);
            assert_close(cover.height(), side);
            assert_close(cover.bottom(), floor);
        }
        assert_close(covers[2].right(), band.right());
        for pair in covers.windows(2) {
            assert_close(pair[1].left() - pair[0].right(), hero::SHELF_GAP);
        }
        assert!(
            covers[0].left() < covers[1].left() && covers[1].left() < covers[2].left(),
            "oldest first: {covers:?}"
        );
    }

    #[test]
    fn a_loading_shelf_cover_draws_the_raised_slot_at_its_rect() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let now = Instant::now();
        let releases = vec![
            release("One", Some(2019), &[], Some(cover("one"))),
            release("Two", Some(2020), &[], Some(cover("two"))),
            release("Three", Some(2021), &[], Some(cover("three"))),
        ];
        let artist = hero("Vela Moreno", None, &releases);
        let mut art = Art::default();
        let output = draw(&ctx, &artist, &mut art, now, PAGE);

        let band = band_rect(&output, PAGE.x);
        let side = plate_side(PAGE.x) * hero::SHELF_SCALE;
        let floor = band.bottom() - stroke::HAIRLINE - space::S5;
        let slots: Vec<Rect> = fills(&output, tokens.palette.surface_raised)
            .into_iter()
            .filter(|rect| (rect.bottom() - floor).abs() < 0.01)
            .collect();
        assert_eq!(slots.len(), 3, "each loading cover is a raised slot");
        for slot in &slots {
            assert_close(slot.width(), side);
            assert_close(slot.height(), side);
        }
        assert_close(slots[2].right(), band.right());
    }

    #[test]
    fn the_mask_fades_the_left_tenth_of_the_shelf_from_the_bands_fill() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let now = Instant::now();
        let releases = vec![
            release("One", Some(2019), &[], Some(cover("one"))),
            release("Two", Some(2020), &[], Some(cover("two"))),
            release("Three", Some(2021), &[], Some(cover("three"))),
        ];
        let artist = hero("Vela Moreno", None, &releases);
        let mut art = Art::default();
        let output = draw(&ctx, &artist, &mut art, now, PAGE);

        let band = band_rect(&output, PAGE.x);
        let side = plate_side(PAGE.x) * hero::SHELF_SCALE;
        let floor = band.bottom() - stroke::HAIRLINE - space::S5;
        let left = band.right() - hero::SHELF_MAX;
        let top = floor - side;
        let shelf = Rect::from_min_max(pos2(left, top), pos2(band.right(), floor));
        let mesh_at = output
            .shapes
            .iter()
            .position(|clipped| matches!(&clipped.shape, egui::epaint::Shape::Mesh(_)))
            .expect("the shelf is masked");
        let mesh = match &output.shapes[mesh_at].shape {
            egui::epaint::Shape::Mesh(mesh) => mesh,
            _ => unreachable!("the mesh was found above"),
        };
        let covers: Vec<usize> = output
            .shapes
            .iter()
            .enumerate()
            .filter_map(|(index, clipped)| match &clipped.shape {
                egui::epaint::Shape::Rect(_) if clipped.clip_rect == shelf => Some(index),
                _ => None,
            })
            .collect();
        assert_eq!(covers.len(), 3, "three cover shapes stand under the mask");
        for index in covers {
            assert!(
                index < mesh_at,
                "the mask is painted after every cover shape"
            );
            assert_eq!(output.shapes[index].clip_rect, shelf);
        }
        assert_eq!(output.shapes[mesh_at].clip_rect, shelf);
        assert_eq!(mesh.vertices.len(), 4);
        assert_eq!(mesh.vertices[0].color, tokens.palette.hero_wash);
        assert_eq!(mesh.vertices[2].color, tokens.palette.hero_wash);
        assert_eq!(mesh.vertices[1].color, Color32::TRANSPARENT);
        assert_eq!(mesh.vertices[3].color, Color32::TRANSPARENT);
        assert_close(mesh.vertices[0].pos.x, left);
        assert_close(
            mesh.vertices[1].pos.x,
            left + hero::SHELF_MAX * hero::SHELF_FADE,
        );
        assert_close(mesh.vertices[0].pos.y, top);
        assert_close(mesh.vertices[2].pos.y, floor);
    }

    #[test]
    fn nothing_paints_outside_the_views_clip() {
        let ctx = context();
        let now = Instant::now();
        let releases = vec![
            release("One", Some(2019), &[], Some(cover("one"))),
            release("Two", Some(2020), &[], Some(cover("two"))),
            release("Three", Some(2021), &[], Some(cover("three"))),
        ];
        let name = "word ".repeat(24);
        let artist = hero(&name, None, &releases);
        let tokens = Tokens::DARK;
        let mut art = Art::default();
        let clip = Rect::from_min_size(Pos2::ZERO, vec2(500.0, 300.0));
        let mut output = ctx.run_ui(input(PAGE), |ui| {
            ui.set_clip_rect(clip);
            show(ui, &artist, &mut art, now, &tokens);
        });
        output.textures_delta.clear();
        for clipped in &output.shapes {
            assert!(
                clip.contains_rect(clipped.clip_rect),
                "a shape paints past the view's clip: {:?} outside {clip:?}",
                clipped.clip_rect
            );
        }
    }

    #[test]
    fn a_long_name_elides_after_three_rows_inside_the_column() {
        let ctx = context();
        let now = Instant::now();
        let name = "word ".repeat(24);
        let artist = hero(&name, None, &[]);
        let mut art = Art::default();
        let band_w = 400.0;
        let output = draw_in(&ctx, &artist, &mut art, now, PAGE, band_w);

        let drawn = title(&output);
        assert_eq!(drawn.galley.rows.len(), hero::ARTIST_TITLE_ROWS);
        assert!(
            drawn.galley.elided,
            "the name is cut at the third row: {:?}",
            drawn.galley.text()
        );
        let column = Rect::from_min_max(
            pos2(space::GUTTER + plate_side(PAGE.x) + space::S5, 0.0),
            pos2(band_w - space::GUTTER, PAGE.y),
        );
        assert!(
            rect_of(drawn).right() <= column.right() + 0.5,
            "the name ends at {} past the column {}",
            rect_of(drawn).right(),
            column.right()
        );
    }

    #[test]
    fn a_long_single_word_stays_inside_the_column() {
        let ctx = context();
        let now = Instant::now();
        let name = "W".repeat(120);
        let artist = hero(&name, None, &[]);
        let mut art = Art::default();
        let band_w = 680.0;
        let output = draw_in(&ctx, &artist, &mut art, now, PAGE, band_w);

        let column_width =
            band_w - space::GUTTER - (space::GUTTER + plate_side(PAGE.x) + space::S5);
        let drawn = title(&output);
        assert!(
            drawn.galley.rows.len() <= hero::ARTIST_TITLE_ROWS,
            "the name takes {} rows",
            drawn.galley.rows.len()
        );
        assert!(drawn.galley.elided, "the fourth row is elided");
        assert!(
            drawn.galley.size().x <= column_width + 0.5,
            "the name is {} wide past the column {column_width}",
            drawn.galley.size().x
        );
        // Egui breaks inside the unbroken word when it finds no word boundary;
        // the row cap then elides the fourth row.
    }

    #[test]
    fn a_name_past_the_row_cap_is_laid_at_the_whole_column() {
        let ctx = context();
        let now = Instant::now();
        let name = "word ".repeat(24);
        let artist = hero(&name, None, &[]);
        let mut art = Art::default();
        let band_w = 680.0;
        let output = draw_in(&ctx, &artist, &mut art, now, PAGE, band_w);

        let drawn = title(&output);
        assert_eq!(drawn.galley.rows.len(), hero::ARTIST_TITLE_ROWS);
        assert!(drawn.galley.elided, "the name is cut at the third row");
        let column = band_w - space::GUTTER - (space::GUTTER + plate_side(PAGE.x) + space::S5);
        assert_close(drawn.galley.job.wrap.max_width, column);
    }

    #[test]
    fn a_one_row_name_is_laid_at_the_whole_column() {
        let ctx = context();
        let now = Instant::now();
        let artist = hero("Vela", None, &[]);
        let mut art = Art::default();
        let output = draw(&ctx, &artist, &mut art, now, PAGE);

        let drawn = title(&output);
        assert_eq!(drawn.galley.rows.len(), 1);
        let column = PAGE.x - space::GUTTER - (space::GUTTER + plate_side(PAGE.x) + space::S5);
        assert_close(drawn.galley.job.wrap.max_width, column);
    }
}
