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
    assert!((with_duration.body.left() - with_duration.toggle.right() - space::S4).abs() < 0.01);
    assert!((with_duration.body.right() - with_duration.duration.left() + space::S4).abs() < 0.01);
    assert!((with_duration.duration.right() - inner.right()).abs() < 0.01);

    let without = row_parts(row(400.0), 0.0);
    assert!(
        (without.body.right() - inner.right()).abs() < 0.01,
        "without a duration the body ends where the row does"
    );
}
