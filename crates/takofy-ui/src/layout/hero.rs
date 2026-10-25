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
//! is a plate, a title over a byline, and a shelf of release covers, all solved
//! here for the same reason.

use egui::{CornerRadius, Pos2, Rect, Vec2, pos2, vec2};

use super::{RAIL_MAX, TABLE_NARROW_MAX};
use crate::theme::{hero, space, text};

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
/// shelf.
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

/// The artist band's square plate for a viewport `viewport_w` points wide.
///
/// `clamp(8.5rem, 15vw, 12.5rem)` in the wide tier and
/// `clamp(6.5rem, 18vw, 9rem)` in the narrow one. A width the solver cannot
/// answer — zero, negative, non-finite — takes the narrow tier's floor.
pub fn plate_side(viewport_w: f32) -> f32 {
    if viewport_w.is_finite() && viewport_w > 0.0 && !narrow(viewport_w) {
        return (hero::ARTIST_PLATE_VW * viewport_w)
            .clamp(hero::ARTIST_PLATE_MIN, hero::ARTIST_PLATE_MAX);
    }
    let width = if viewport_w.is_finite() && viewport_w > 0.0 {
        viewport_w
    } else {
        0.0
    };
    (hero::ARTIST_PLATE_NARROW_VW * width)
        .clamp(hero::ARTIST_PLATE_NARROW_MIN, hero::ARTIST_PLATE_NARROW_MAX)
}

/// The indices the shelf takes from `n_covered` covered, unduplicated releases.
///
/// The oldest and the newest with one in the middle, all of them when there are
/// no more than a shelf's worth. The input is in the shelf's own order.
pub fn shelf_picks(n_covered: usize) -> Vec<usize> {
    if n_covered == 0 {
        return Vec::new();
    }
    if n_covered <= hero::SHELF_COVERS {
        return (0..n_covered).collect();
    }
    vec![0, (n_covered - 1) / 2, n_covered - 1]
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

/// The artist band's shelf: the covers, their square side, and the box they are
/// right-aligned in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShelfBox {
    pub covers: usize,
    pub side: f32,
    pub width: f32,
}

/// The artist band's fixed shape for a window, before its words are measured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArtistFrame {
    pub plate: f32,
    /// The tier's height floor; the band grows past it when its words demand it.
    pub min_h: f32,
    pub title_size: f32,
    pub shelf: Option<ShelfBox>,
}

impl ArtistFrame {
    /// The width the name and the byline are laid out in: from the plate's far
    /// edge to the shelf's, or to the band's far gutter when no shelf stands.
    pub fn text_width(&self, band_w: f32) -> f32 {
        let right = match self.shelf {
            Some(shelf) => band_w - shelf.width - space::S5,
            None => band_w - space::GUTTER,
        };
        (right - (space::GUTTER + self.plate + space::S5)).max(0.0)
    }

    /// The band's boxes on the same foot line, for a solved column `text_h` tall.
    pub fn rects(&self, origin: Pos2, band_w: f32, text_h: f32) -> ArtistRects {
        let height = (space::S5 + self.plate.max(text_h) + space::S5).max(self.min_h);
        let band = Rect::from_min_size(origin, vec2(band_w, height));
        let floor = band.bottom() - space::S5;
        let plate = Rect::from_min_size(
            pos2(band.left() + space::GUTTER, floor - self.plate),
            Vec2::splat(self.plate),
        );
        let text_left = plate.right() + space::S5;
        let text = Rect::from_min_max(
            pos2(text_left, floor - text_h),
            pos2(text_left + self.text_width(band_w), floor),
        );

        let Some(shelf) = self.shelf else {
            return ArtistRects {
                band,
                plate,
                text,
                shelf: None,
                fade: None,
                covers: Vec::new(),
            };
        };

        let box_ = Rect::from_min_max(
            pos2(band.right() - shelf.width, floor - shelf.side),
            pos2(band.right(), floor),
        );
        let mut covers = Vec::with_capacity(shelf.covers);
        let mut right = box_.right();
        for _ in 0..shelf.covers {
            covers.push(Rect::from_min_max(
                pos2(right - shelf.side, box_.top()),
                pos2(right, box_.bottom()),
            ));
            right -= shelf.side + hero::SHELF_GAP;
        }
        covers.reverse();
        let fade = Rect::from_min_max(
            box_.min,
            pos2(box_.left() + hero::SHELF_FADE * box_.width(), box_.bottom()),
        );
        ArtistRects {
            band,
            plate,
            text,
            shelf: Some(box_),
            fade: Some(fade),
            covers,
        }
    }
}

