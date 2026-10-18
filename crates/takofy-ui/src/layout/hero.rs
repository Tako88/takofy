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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::WINDOW_MIN;

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
    fn zero_and_non_finite_widths_return_the_floor() {
        for width in [0.0, -5.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(title_size(width), hero::TITLE_MIN, "{width}");
        }
    }
}
