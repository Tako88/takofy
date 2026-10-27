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
