//! A queue row's interior: the art, the text and the duration.
//!
//! The reference composes the row as `2.5rem minmax(0,1fr) auto` with a `S3`
//! gap, so the art and the duration are fixed and the text takes what is left.
//! The row's own padding is `S1 S2`, which is why its height is the art plus two
//! `S1` and not the art alone.
//!
//! Every rect is clamped inside the row's inner box: a row too narrow for its
//! parts yields zero-width rects rather than inverted ones.

use egui::{Rect, pos2, vec2};

use crate::theme::{extent, space};

/// One row's pitch: the art plus the padding above and below it.
pub const ROW_H: f32 = extent::QUEUE_ART + 2.0 * space::S1;

/// The row's three parts, left to right.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Parts {
    pub art: Rect,
    pub text: Rect,
    pub duration: Rect,
}

/// Splits a row into its art, text and duration.
///
/// `duration_w` is the width the duration's label needs; zero means the entry
/// has no length and the text takes the whole of what is left.
pub fn row_parts(row: Rect, duration_w: f32) -> Parts {
    let inner = inner_rect(row);

    let duration_w = if duration_w.is_finite() {
        duration_w.max(0.0).min(inner.width())
    } else {
        0.0
    };

    let side = extent::QUEUE_ART.min(inner.width()).min(inner.height());
    let art = Rect::from_min_size(
        pos2(inner.left(), inner.center().y - side / 2.0),
        vec2(side, side),
    );

    let text_left = (art.right() + space::S3).min(inner.right());
    // A row too narrow to hold the duration keeps the two apart rather than
    // letting one slide under the other: whichever does not fit collapses.
    let duration_left = (inner.right() - duration_w).max(text_left);
    let duration = Rect::from_min_size(
        pos2(duration_left, inner.top()),
        vec2((inner.right() - duration_left).max(0.0), inner.height()),
    );
    let text_right = if duration.width() > 0.0 {
        (duration.left() - space::S3).max(text_left)
    } else {
        inner.right()
    };

    Parts {
        art,
        text: Rect::from_min_max(
            pos2(text_left, inner.top()),
            pos2(text_right.min(inner.right()), inner.bottom()),
        ),
        duration,
    }
}

/// The row's content box: the row less its own `S1 S2` padding.
fn inner_rect(row: Rect) -> Rect {
    let row = sane(row);
    let left = row.left() + space::S2;
    let right = (row.right() - space::S2).max(left);
    let top = row.top() + space::S1;
    let bottom = (row.bottom() - space::S1).max(top);
    Rect::from_min_max(pos2(left, top), pos2(right, bottom))
}

/// A rect with a finite origin and a non-negative size, so a bad caller cannot
/// turn an arithmetic clamp into a NaN.
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
