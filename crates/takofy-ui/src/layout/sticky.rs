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
mod tests;
