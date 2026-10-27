use super::*;
use crate::theme::{extent, space};

/// The values the painter will pass from the token layer: the reference's
/// `9rem` minimum and `space-3` gap, plus a made-up content height for the
/// tests.
const CARD_MIN: f32 = extent::CARD_MIN_W;
const GAP: f32 = space::S3;
const EXTRA: f32 = 48.0;

fn assert_close(left: f32, right: f32) {
    assert!((left - right).abs() < 0.01, "{left} is not {right}");
}

/// An area wide enough for several cards and tall enough for these tests.
fn area(width: f32) -> Rect {
    Rect::from_min_size(pos2(20.0, 30.0), vec2(width, 2000.0))
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.left() < b.right() - 0.01
        && b.left() < a.right() - 0.01
        && a.top() < b.bottom() - 0.01
        && b.top() < a.bottom() - 0.01
}

#[test]
fn one_column_is_the_floor_below_two_cards_and_a_gap() {
    // Two 9rem cards and a 12-point gap need 300 points; below that the
    // reference's auto-fill keeps a single column that takes the whole
    // width, even when that is narrower than a card.
    for width in [0.0, 1.0, 50.0, CARD_MIN - 1.0, CARD_MIN, CARD_MIN + 1.0] {
        let grid = grid(area(width), CARD_MIN, GAP, EXTRA);
        assert_eq!(grid.columns, 1, "{width}");
        assert_close(grid.card_w, width);
    }
    assert_eq!(
        grid(area(2.0 * CARD_MIN + GAP - 1.0), CARD_MIN, GAP, EXTRA).columns,
        1
    );
    assert_eq!(
        grid(area(2.0 * CARD_MIN + GAP), CARD_MIN, GAP, EXTRA).columns,
        2
    );
}

#[test]
fn the_columns_are_the_most_that_fit_and_share_the_width() {
    for width in [300.0, 312.0, 640.0, 1000.0, 1440.0] {
        let grid = grid(area(width), CARD_MIN, GAP, EXTRA);
        let expected = (((width + GAP) / (CARD_MIN + GAP)).floor() as usize).max(1);
        assert_eq!(grid.columns, expected, "{width}");
        assert_close(
            grid.card_w,
            (width - GAP * (expected - 1) as f32) / expected as f32,
        );
        assert!(grid.card_w >= CARD_MIN - 0.01, "{width}: {}", grid.card_w);
    }
}

#[test]
fn every_card_tiles_without_overlap_and_stays_inside_the_area() {
    let available = area(1000.0);
    let grid = grid(available, CARD_MIN, GAP, EXTRA);
    let items = grid.columns * 3 + 2;
    assert!(
        grid.columns > 1,
        "the grid must be non-trivial to be a test"
    );

    let rects: Vec<Rect> = (0..items).map(|index| grid.card_rect(index)).collect();
    for (index, rect) in rects.iter().enumerate() {
        assert!(rect.width() > 0.0, "{index}: {rect:?}");
        assert_close(rect.height(), grid.card_w + EXTRA);
        assert!(rect.left() >= available.left() - 0.01, "{index}: {rect:?}");
        assert!(
            rect.right() <= available.right() + 0.01,
            "{index}: {rect:?}"
        );
        assert!(rect.top() >= available.top() - 0.01, "{index}: {rect:?}");
        assert!(
            rect.bottom() <= available.bottom() + 0.01,
            "{index}: {rect:?}"
        );
    }

    // Each full row ends flush with the area's right edge.
    for index in (grid.columns - 1..items).step_by(grid.columns) {
        assert_close(rects[index].right(), available.right());
    }

    for first in 0..items {
        for second in first + 1..items {
            assert!(
                !overlaps(rects[first], rects[second]),
                "cards {first} and {second} overlap: {:?} and {:?}",
                rects[first],
                rects[second]
            );
        }
    }
}

