//! The artist page's band: the sleeve, the name and the byline.
//!
//! The band is a solid wash sampled from the portrait. The portrait is the
//! band's own left edge — flush to its top and bottom rule, a hairline down its
//! right — and the name sits centred beside it over a byline. At the phone tier
//! the sleeve comes back inside the gutters and the type stacks under it.
//!
//! The band is all this view draws. Its action row is the shared one, which is
//! the only way the row can pin while the discography scrolls under it; the page
//! owns the presses that row reports.

use std::sync::Arc;
use std::time::Instant;

use egui::emath::GuiRounding;
use egui::{FontId, Painter, Rect, Sense, Ui, pos2};
use takofy_model::{CoverSet, Release};

use crate::cover::{Art, Cover, want_px};
use crate::format::discography_meta;
use crate::layout::hero::{artist_frame, balanced_width};
use crate::theme::{Tokens, Weight, fonts, hero, radius, space, stroke, tracking};

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
    let frame = artist_frame(ui.ctx().content_rect().width());
    let pixels_per_point = ui.pixels_per_point();

    let want = want_px(frame.plate, pixels_per_point);
    let plate = art.look(artist.portrait, want, now);
    let tint = art.tint_held(artist.portrait, want, &tokens.palette.tint);

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
    sleeve(ui.painter(), rect, rects.plate, frame.stack, plate, tokens);

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

/// The sleeve: the portrait standing on the band's left edge, or the small plate
/// the stacked tier sets inside its padding.
///
/// Flush, the sleeve carries a hairline down its right side alone — the other
/// three sides are the band's own edges, and one there would double the rule. Its
/// art also runs a hairline past its open edges, under the band's bottom rule,
/// because a fill rounded to the pixel grid can otherwise leave a sliver of the
/// wash between the picture and the edge it is flush to.
fn sleeve(painter: &Painter, band: Rect, rect: Rect, stacked: bool, cover: Cover, tokens: &Tokens) {
    if stacked {
        art::tile(
            painter,
            rect,
            radius::SM,
            tokens.palette.border_strong,
            cover,
            tokens,
        );
        return;
    }
    let paint = painter.with_clip_rect(band);
    let bleed = Rect::from_min_max(
        pos2(rect.left(), rect.top() - stroke::HAIRLINE),
        pos2(rect.right(), rect.bottom() + stroke::HAIRLINE),
    );
    art::fill_rounded(&paint, bleed, radius::NONE, cover, tokens);
    paint.rect_filled(
        Rect::from_min_max(
            pos2(rect.left(), band.bottom() - stroke::HAIRLINE),
            pos2(rect.right(), band.bottom()),
        ),
        radius::NONE,
        tokens.palette.border,
    );
    let edge = Rect::from_min_max(
        pos2(rect.right() - stroke::HAIRLINE, rect.top()),
        pos2(rect.right(), band.bottom() - stroke::HAIRLINE),
    )
    .round_to_pixels(painter.pixels_per_point());
    paint.rect_filled(edge, radius::NONE, tokens.palette.border_strong);
}

#[cfg(test)]
mod tests;
