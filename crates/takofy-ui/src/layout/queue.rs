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
mod tests {
    use super::*;
    use crate::theme::text;

    fn row(width: f32) -> Rect {
        Rect::from_min_size(pos2(10.0, 20.0), vec2(width, ROW_H))
    }

    fn outline(parts: Parts) -> [(&'static str, Rect); 3] {
        [
            ("art", parts.art),
            ("text", parts.text),
            ("duration", parts.duration),
        ]
    }

    #[test]
    fn the_row_height_is_the_art_plus_its_padding() {
        assert_eq!(ROW_H, extent::QUEUE_ART + 2.0 * space::S1);
        assert_eq!(ROW_H, 48.0);
    }

    #[test]
    fn the_parts_do_not_overlap_and_stay_inside_the_row() {
        for width in [272.0, extent::QUEUE_ART + space::S3 + 10.0, 400.0] {
            let row = row(width);
            let parts = row_parts(row, text::XXS * 3.0);
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
                parts.art.right() <= parts.text.left() + 0.01,
                "the art overlaps the text at {width}"
            );
            assert!(
                parts.text.right() <= parts.duration.left() + 0.01,
                "the text overlaps the duration at {width}"
            );
        }
    }

    #[test]
    fn the_gap_between_the_art_the_text_and_the_duration_is_s3() {
        let parts = row_parts(row(272.0), text::XXS * 3.0);
        assert_eq!(parts.art.width(), extent::QUEUE_ART);
        assert_eq!(parts.art.height(), extent::QUEUE_ART);
        assert!((parts.text.left() - parts.art.right() - space::S3).abs() < 0.01);
        assert!((parts.duration.left() - parts.text.right() - space::S3).abs() < 0.01);
        assert!((parts.duration.right() - inner_rect(row(272.0)).right()).abs() < 0.01);
    }

    #[test]
    fn without_a_duration_the_text_takes_the_rest() {
        let inner = inner_rect(row(272.0));
        let parts = row_parts(row(272.0), 0.0);
        assert_eq!(parts.duration.width(), 0.0);
        assert!(
            (parts.text.right() - inner.right()).abs() < 0.01,
            "the text must end where the row does, not at a missing column"
        );
    }

    #[test]
    fn a_narrow_row_clamps_to_zero_without_inverting() {
        for width in [0.0, 1.0, space::S2, extent::QUEUE_ART, 60.0] {
            let row = row(width);
            let parts = row_parts(row, 100.0);
            for (name, rect) in outline(parts) {
                assert!(rect.width() >= 0.0, "{name} at {width}: {rect:?}");
                assert!(rect.height() >= 0.0, "{name} at {width}: {rect:?}");
                assert!(
                    rect.width().is_finite() && rect.height().is_finite(),
                    "{name} at {width} is not finite: {rect:?}"
                );
            }
            assert!(parts.art.right() <= parts.text.left() + 0.01, "{width}");
            assert!(
                parts.text.right() <= parts.duration.left() + 0.01,
                "{width}"
            );
        }
    }

    #[test]
    fn a_zero_or_non_finite_rect_does_not_panic() {
        let cases = [
            Rect::ZERO,
            Rect::from_min_size(pos2(f32::NAN, 0.0), vec2(100.0, ROW_H)),
            Rect::from_min_size(pos2(0.0, 0.0), vec2(f32::INFINITY, ROW_H)),
            Rect::from_min_size(pos2(0.0, 0.0), vec2(-100.0, -ROW_H)),
        ];
        for row in cases {
            for duration_w in [0.0, 12.0, f32::NAN, f32::INFINITY, -5.0] {
                let parts = row_parts(row, duration_w);
                for (name, rect) in outline(parts) {
                    assert!(rect.width() >= 0.0, "{name} in {row:?}: {rect:?}");
                    assert!(
                        rect.min.x.is_finite() && rect.width().is_finite(),
                        "{name} in {row:?} is not finite: {rect:?}"
                    );
                }
            }
        }
    }
}
