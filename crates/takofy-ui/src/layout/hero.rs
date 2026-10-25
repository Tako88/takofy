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
mod tests {
    use super::*;
    use crate::layout::WINDOW_MIN;
    use crate::theme::radius;

    const BOX: Rect = Rect {
        min: Pos2::new(0.0, 0.0),
        max: Pos2::new(100.0, 100.0),
    };

    #[test]
    fn the_quadrants_tile_the_box_without_gaps_or_overlap() {
        let quadrants = quadrants(BOX);
        for quadrant in quadrants {
            assert!(BOX.contains_rect(quadrant), "{quadrant:?} leaves the box");
        }
        let area: f32 = quadrants.iter().map(|quadrant| quadrant.area()).sum();
        assert!(
            (area - BOX.area()).abs() < 0.001,
            "the quadrants cover {area} of {}",
            BOX.area()
        );
        assert_eq!(quadrants[0].max.x, quadrants[1].min.x, "the top seam");
        assert_eq!(quadrants[2].max.x, quadrants[3].min.x, "the bottom seam");
        assert_eq!(quadrants[0].max.y, quadrants[2].min.y, "the left seam");
        assert_eq!(quadrants[1].max.y, quadrants[3].min.y, "the right seam");
    }

    #[test]
    fn the_quadrants_read_top_left_top_right_bottom_left_bottom_right() {
        let [top_left, top_right, bottom_left, bottom_right] = quadrants(BOX);
        assert_eq!(top_left.min, BOX.min);
        assert_eq!(top_right.min, Pos2::new(BOX.center().x, BOX.top()));
        assert_eq!(bottom_left.min, Pos2::new(BOX.left(), BOX.center().y));
        assert_eq!(bottom_right.max, BOX.max);
    }

    #[test]
    fn an_odd_box_splits_at_its_centre() {
        let odd = Rect {
            min: Pos2::new(0.0, 0.0),
            max: Pos2::new(5.0, 5.0),
        };
        let [top_left, top_right, bottom_left, bottom_right] = quadrants(odd);
        assert_eq!(top_left.max.x, 2.5);
        assert_eq!(top_left.max.y, 2.5);
        assert_eq!(top_right.min.x, 2.5);
        assert_eq!(bottom_left.min.y, 2.5);
        assert_eq!(bottom_right.min, Pos2::new(2.5, 2.5));
    }

