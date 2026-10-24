//! The hero band's title arithmetic.
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
//! places rects and the arithmetic is testable without a window.

use egui::{CornerRadius, Pos2, Rect};

use crate::theme::{hero, text};

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
/// The same clamp with the reference's artist numbers: `2.25rem` floor,
/// `5.5vw`, `4.5rem` cap. Every number differs from the playlist band's, so the
/// artist's name is the largest type the design has.
pub fn artist_title_size(viewport_w: f32) -> f32 {
    if !viewport_w.is_finite() || viewport_w <= 0.0 {
        return hero::ARTIST_TITLE_MIN;
    }
    (hero::ARTIST_TITLE_VW * viewport_w).clamp(hero::ARTIST_TITLE_MIN, hero::ARTIST_TITLE_MAX)
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
    fn the_artist_title_clamp_is_wider_than_the_playlists() {
        assert_eq!(artist_title_size(WINDOW_MIN[0]), hero::ARTIST_TITLE_MIN);
        assert_eq!(artist_title_size(1.0), hero::ARTIST_TITLE_MIN);
        assert_eq!(artist_title_size(1920.0), hero::ARTIST_TITLE_MAX);
        assert_eq!(artist_title_size(f32::MAX), hero::ARTIST_TITLE_MAX);
        // 5.5% of 1000 is 55, between the floor and the cap.
        assert_close(artist_title_size(1000.0), 55.0);
        assert!(artist_title_size(1000.0) > title_size(1000.0));
    }

    #[test]
    fn the_artist_size_never_decreases_as_the_viewport_grows() {
        let mut previous = artist_title_size(0.0);
        for width in (1..=4000).map(|width| width as f32) {
            let size = artist_title_size(width);
            assert!(size >= previous, "{width}: {size} < {previous}");
            previous = size;
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
