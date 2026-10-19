//! Card-grid arithmetic.
//!
//! The reference lays a card grid out with
//! `repeat(auto-fill, minmax(9rem, 1fr))` and a `space-3` gap: as many columns
//! as fit at the minimum width, sharing whatever is left over. `grid` solves
//! that count and the resulting card width; `card_rect` places the cards. The
//! painter and the hit-test both read the same rects, so a click and the card
//! under it can never disagree.
//!
//! The grid knows only geometry. A card's height is its square art plus
//! `card_extra_h`, which the painter owns — the padding, the title and the sub
//! line. The play tile, its offset, its rise and its easing are the painter's
//! too and are not solved here.
//!
//! Every value is taken from the token layer by the caller (`extent::CARD_MIN_W`,
//! `space::S3`), not read here. A zero, negative or non-finite width counts as
//! zero, as it does in `layout::table`.

use egui::{Pos2, Rect, pos2, vec2};

/// A solved card grid.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Grid {
    /// How many cards fit across the available area.
    pub columns: usize,
    /// The width of one card. The art is square, so this is the art's height
    /// too; the full card is `card_w + card_extra_h` tall.
    pub card_w: f32,
    card_h: f32,
    gap: f32,
    origin: Pos2,
}

impl Grid {
    /// The number of rows `items` cards occupy.
    ///
    /// An empty grid has no rows, and a partial last row still counts as a row.
    pub fn rows(&self, items: usize) -> usize {
        items.div_ceil(self.columns)
    }

    /// The rect of the `index`-th card, in the same space as the area `grid`
    /// was given.
    ///
    /// The index is a position, not a bound: an `index` past the grid's item
    /// count returns the next row's card at the same column. That is what the
    /// painter needs — it asks for the indices it holds and the hit-test only
    /// tests cards it has drawn — and it means the vertical extent is decided
    /// by the caller's item count rather than clamped to the area's height. The
    /// first card sits on the area's top-left corner and the columns tile the
    /// area's width exactly.
    pub fn card_rect(&self, index: usize) -> Rect {
        let column = index % self.columns;
        let row = index / self.columns;
        let x = self.origin.x + column as f32 * (self.card_w + self.gap);
        let y = self.origin.y + row as f32 * (self.card_h + self.gap);
        Rect::from_min_size(pos2(x, y), vec2(self.card_w, self.card_h))
    }
}

/// Solves the reference's auto-fill grid for an `available` area.
///
/// The reference's `repeat(auto-fill, minmax(card_min, 1fr))` with a `gap`
/// between tracks fills as many columns as fit at the minimum:
///
/// ```text
/// n = max(1, floor((avail + gap) / (card_min + gap)))
/// w = (avail - gap * (n - 1)) / n
/// ```
///
/// `w` is never below `card_min` while the columns fit, and the last column's
/// right edge lands exactly on the area's right edge. A card's height is `w`
/// plus `card_extra_h`; the gap separates rows as well as columns, as the
/// reference's single `gap` does.
pub fn grid(available: Rect, card_min: f32, gap: f32, card_extra_h: f32) -> Grid {
    let avail = usable(available.width());
    let card_min = usable(card_min);
    let gap = usable(gap);
    let extra = usable(card_extra_h);

    let columns = columns_for(avail, card_min, gap);
    let card_w = ((avail - gap * (columns - 1) as f32) / columns as f32).max(0.0);
    let card_h = card_w + extra;

    Grid {
        columns,
        card_w,
        card_h,
        gap,
        origin: pos2(
            finite_or_zero(available.min.x),
            finite_or_zero(available.min.y),
        ),
    }
}

/// The auto-fill column count for non-negative `avail`, `card_min` and `gap`.
///
/// A zero minimum makes `minmax(0, 1fr)` meaningless — every positive width
/// fits infinitely many zero-width columns — so it falls back to a single
/// column, as does a degenerate pitch.
fn columns_for(avail: f32, card_min: f32, gap: f32) -> usize {
    let pitch = card_min + gap;
    if card_min <= 0.0 || !pitch.is_finite() || pitch <= 0.0 {
        return 1;
    }
    let count = ((avail + gap) / pitch).floor();
    if count.is_finite() && count >= 1.0 {
        count as usize
    } else {
        1
    }
}

/// A width as a usable length: negative and non-finite become zero.
fn usable(value: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}

/// A coordinate with non-finite values replaced by zero.
fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

#[cfg(test)]
mod tests {
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
}