    #[test]
    fn only_the_outside_corner_of_each_quadrant_is_rounded() {
        let corners = quadrant_corners(radius::LG);
        let each = radius::LG.nw;
        assert_eq!(
            corners,
            [
                CornerRadius {
                    nw: each,
                    ne: 0,
                    sw: 0,
                    se: 0
                },
                CornerRadius {
                    nw: 0,
                    ne: each,
                    sw: 0,
                    se: 0
                },
                CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: each,
                    se: 0
                },
                CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: 0,
                    se: each
                },
            ]
        );
    }

    fn assert_close(left: f32, right: f32) {
        assert!((left - right).abs() < 0.01, "{left} is not {right}");
    }

    #[test]
    fn the_floor_holds_below_the_first_crossing() {
        // 4.5% of the minimum window (640) is 28.8, under the 2 rem floor.
        assert_eq!(title_size(WINDOW_MIN[0]), hero::TITLE_MIN);
        assert_eq!(title_size(1.0), hero::TITLE_MIN);
    }

    #[test]
    fn the_cap_holds_above_the_second_crossing() {
        // 4.5% of 1920 is 86.4, over the 3.5 rem cap.
        assert_eq!(title_size(1920.0), text::XXXXL);
        assert_eq!(title_size(f32::MAX), text::XXXXL);
    }

    #[test]
    fn between_the_crossings_the_size_follows_the_viewport() {
        // 4.5% of 1000 is 45, between the floor and the cap.
        assert_close(title_size(1000.0), hero::TITLE_VW * 1000.0);
    }

    #[test]
    fn the_size_never_decreases_as_the_viewport_grows() {
        let mut previous = title_size(0.0);
        for width in (1..=4000).map(|width| width as f32) {
            let size = title_size(width);
            assert!(size >= previous, "{width}: {size} < {previous}");
            previous = size;
        }
    }

    #[test]
    fn the_artist_title_takes_the_phone_clamp_at_the_phone_tier() {
        assert_eq!(artist_title_size(200.0), hero::ARTIST_TITLE_PHONE_MIN);
        assert_close(
            artist_title_size(320.0),
            hero::ARTIST_TITLE_PHONE_VW * 320.0,
        );
        assert_eq!(artist_title_size(760.0), hero::ARTIST_TITLE_PHONE_MAX);
        // The phone clamp caps at 44 where the band clamp starts at 30.44.
        assert_close(artist_title_size(761.0), hero::ARTIST_TITLE_VW * 761.0);
        assert_eq!(artist_title_size(1920.0), hero::ARTIST_TITLE_MAX);
        assert_eq!(artist_title_size(f32::MAX), hero::ARTIST_TITLE_MAX);
    }

    #[test]
    fn the_artist_size_never_decreases_within_a_tier() {
        // The phone clamp, the narrow band clamp, and the wide clamp.
        for tier in [
            1..=TABLE_NARROW_MAX as u32,
            TABLE_NARROW_MAX as u32 + 1..=RAIL_MAX as u32,
            RAIL_MAX as u32 + 1..=4000,
        ] {
            let mut previous = 0.0;
            for width in tier.map(|width| width as f32) {
                let size = artist_title_size(width);
                assert!(size >= previous, "{width}: {size} < {previous}");
                previous = size;
            }
        }
    }

    #[test]
    fn the_sleeve_steps_down_with_the_tier() {
        assert_eq!(plate_side(1240.0), hero::ARTIST_PLATE);
        // The band keeps its two columns down to the phone tier, with a smaller
        // sleeve.
        assert_eq!(plate_side(901.0), hero::ARTIST_PLATE);
        assert_eq!(plate_side(900.0), hero::ARTIST_PLATE_NARROW);
        assert_eq!(plate_side(761.0), hero::ARTIST_PLATE_NARROW);
        assert_eq!(plate_side(760.0), hero::ARTIST_PLATE_PHONE);
    }

    #[test]
    fn a_zero_or_non_finite_width_takes_the_phone_sleeve() {
        for width in [0.0, -5.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(plate_side(width), hero::ARTIST_PLATE_PHONE, "{width}");
        }
    }

    const ORIGIN: Pos2 = Pos2::new(40.0, 100.0);

    fn framed(window_w: f32, band_w: f32, text_h: f32) -> ArtistRects {
        artist_frame(window_w).rects(ORIGIN, band_w, text_h)
    }

    #[test]
    fn the_sleeve_is_the_bands_own_left_edge() {
        let rects = framed(1240.0, 680.0, 100.0);
        let frame = artist_frame(1240.0);
        let body = rects.band.bottom() - stroke::HAIRLINE;
        assert_close(rects.plate.left(), rects.band.left());
        assert_close(rects.plate.top(), rects.band.top());
        assert_close(rects.plate.bottom(), body);
        assert_close(rects.plate.width(), frame.plate);
        assert_close(rects.plate.height(), frame.plate);
    }

    #[test]
    fn the_words_sit_beside_the_sleeve_to_the_far_gutter() {
        let rects = framed(1240.0, 680.0, 100.0);
        assert_close(rects.text.left(), rects.plate.right() + space::S5);
        assert_close(rects.text.right(), rects.band.right() - space::GUTTER);
        assert_close(
            rects.text.center().y,
            rects.band.center().y - stroke::HAIRLINE / 2.0,
        );
    }

    #[test]
    fn the_sleeve_holds_the_band_open_at_its_own_width() {
        let frame = artist_frame(1240.0);
        assert_close(
            framed(1240.0, 680.0, 100.0).band.height(),
            frame.plate + stroke::HAIRLINE,
        );
        assert_close(
            framed(900.0, 600.0, 100.0).band.height(),
            hero::ARTIST_PLATE_NARROW + stroke::HAIRLINE,
        );
    }

    #[test]
    fn the_words_grow_the_band_past_the_sleeve() {
        let frame = artist_frame(1240.0);
        let tall = frame.rects(ORIGIN, 680.0, frame.plate * 1.5);
        assert_close(tall.band.height(), frame.plate * 1.5 + stroke::HAIRLINE);
        assert_close(
            tall.plate.center().y,
            tall.band.center().y - stroke::HAIRLINE / 2.0,
        );
        assert_close(tall.plate.width(), frame.plate);
    }

    #[test]
    fn the_phone_tier_stacks_the_sleeve_over_the_full_width_column() {
        for window in [640.0f32, 760.0] {
            let frame = artist_frame(window);
            assert!(frame.stack, "{window} is the phone tier");

            let band_w = 480.0;
            let text_h = 80.0;
            let rects = frame.rects(ORIGIN, band_w, text_h);
            assert_close(rects.plate.left(), rects.band.left() + space::S4);
            assert_close(rects.plate.top(), rects.band.top() + space::S4);
            assert_close(rects.text.left(), rects.plate.left());
            assert_close(rects.text.right(), rects.band.right() - space::S4);
            assert_close(rects.text.top(), rects.plate.bottom() + space::S5);
            assert_close(
                rects.band.height(),
                space::S4 + frame.plate + space::S5 + text_h + space::S4 + stroke::HAIRLINE,
            );
        }
    }

    #[test]
    fn the_words_column_never_goes_negative() {
        for band_w in [0.0f32, 100.0, 200.0, 400.0, 640.0] {
            for window in [640.0f32, 900.0, 1240.0] {
                let frame = artist_frame(window);
                assert!(frame.text_width(band_w) >= 0.0, "at {band_w}");
                let rects = frame.rects(ORIGIN, band_w, 60.0);
                assert!(rects.text.width() >= 0.0, "at {band_w}");
            }
        }
    }

    #[test]
    fn balanced_width_keeps_the_row_count() {
        // A word that fills `width / 10` characters per row.
        let rows = |width: f32| ((1000.0 / width.max(1.0)).ceil() as usize).max(1);
        let width = balanced_width(400.0, rows, 3);
        assert_eq!(rows(width), 3, "{width} balances to three rows");
        assert!(width <= 400.0);
        // The narrowest width that still holds three rows is 1000/3, within
        // the resolution of eleven halvings over the column.
        assert!((width - 1000.0 / 3.0).abs() < 0.25, "{width}");
    }

    #[test]
    fn balanced_width_leaves_a_single_row_at_the_column() {
        assert_eq!(balanced_width(400.0, |_| 1, 3), 400.0);
    }

    #[test]
    fn balanced_width_leaves_a_word_past_the_cap_at_the_column() {
        assert_eq!(balanced_width(400.0, |_| 5, 3), 400.0);
    }

    #[test]
    fn balanced_width_never_exceeds_the_column() {
        let rows = |width: f32| ((200.0 / width.max(1.0)).ceil() as usize).max(1);
        for max_w in [0.0f32, 1.0, 40.0, 133.0, 400.0] {
            let width = balanced_width(max_w, rows, 3);
            assert!(width <= max_w, "{width} past {max_w}");
            assert!(width >= 0.0);
        }
    }

    #[test]
    fn a_zero_or_non_finite_artist_width_returns_the_floor() {
        for width in [0.0, -5.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(artist_title_size(width), hero::ARTIST_TITLE_MIN, "{width}");
        }
    }

    #[test]
    fn zero_and_non_finite_widths_return_the_floor() {
        for width in [0.0, -5.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(title_size(width), hero::TITLE_MIN, "{width}");
        }
    }
}