#[test]
fn a_card_is_as_tall_as_its_art_plus_the_painters_content() {
    let grid = grid(area(1000.0), CARD_MIN, GAP, EXTRA);
    for index in 0..5 {
        assert_close(grid.card_rect(index).height(), grid.card_w + EXTRA);
    }
}

#[test]
fn the_row_count_follows_the_item_count() {
    let grid = grid(area(1000.0), CARD_MIN, GAP, EXTRA);
    let columns = grid.columns;
    assert!(columns > 1, "the grid must have more than one column");
    assert_eq!(grid.rows(0), 0);
    assert_eq!(grid.rows(1), 1);
    assert_eq!(grid.rows(columns - 1), 1);
    assert_eq!(grid.rows(columns), 1);
    assert_eq!(grid.rows(columns + 1), 2);
    assert_eq!(grid.rows(2 * columns), 2);
    assert_eq!(grid.rows(2 * columns + 1), 3);
}

#[test]
fn a_card_beyond_the_item_count_starts_the_next_row() {
    let grid = grid(area(1000.0), CARD_MIN, GAP, EXTRA);
    let first = grid.card_rect(0);
    let past = grid.card_rect(grid.columns);
    assert_close(past.left(), first.left());
    assert_close(past.top(), first.bottom() + GAP);
    assert_close(past.height(), first.height());
}

#[test]
fn the_first_card_sits_on_the_areas_own_corner() {
    let available = area(1000.0);
    let grid = grid(available, CARD_MIN, GAP, EXTRA);
    let first = grid.card_rect(0);
    assert_eq!(first.min, available.min);
    assert_close(first.width(), grid.card_w);
}

#[test]
fn height_is_the_rows_and_the_gaps_between_them() {
    let grid = grid(area(1000.0), CARD_MIN, GAP, EXTRA);
    let card_h = grid.card_w + EXTRA;
    assert_eq!(grid.height(0), 0.0);
    assert_close(grid.height(1), card_h);
    assert_close(grid.height(grid.columns), card_h);
    assert_close(grid.height(grid.columns + 1), 2.0 * card_h + GAP);
    for items in 1..=40 {
        let last = grid.card_rect(items - 1);
        assert_close(grid.height(items), last.bottom() - grid.card_rect(0).top());
    }
}

#[test]
fn height_is_finite_for_degenerate_inputs() {
    for width in [0.0, -5.0, f32::NAN, f32::INFINITY] {
        let available = Rect::from_min_size(Pos2::ZERO, vec2(width, 10.0));
        let grid = grid(available, f32::NAN, f32::NAN, f32::NAN);
        assert!(grid.height(10).is_finite());
    }
}

#[test]
fn zero_and_non_finite_inputs_do_not_panic() {
    let areas = [
        Rect::from_min_size(Pos2::ZERO, vec2(0.0, 0.0)),
        Rect::from_min_size(Pos2::ZERO, vec2(-5.0, 10.0)),
        Rect::from_min_size(Pos2::ZERO, vec2(f32::NAN, 10.0)),
        Rect::from_min_size(Pos2::ZERO, vec2(f32::INFINITY, 10.0)),
    ];
    for available in areas {
        for card_min in [0.0, -1.0, f32::NAN, f32::INFINITY, CARD_MIN] {
            for gap in [0.0, -1.0, f32::NAN, GAP] {
                for extra in [0.0, f32::NAN, EXTRA] {
                    let grid = grid(available, card_min, gap, extra);
                    assert!(grid.columns >= 1);
                    assert!(grid.card_w.is_finite() && grid.card_w >= 0.0);
                    for index in 0..5 {
                        let rect = grid.card_rect(index);
                        assert!(rect.min.x.is_finite(), "{rect:?}");
                        assert!(rect.min.y.is_finite(), "{rect:?}");
                        assert!(rect.width().is_finite() && rect.width() >= 0.0);
                        assert!(rect.height().is_finite() && rect.height() >= 0.0);
                    }
                }
            }
        }
    }
}
