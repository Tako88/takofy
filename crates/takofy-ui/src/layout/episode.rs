//! An episode row's interior: the play toggle, the text column and the duration.
//!
//! The reference composes the row as `2.5rem minmax(0,1fr) auto` with an `S4`
//! gap and `S4 S3` padding, so the toggle and the duration are fixed and the body
//! takes what is left.
//!
//! Every rect is clamped inside the row's inner box: a row too narrow for its
//! parts yields zero-width rects rather than inverted ones.

use egui::{Rect, pos2, vec2};

use crate::theme::{extent, space};

/// One row's pitch: the token the list reserves per index.
pub const ROW_H: f32 = extent::EPISODE_ROW_H;

/// The row's three parts, left to right.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Parts {
    pub toggle: Rect,
    pub body: Rect,
    pub duration: Rect,
}

/// Splits a row into its toggle, body and duration.
///
/// `duration_w` is the width the duration's label needs; zero means the entry
/// has no length and the body takes the whole of what is left.
pub fn row_parts(row: Rect, duration_w: f32) -> Parts {
    let inner = inner_rect(row);

    let duration_w = if duration_w.is_finite() {
        duration_w.max(0.0).min(inner.width())
    } else {
        0.0
    };

    let side = extent::EPISODE_TOGGLE
        .min(inner.width())
        .min(inner.height());
    let toggle = Rect::from_min_size(inner.min, vec2(side, side));

    let body_left = (toggle.right() + space::S4).min(inner.right());
    // A row too narrow to hold both keeps the two apart rather than letting one
    // slide under the other: whichever does not fit collapses.
    let duration_left = (inner.right() - duration_w).max(body_left);
    let duration = Rect::from_min_size(
        pos2(duration_left, inner.top()),
        vec2((inner.right() - duration_left).max(0.0), inner.height()),
    );
    let body_right = if duration.width() > 0.0 {
        (duration.left() - space::S4).max(body_left)
    } else {
        inner.right()
    };

    Parts {
        toggle,
        body: Rect::from_min_max(
            pos2(body_left, inner.top()),
            pos2(body_right.min(inner.right()), inner.bottom()),
        ),
        duration,
    }
}

/// The row's content box: the row less its `S4 S3` padding.
fn inner_rect(row: Rect) -> Rect {
    let row = sane(row);
    let left = row.left() + space::S3;
    let right = (row.right() - space::S3).max(left);
    let top = row.top() + space::S4;
    let bottom = (row.bottom() - space::S4).max(top);
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
mod tests {
    use super::*;
    use crate::theme::text;

    fn row(width: f32) -> Rect {
        Rect::from_min_size(pos2(10.0, 20.0), vec2(width, ROW_H))
    }

    fn outline(parts: Parts) -> [(&'static str, Rect); 3] {
        [
            ("toggle", parts.toggle),
            ("body", parts.body),
            ("duration", parts.duration),
        ]
    }

    #[test]
    fn every_part_sits_inside_the_row() {
        for width in [320.0, extent::EPISODE_TOGGLE + space::S4 + 10.0, 720.0] {
            let row = row(width);
            let parts = row_parts(row, text::XXS * 4.0);
            let inner = inner_rect(row);
            for (name, rect) in outline(parts) {
                assert!(
                    rect.left() >= inner.left() - 0.01 && rect.right() <= inner.right() + 0.01,
                    "{name} at {width} is outside {inner:?}: {rect:?}"
                );
                assert!(rect.width() >= 0.0, "{name} at {width} is negative");
                assert!(rect.height() >= 0.0, "{name} at {width} is negative");
            }
            assert!(
                parts.toggle.right() <= parts.body.left() + 0.01,
                "the toggle overlaps the body at {width}"
            );
            assert!(
                parts.body.right() <= parts.duration.left() + 0.01,
                "the body overlaps the duration at {width}"
            );
        }
    }

    #[test]
    fn a_zero_duration_width_is_safe() {
        for width in [0.0, 1.0, space::S3, extent::EPISODE_TOGGLE, 60.0] {
            let row = row(width);
            let parts = row_parts(row, 0.0);
            for (name, rect) in outline(parts) {
                assert!(rect.width() >= 0.0, "{name} at {width}: {rect:?}");
                assert!(rect.height() >= 0.0, "{name} at {width}: {rect:?}");
                assert!(
                    rect.width().is_finite() && rect.height().is_finite(),
                    "{name} at {width} is not finite: {rect:?}"
                );
            }
            assert_eq!(parts.duration.width(), 0.0, "no duration, no column");
        }
    }

    #[test]
    fn the_body_takes_the_remaining_width() {
        let inner = inner_rect(row(400.0));
        let with_duration = row_parts(row(400.0), 40.0);
        assert!((with_duration.toggle.left() - inner.left()).abs() < 0.01);
        assert!((with_duration.toggle.width() - extent::EPISODE_TOGGLE).abs() < 0.01);
        assert!(
            (with_duration.body.left() - with_duration.toggle.right() - space::S4).abs() < 0.01
        );
        assert!(
            (with_duration.body.right() - with_duration.duration.left() + space::S4).abs() < 0.01
        );
        assert!((with_duration.duration.right() - inner.right()).abs() < 0.01);

        let without = row_parts(row(400.0), 0.0);
        assert!(
            (without.body.right() - inner.right()).abs() < 0.01,
            "without a duration the body ends where the row does"
        );
    }
}