/// The artist band's boxes: the band, the plate, the words' column, the shelf,
/// its left mask and the covers inside it, oldest first.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtistRects {
    pub band: Rect,
    pub plate: Rect,
    pub text: Rect,
    pub shelf: Option<Rect>,
    pub fade: Option<Rect>,
    pub covers: Vec<Rect>,
}

/// The artist band's shape for a window and a number of shelf covers.
///
/// The plate and the title follow the window; the shelf is dropped at the phone
/// tier and kept at its natural width up to its tier's cap.
pub fn artist_frame(window_w: f32, covers: usize) -> ArtistFrame {
    let narrow = narrow(window_w);
    let plate = plate_side(window_w);
    ArtistFrame {
        plate,
        min_h: if narrow {
            hero::ARTIST_MIN_H_NARROW
        } else {
            hero::ARTIST_MIN_H
        },
        title_size: artist_title_size(window_w),
        shelf: shelf_box(window_w, narrow, plate, covers),
    }
}

fn shelf_box(window_w: f32, narrow: bool, plate: f32, covers: usize) -> Option<ShelfBox> {
    if covers == 0 || !window_w.is_finite() || window_w <= TABLE_NARROW_MAX {
        return None;
    }
    let side = plate * hero::SHELF_SCALE;
    let natural = covers as f32 * side + (covers - 1) as f32 * hero::SHELF_GAP;
    let max = if narrow {
        (hero::SHELF_NARROW_VW * window_w).min(hero::SHELF_NARROW_MAX)
    } else {
        (hero::SHELF_VW * window_w).min(hero::SHELF_MAX)
    };
    Some(ShelfBox {
        covers,
        side,
        width: natural.min(max),
    })
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
    fn the_artist_plate_follows_the_window_within_each_tier() {
        // 18% of 640 is 115.2, between the narrow floor and cap.
        assert_close(plate_side(640.0), hero::ARTIST_PLATE_NARROW_VW * 640.0);
        // 18% of 900 is 162, past the narrow cap.
        assert_eq!(plate_side(900.0), hero::ARTIST_PLATE_NARROW_MAX);
        // 15% of 901 is 135.15, under the wide floor.
        assert_close(plate_side(901.0), hero::ARTIST_PLATE_MIN);
        assert_close(plate_side(1240.0), hero::ARTIST_PLATE_VW * 1240.0);
        // 15% of 1920 is 288, past the wide cap.
        assert_eq!(plate_side(1920.0), hero::ARTIST_PLATE_MAX);
    }

    #[test]
    fn the_artist_plate_never_decreases_within_a_tier() {
        for tier in [1..=RAIL_MAX as u32, RAIL_MAX as u32 + 1..=4000] {
            let mut previous = 0.0;
            for width in tier.map(|width| width as f32) {
                let side = plate_side(width);
                assert!(side >= previous, "{width}: {side} < {previous}");
                previous = side;
            }
        }
    }

    #[test]
    fn a_zero_or_non_finite_width_takes_the_narrow_plate_floor() {
        for width in [0.0, -5.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(plate_side(width), hero::ARTIST_PLATE_NARROW_MIN, "{width}");
        }
    }

    #[test]
    fn the_shelf_takes_the_oldest_the_middle_and_the_newest() {
        assert_eq!(shelf_picks(0), Vec::<usize>::new());
        assert_eq!(shelf_picks(1), vec![0]);
        assert_eq!(shelf_picks(2), vec![0, 1]);
        assert_eq!(shelf_picks(3), vec![0, 1, 2]);
        assert_eq!(shelf_picks(4), vec![0, 1, 3]);
        assert_eq!(shelf_picks(5), vec![0, 2, 4]);
        assert_eq!(shelf_picks(9), vec![0, 4, 8]);
    }

    #[test]
    fn the_shelf_is_dropped_at_the_phone_tier_and_without_covers() {
        assert!(
            artist_frame(760.0, 3).shelf.is_none(),
            "a phone has no shelf"
        );
        assert!(artist_frame(761.0, 3).shelf.is_some(), "one point above");
        assert!(
            artist_frame(1240.0, 0).shelf.is_none(),
            "no covers, no shelf"
        );
    }

    #[test]
    fn the_shelf_is_as_wide_as_its_covers_up_to_the_tier_cap() {
        for window in [800.0f32, 900.0, 901.0, 1240.0] {
            let frame = artist_frame(window, 1);
            let side = frame.plate * hero::SHELF_SCALE;
            let max = if window <= RAIL_MAX {
                (hero::SHELF_NARROW_VW * window).min(hero::SHELF_NARROW_MAX)
            } else {
                (hero::SHELF_VW * window).min(hero::SHELF_MAX)
            };
            for covers in 1..=hero::SHELF_COVERS {
                let shelf = artist_frame(window, covers).shelf.expect("a shelf");
                let natural = covers as f32 * side + (covers - 1) as f32 * hero::SHELF_GAP;
                assert_close(shelf.width, natural.min(max));
            }
        }
    }

    const ORIGIN: Pos2 = Pos2::new(40.0, 100.0);

    fn framed(window_w: f32, band_w: f32, covers: usize, text_h: f32) -> ArtistRects {
        artist_frame(window_w, covers).rects(ORIGIN, band_w, text_h)
    }

    #[test]
    fn the_plate_and_the_covers_stand_on_the_bands_floor() {
        let rects = framed(1240.0, 680.0, 3, 100.0);
        let floor = rects.band.bottom() - space::S5;
        assert_close(rects.plate.bottom(), floor);
        assert_close(rects.shelf.expect("a shelf").bottom(), floor);
        for cover in &rects.covers {
            assert_close(cover.bottom(), floor);
        }
    }

    #[test]
    fn the_newest_cover_is_flush_with_the_bands_edge() {
        let rects = framed(1240.0, 680.0, 3, 100.0);
        assert_close(
            rects.covers.last().expect("a cover").right(),
            rects.band.right(),
        );
    }

    #[test]
    fn the_covers_are_square_half_the_plate_and_a_point_apart() {
        let rects = framed(1240.0, 680.0, 3, 100.0);
        assert_eq!(rects.covers.len(), hero::SHELF_COVERS);
        let side = rects.plate.width() * hero::SHELF_SCALE;
        for cover in &rects.covers {
            assert_close(cover.width(), side);
            assert_close(cover.height(), side);
        }
        for pair in rects.covers.windows(2) {
            assert_close(pair[1].left() - pair[0].right(), hero::SHELF_GAP);
        }
    }

    #[test]
    fn the_mask_spans_the_left_tenth_of_the_shelf() {
        let rects = framed(1240.0, 680.0, 3, 100.0);
        let shelf = rects.shelf.expect("a shelf");
        let fade = rects.fade.expect("a mask");
        assert_close(fade.left(), shelf.left());
        assert_close(fade.width(), shelf.width() * hero::SHELF_FADE);
        assert_close(fade.top(), shelf.top());
        assert_close(fade.bottom(), shelf.bottom());
    }

    #[test]
    fn the_words_end_at_the_shelf_or_the_far_gutter() {
        let band_w = 680.0;
        let with = framed(1240.0, band_w, 3, 100.0);
        assert_close(
            with.text.right(),
            with.shelf.expect("a shelf").left() - space::S5,
        );
        let without = framed(1240.0, band_w, 0, 100.0);
        assert_close(without.text.right(), ORIGIN.x + band_w - space::GUTTER);
    }

    #[test]
    fn the_words_column_never_goes_negative() {
        for band_w in [0.0f32, 100.0, 200.0, 400.0, 640.0] {
            for covers in [0, hero::SHELF_COVERS] {
                let frame = artist_frame(640.0, covers);
                assert!(frame.text_width(band_w) >= 0.0, "at {band_w}");
                let rects = frame.rects(ORIGIN, band_w, 60.0);
                assert!(rects.text.width() >= 0.0, "at {band_w}");
            }
        }
    }

    #[test]
    fn the_band_keeps_its_floor_until_the_words_outgrow_it() {
        let frame = artist_frame(1240.0, 3);
        assert_close(
            frame.rects(ORIGIN, 680.0, 100.0).band.height(),
            hero::ARTIST_MIN_H,
        );
        let grown = frame.rects(ORIGIN, 680.0, hero::ARTIST_MIN_H);
        assert_close(grown.band.height(), 2.0 * space::S5 + hero::ARTIST_MIN_H);
        let narrow = artist_frame(900.0, 3);
        assert_close(
            narrow.rects(ORIGIN, 600.0, 100.0).band.height(),
            hero::ARTIST_MIN_H_NARROW,
        );
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
