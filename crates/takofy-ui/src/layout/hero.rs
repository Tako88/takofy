//! The hero bars' arithmetic.
//!
//! The reference sizes the hero title as `clamp(2rem, 4.5vw, 3.5rem)`: a share of
//! the viewport width, floored so it stays legible at a narrow window and capped
//! so it does not outgrow the band. `4.5vw` is a share of the viewport, not of the
//! band the title is drawn in, so the argument here is the window width rather
//! than the hero's rect.
//!
//! Every constant is a step-3 token. At the 16-point rem base the floor is
//! `hero::TITLE_MIN` (2 rem) and the cap is `text::XXXXL` (3.5 rem).
//!
//! A mosaic band's four covers are quartered here too, so the painter only
//! places rects and the arithmetic is testable without a window. The artist band
//! is a sleeve, a title over a byline, all solved here for the same reason.

use egui::{CornerRadius, Pos2, Rect, Vec2, pos2, vec2};

use super::{RAIL_MAX, TABLE_NARROW_MAX};
use crate::theme::{hero, space, stroke, text};

/// The hero title's size for a viewport `viewport_w` points wide.
///
/// The width can arrive as `0.0`, negative or non-finite — the window can be
/// resized to nothing and the solver's output flows straight in — so anything
/// that is not a positive finite width returns the floor rather than letting a
/// `NaN` product escape the clamp.
pub fn title_size(viewport_w: f32) -> f32 {
    if !viewport_w.is_finite() || viewport_w <= 0.0 {
        return hero::TITLE_MIN;
    }
    (hero::TITLE_VW * viewport_w).clamp(hero::TITLE_MIN, text::XXXXL)
}

/// The artist hero's title size for a viewport `viewport_w` points wide.
///
/// The reference's artist clamp: `1.75rem` floor, `4vw`, `3.25rem` cap, and its
/// phone clamp `1.75rem`, `10vw`, `2.75rem` at the phone tier. The name is sized
/// to break over two or three rows inside the band rather than to run past the
/// far gutter.
pub fn artist_title_size(viewport_w: f32) -> f32 {
    if !viewport_w.is_finite() || viewport_w <= 0.0 {
        return hero::ARTIST_TITLE_MIN;
    }
    if viewport_w <= TABLE_NARROW_MAX {
        (hero::ARTIST_TITLE_PHONE_VW * viewport_w)
            .clamp(hero::ARTIST_TITLE_PHONE_MIN, hero::ARTIST_TITLE_PHONE_MAX)
    } else {
        (hero::ARTIST_TITLE_VW * viewport_w).clamp(hero::ARTIST_TITLE_MIN, hero::ARTIST_TITLE_MAX)
    }
}

/// Whether `window_w` is in the artist band's narrow tier.
fn narrow(window_w: f32) -> bool {
    !window_w.is_finite() || window_w <= RAIL_MAX
}

/// Whether `window_w` is in the artist band's stacked phone tier.
fn phone(window_w: f32) -> bool {
    !window_w.is_finite() || window_w <= TABLE_NARROW_MAX
}

/// The artist band's sleeve for a viewport `viewport_w` points wide.
///
/// The reference's `--hero-plate`: 16 rem wide, 13 rem at the narrow tier and
/// 6.5 rem once the band stacks. A width the solver cannot answer takes the
/// stacked tier's sleeve.
pub fn plate_side(viewport_w: f32) -> f32 {
    if phone(viewport_w) {
        hero::ARTIST_PLATE_PHONE
    } else if narrow(viewport_w) {
        hero::ARTIST_PLATE_NARROW
    } else {
        hero::ARTIST_PLATE
    }
}

/// The narrowest width at or under `max_w` that keeps `rows_at(max_w)` rows.
///
/// `rows_at` answers how many rows a word takes at a width. A word that fits on
/// one row, or that is already past `cap` rows, is left at `max_w`: there is
/// nothing to balance. A binary search over the width keeps the row count and
/// spends a bounded number of layouts per frame.
pub fn balanced_width(max_w: f32, rows_at: impl Fn(f32) -> usize, cap: usize) -> f32 {
    if !max_w.is_finite() || max_w <= 0.0 {
        return 0.0;
    }
    let rows = rows_at(max_w);
    if rows <= 1 || rows > cap {
        return max_w;
    }
    // Eleven halvings resolve a band-wide column to about a tenth of a point.
    let (mut narrow, mut wide) = (0.0, max_w);
    for _ in 0..11 {
        let mid = (narrow + wide) / 2.0;
        if rows_at(mid) > rows {
            narrow = mid;
        } else {
            wide = mid;
        }
    }
    wide
}

