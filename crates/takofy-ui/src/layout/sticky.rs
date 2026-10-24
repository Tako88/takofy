//! A band that holds its place under the bar and lets go at the end of the list
//! it filters.
//!
//! The reference's sticky row is `position: sticky; top: var(--actionbar-h)`
//! inside its own block: it holds while the list under it is still passing, and
//! once the block's end arrives the row travels with the end instead of banding
//! over the section below. That is three states, not two, and the difference
//! between "at the floor" and "riding the end" decides both the clip the body is
//! drawn through and whether the row may speak for the viewport's top in the
//! reveal seam.
//!
//! The geometry is pure so the clamp can be tested without a scroll area: the
//! page hands in where the row sits in its flow, the floor it may not rise above
//! and the bottom of the block it filters.

use egui::{Pos2, Rect};

/// Where a band stands this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hold {
    /// The band is in its own slot, above the floor: nothing passes under it.
    Resting,
    /// The band sits at the floor, with the list sliding beneath it.
    Holding,
    /// The band's block has ended: the end carries the band upward and out.
    Released,
}

/// A band's rect and how it stands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stuck {
    /// Where the band is drawn: its own slot, the floor, or the end's edge.
    pub rect: Rect,
    /// What the band is doing there.
    pub hold: Hold,
}

/// Where a band sits between its slot, the floor and the end of its block.
///
/// `floor` is the top of the strip the band may not rise above, which is the
/// bar's own bottom once the bar holds. `end` is the bottom of the block the
/// band filters: the band cannot pass it, so the last part of the block carries
/// the band up instead of the band banding over whatever follows.
pub fn stick(flow: Rect, floor: f32, end: f32) -> Stuck {
    // The band cannot pass the end of the block it filters: past it the end
    // carries the band upward instead of the band covering what follows.
    let top = floor.min(end - flow.height());
    if top <= flow.top() {
        return Stuck {
            rect: flow,
            hold: Hold::Resting,
        };
    }
    Stuck {
        rect: Rect::from_min_size(Pos2::new(flow.left(), top), flow.size()),
        // `top` is the floor itself whenever the clamp did not bite, so this is
        // exact rather than a tolerance on arithmetic.
        hold: if top == floor {
            Hold::Holding
        } else {
            Hold::Released
        },
    }
}

/// The clip the block under the band is drawn through.
///
/// A resting band takes no strip from the body. A held one owns the viewport's
/// top down to its own bottom, so the rows sliding under it stop there. A
/// released one sits *above* the rows drawn after it — the body is painted after
/// the band — so the body stops at the band's top edge instead. Nothing here may
/// grow the clip: the bar's own shrink and the pane's edges are the caller's.
pub fn body_clip(clip: Rect, band: Rect, hold: Hold) -> Rect {
    // Both edges are clamped into the clip: a band that has left the viewport
    // leaves an empty body rather than an inverted rect.
    match hold {
        Hold::Resting => clip,
        Hold::Holding => {
            let top = band.bottom().clamp(clip.min.y, clip.max.y);
            Rect::from_min_max(Pos2::new(clip.min.x, top), clip.max)
        }
        Hold::Released => {
            let bottom = band.top().clamp(clip.min.y, clip.max.y);
            Rect::from_min_max(clip.min, Pos2::new(clip.max.x, bottom))
        }
    }
}

/// The height a stack of `heights` takes, with `gap` between them and none after
/// the last.
///
/// The page solves a whole discography's height before drawing it, so the band's
/// end is exact rather than remembered from the last frame.
pub fn stack_height(heights: &[f32], gap: f32) -> f32 {
    let stack: f32 = heights.iter().sum();
    let gaps = heights.len().saturating_sub(1) as f32;
    stack + gaps * gap
}

#[cfg(test)]
mod tests {
    use egui::{Rect, pos2, vec2};

    use super::*;

    /// A row that is `H` tall at `left`, `top` wide enough to be a band.
    const H: f32 = 40.0;

    fn flow(top: f32) -> Rect {
        Rect::from_min_size(pos2(24.0, top), vec2(600.0, H))
    }

    fn clip() -> Rect {
        Rect::from_min_max(pos2(0.0, 0.0), pos2(900.0, 300.0))
    }

    fn band(top: f32) -> Rect {
        Rect::from_min_size(pos2(0.0, top), vec2(900.0, H))
    }

    #[test]
    fn a_band_below_the_floor_rests() {
        let stuck = stick(flow(400.0), 60.0, 900.0);
        assert_eq!(stuck.hold, Hold::Resting);
        assert_eq!(stuck.rect, flow(400.0), "the band keeps its slot");
    }

