//! The vertical stack inside the Now Playing panel.
//!
//! `layout.rs` owns which regions a window shows and how wide each one is; this
//! is the arithmetic inside one region. The reference's panel is a flex column —
//! head, art, meta, tabs, pane — so the stack is solved top down, and the art is
//! the one element that gives way: it shrinks so the pane keeps its minimum, and
//! stops at its floor rather than vanishing.
//!
//! Everything is clamped. A band shorter than its own contents yields a stack
//! that still ends at the band's bottom, with the pane's height never negative.

use egui::{Rect, pos2, vec2};

use crate::layout::queue;
use crate::theme::{extent, space, text};

/// The panel's head: the caption and the more button.
pub const HEAD_H: f32 = extent::BUTTON;

/// The queue pane's caption, `space-2` above and below a line of caps.
pub const CAPTION_H: f32 = text::XXS + 2.0 * space::S2;

/// The least the pane is worth drawing: its caption and one row.
pub const PANE_MIN_H: f32 = CAPTION_H + queue::ROW_H;

/// The panel's five parts, top to bottom.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Stack {
    pub head: Rect,
    pub art: Rect,
    pub meta: Rect,
    pub tabs: Rect,
    /// The scrollable pane. It spans the band's full width; its own content is
    /// inset by the pane, which is the one thing that scrolls.
    pub pane: Rect,
}

/// The height the meta block reserves: title, artist, the gap before the state
/// and the state line, whether or not each of them is drawn.
///
/// Nothing about a track enters it, so the art box and the tabs do not move when
/// playback starts. The arguments are measured heights, which keeps this pure.
pub fn meta_reserved(title_h: f32, secondary_h: f32, state_h: f32) -> f32 {
    height(title_h) + height(secondary_h) + space::S1 + height(state_h)
}

/// Solves the stack inside `band`.
///
/// `meta_h` is what [`meta_reserved`] returns and `tabs_h` the height of the tab
/// pair, both measured by the painter because they depend on the fonts.
pub fn stack_for(band: Rect, meta_h: f32, tabs_h: f32) -> Stack {
    let band = sane(band);
    let meta_h = height(meta_h);
    let tabs_h = height(tabs_h);
    let inset = inset(band, space::S3);
    let width = inset.width();

    let head = Rect::from_min_size(inset.min, vec2(width, HEAD_H.min(inset.height())));

    let art_top = head.bottom() + space::S3;
    // What the art may take once everything below it holds its minimum.
    let below = space::S4 + meta_h + space::S3 + tabs_h + space::S3 + PANE_MIN_H;
    let room = band.bottom() - art_top - below;
    let capped = width.min(extent::NOWPLAYING_ART_MAX);
    let side = room.min(capped).max(extent::NOWPLAYING_ART_MIN);
    let art = Rect::from_min_size(
        pos2(band.center().x - side / 2.0, art_top),
        vec2(side, side),
    );

    let meta = Rect::from_min_size(
        pos2(inset.left(), art.bottom() + space::S4),
        vec2(width, meta_h),
    );
    let tabs = Rect::from_min_size(
        pos2(inset.left(), meta.bottom() + space::S3),
        vec2(width, tabs_h),
    );
    let pane_top = (tabs.bottom() + space::S3).min(band.bottom());
    let pane = Rect::from_min_max(
        pos2(band.left(), pane_top),
        pos2(band.right(), band.bottom()),
    );

    Stack {
        head,
        art,
        meta,
        tabs,
        pane,
    }
}

/// `rect` inset by `amount` on every side, collapsed to zero rather than
/// inverted when it is too small to inset.
fn inset(rect: Rect, amount: f32) -> Rect {
    let left = rect.left() + amount;
    let top = rect.top() + amount;
    let right = (rect.right() - amount).max(left);
    let bottom = (rect.bottom() - amount).max(top);
    Rect::from_min_max(pos2(left, top), pos2(right, bottom))
}

/// A measured height, with anything unusable read as none.
fn height(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

/// A rect with a finite origin and a non-negative size.
fn sane(rect: Rect) -> Rect {
    if !rect.min.x.is_finite()
        || !rect.min.y.is_finite()
        || !rect.width().is_finite()
        || !rect.height().is_finite()
    {
        return Rect::ZERO;
    }
    Rect::from_min_size(
        rect.min,
        vec2(rect.width().max(0.0), rect.height().max(0.0)),
    )
}

#[cfg(test)]
mod tests;