/// The artist band's fixed shape for a window, before its words are measured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArtistFrame {
    pub plate: f32,
    pub title_size: f32,
    /// Whether the phone tier stacks the sleeve above a full-width column.
    pub stack: bool,
}

impl ArtistFrame {
    /// The width the name and the byline are laid out in: the band less its two
    /// gutters when stacked, otherwise from the sleeve's far edge to the far
    /// gutter.
    pub fn text_width(&self, band_w: f32) -> f32 {
        if self.stack {
            return (band_w - 2.0 * space::S4).max(0.0);
        }
        (band_w - space::GUTTER - self.plate - space::S5).max(0.0)
    }

    /// The band's height for a solved column `text_h` tall: the sleeve holds it
    /// open, and the words grow it past the sleeve when they need the room.
    pub fn height(&self, text_h: f32) -> f32 {
        let content = if self.stack {
            space::S4 + self.plate + space::S5 + text_h + space::S4
        } else {
            self.plate.max(text_h)
        };
        content + stroke::HAIRLINE
    }

    /// The band's boxes for a solved column `text_h` tall.
    ///
    /// The rule along the band's bottom edge is painted inside it, so the body
    /// the sleeve and the words sit in ends one hairline above it.
    pub fn rects(&self, origin: Pos2, band_w: f32, text_h: f32) -> ArtistRects {
        let band = Rect::from_min_size(origin, vec2(band_w, self.height(text_h)));
        let body = Rect::from_min_max(
            band.min,
            pos2(band.right(), band.bottom() - stroke::HAIRLINE),
        );
        if self.stack {
            let plate = Rect::from_min_size(
                pos2(body.left() + space::S4, body.top() + space::S4),
                Vec2::splat(self.plate),
            );
            let text = Rect::from_min_size(
                pos2(plate.left(), plate.bottom() + space::S5),
                vec2(self.text_width(band_w), text_h),
            );
            return ArtistRects { band, plate, text };
        }
        let plate = Rect::from_min_size(
            pos2(body.left(), body.center().y - self.plate / 2.0),
            Vec2::splat(self.plate),
        );
        let text = Rect::from_min_size(
            pos2(plate.right() + space::S5, body.center().y - text_h / 2.0),
            vec2(self.text_width(band_w), text_h),
        );
        ArtistRects { band, plate, text }
    }
}

/// The artist band's boxes: the band, the sleeve and the words' column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArtistRects {
    pub band: Rect,
    pub plate: Rect,
    pub text: Rect,
}

/// The artist band's shape for a window.
///
/// The sleeve and the title follow the window; the sleeve steps down with it.
pub fn artist_frame(window_w: f32) -> ArtistFrame {
    ArtistFrame {
        plate: plate_side(window_w),
        title_size: artist_title_size(window_w),
        stack: phone(window_w),
    }
}

/// The four quadrants of a mosaic box, in reading order: top left, top right,
/// bottom left, bottom right.
///
/// The split is the box's centre, so an odd width halves evenly rather than
/// leaving a one-pixel seam between two neighbours.
pub fn quadrants(rect: Rect) -> [Rect; 4] {
    let mid = rect.center();
    [
        Rect::from_min_max(rect.min, mid),
        Rect::from_min_max(Pos2::new(mid.x, rect.top()), Pos2::new(rect.right(), mid.y)),
        Rect::from_min_max(
            Pos2::new(rect.left(), mid.y),
            Pos2::new(mid.x, rect.bottom()),
        ),
        Rect::from_min_max(mid, rect.max),
    ]
}

/// Each quadrant's corners with only its outside one rounded.
///
/// The four outside corners are the box's own, so the inner edges stay square and
/// the quadrants join without a notch.
pub fn quadrant_corners(radius: CornerRadius) -> [CornerRadius; 4] {
    [
        CornerRadius {
            nw: radius.nw,
            ne: 0,
            sw: 0,
            se: 0,
        },
        CornerRadius {
            nw: 0,
            ne: radius.ne,
            sw: 0,
            se: 0,
        },
        CornerRadius {
            nw: 0,
            ne: 0,
            sw: radius.sw,
            se: 0,
        },
        CornerRadius {
            nw: 0,
            ne: 0,
            sw: 0,
            se: radius.se,
        },
    ]
}

#[cfg(test)]
mod tests;