    #[test]
    fn a_band_exactly_at_the_floor_rests() {
        let stuck = stick(flow(60.0), 60.0, 900.0);
        assert_eq!(stuck.hold, Hold::Resting);
        assert_eq!(stuck.rect, flow(60.0));
    }

    #[test]
    fn a_band_above_the_floor_holds_at_it() {
        let stuck = stick(flow(-20.0), 60.0, 900.0);
        assert_eq!(stuck.hold, Hold::Holding);
        assert_eq!(stuck.rect.top(), 60.0, "the band stops at the floor");
        assert_eq!(stuck.rect.left(), 24.0, "and keeps its column");
        assert_eq!(stuck.rect.size(), vec2(600.0, H));
    }

    #[test]
    fn it_holds_up_to_the_exact_end() {
        let stuck = stick(flow(-20.0), 60.0, 60.0 + H);
        assert_eq!(stuck.hold, Hold::Holding);
        assert_eq!(stuck.rect.bottom(), 60.0 + H, "the end is the band's floor");
    }

    #[test]
    fn one_point_past_it_releases() {
        let end = 60.0 + H - 1.0;
        let stuck = stick(flow(-20.0), 60.0, end);
        assert_eq!(stuck.hold, Hold::Released);
        assert_eq!(stuck.rect.bottom(), end, "the band rides the end");
        assert!(stuck.rect.top() < 60.0, "and is above the floor");
    }

    #[test]
    fn a_band_never_passes_its_end_or_leaves_its_slot_upward() {
        let floor = 60.0;
        let mut previous = f32::INFINITY;
        // From above the floor to well past the end of the block, so the clamp on
        // the end is exercised and not only the one on the floor.
        for step in 0..400 {
            let top = 400.0 - step as f32 * 2.0;
            let flow = flow(top);
            // A block that ends a little past the row it holds.
            let end = top + H + 120.0;
            let stuck = stick(flow, floor, end);
            assert!(
                stuck.rect.top() >= flow.top() - 0.01,
                "the band left its slot upward at {top}: {:?}",
                stuck.rect
            );
            assert!(
                stuck.rect.bottom() <= end + 0.01,
                "the band passed its end at {top}: {:?} against {end}",
                stuck.rect
            );
            assert!(
                stuck.rect.top() <= previous + 0.01,
                "the band moved down while the page scrolled up: {:?}",
                stuck.rect
            );
            previous = stuck.rect.top();
        }
    }

    #[test]
    fn a_resting_band_leaves_the_clip_alone() {
        let body = body_clip(clip(), band(400.0), Hold::Resting);
        assert_eq!(body, clip(), "a band in its slot takes no strip");
    }

    #[test]
    fn a_held_band_clips_the_body_from_its_bottom() {
        let body = body_clip(clip(), band(60.0), Hold::Holding);
        assert_eq!(body.min.y, 60.0 + H, "the rows stop at the band's bottom");
        assert_eq!(body.max, clip().max, "and keep the viewport's floor");
        assert_eq!(body.min.x, clip().min.x);
    }

    #[test]
    fn a_released_band_clips_the_body_above_it() {
        let body = body_clip(clip(), band(20.0), Hold::Released);
        assert_eq!(body.max.y, 20.0, "the rows stop above the riding band");
        assert_eq!(body.min, clip().min);
    }

    #[test]
    fn a_band_above_the_clip_leaves_no_body() {
        let body = body_clip(clip(), band(-80.0), Hold::Released);
        assert_eq!(body.max.y, clip().min.y, "the body is empty, not inverted");
        assert_eq!(body.height(), 0.0);
    }

    #[test]
    fn a_body_clip_never_grows_the_clip() {
        for top in [-120.0, -40.0, 0.0, 20.0, 60.0, 120.0, 260.0, 400.0] {
            for hold in [Hold::Resting, Hold::Holding, Hold::Released] {
                let body = body_clip(clip(), band(top), hold);
                assert!(
                    body.min.x >= clip().min.x
                        && body.min.y >= clip().min.y
                        && body.max.x <= clip().max.x
                        && body.max.y <= clip().max.y,
                    "{hold:?} at {top} grew the clip: {body:?}"
                );
                assert!(body.min.x == clip().min.x, "the columns are the caller's");
                assert!(body.max.x == clip().max.x, "the columns are the caller's");
            }
        }
    }

    #[test]
    fn stack_height_counts_only_the_gaps_between() {
        assert_eq!(stack_height(&[100.0, 50.0], 8.0), 158.0);
        assert_eq!(stack_height(&[], 8.0), 0.0, "nothing is zero high");
        assert_eq!(stack_height(&[42.0], 8.0), 42.0, "one block has no gap");
    }
}
