//! Wrapping a row of sized items into the width it has.
//!
//! The sidebar's filter chips and the discography's have the same job: place
//! each item left to right, break to a new row when the next one would not fit,
//! and report the height the rows took. One implementation, so the two rows
//! cannot disagree about when a line breaks.
//!
//! The rule is the reference's `flex-wrap`: an item that does not fit moves to
//! the next row, but the first item of a row is never moved — otherwise an item
//! wider than the row would wrap forever.

use egui::{Rect, Vec2, pos2};

/// The placed items and the height their rows occupy.
#[derive(Clone, Debug, PartialEq)]
pub struct Wrapped {
    /// One rect per item, in order, relative to the row's top-left corner.
    pub rects: Vec<Rect>,
    /// How tall the rows are: `rows * row_h` plus the gaps between them. Zero
    /// for no items.
    pub height: f32,
}

/// Wraps `sizes` into `width`, in rows `row_h` tall and `gap_y` apart.
///
/// `gap_x` is the space between two items on one row. The items keep their own
/// size; `row_h` is only the pitch between rows, which the sidebar needs to be
/// its button height rather than a chip's.
pub fn wrap(sizes: &[Vec2], width: f32, gap_x: f32, row_h: f32, gap_y: f32) -> Wrapped {
    let mut rects = Vec::with_capacity(sizes.len());
    let mut rows = 0usize;
    let mut x = 0.0;
    let mut first = true;

    for size in sizes {
        // The first item of a row is never moved, however wide it is: only the
        // item that would overflow is pushed to the next row.
        if !first && x + size.x > width {
            x = 0.0;
            rows += 1;
        }
        rects.push(Rect::from_min_size(pos2(x, rows as f32 * (row_h + gap_y)), *size));
        x += size.x + gap_x;
        first = false;
    }

    let height = if rects.is_empty() {
        0.0
    } else {
        (rows + 1) as f32 * row_h + rows as f32 * gap_y
    };

    Wrapped { rects, height }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sidebar's own numbers: buttons 28 tall, one `S1` between chips and
    /// between rows.
    const ROW_H: f32 = 28.0;
    const GAP: f32 = 4.0;

    fn row(widths: &[f32]) -> Vec<Vec2> {
        widths.iter().map(|width| Vec2::new(*width, 19.4)).collect()
    }

    fn mins(wrapped: &Wrapped) -> Vec<(f32, f32)> {
        wrapped
            .rects
            .iter()
            .map(|rect| (rect.min.x, rect.min.y))
            .collect()
    }

    #[test]
    fn wrap_empty_has_no_height_and_no_rects() {
        let wrapped = wrap(&[], 100.0, GAP, ROW_H, GAP);
        assert!(wrapped.rects.is_empty());
        assert_eq!(wrapped.height, 0.0);
    }

    #[test]
    fn wrap_one_row_places_items_left_to_right_with_the_gap() {
        let wrapped = wrap(&row(&[10.0, 12.0, 8.0]), 100.0, GAP, ROW_H, GAP);
        assert_eq!(mins(&wrapped), vec![(0.0, 0.0), (14.0, 0.0), (30.0, 0.0)]);
        assert_eq!(wrapped.height, ROW_H);
    }

    #[test]
    fn wrap_breaks_when_an_item_would_overflow() {
        // The second item ends at 14 + 12 = 26, past the 20 it has.
        let wrapped = wrap(&row(&[10.0, 12.0]), 20.0, GAP, ROW_H, GAP);
        assert_eq!(mins(&wrapped), vec![(0.0, 0.0), (0.0, ROW_H + GAP)]);
    }

    #[test]
    fn an_item_wider_than_the_width_still_gets_its_own_row_and_is_not_dropped() {
        let wrapped = wrap(&row(&[30.0, 5.0]), 10.0, GAP, ROW_H, GAP);
        assert_eq!(wrapped.rects.len(), 2, "nothing is dropped");
        assert_eq!(wrapped.rects[0].width(), 30.0, "it keeps its own size");
        assert_eq!(mins(&wrapped), vec![(0.0, 0.0), (0.0, ROW_H + GAP)]);
    }

    #[test]
    fn the_first_item_of_a_row_never_triggers_a_break() {
        let wrapped = wrap(&row(&[30.0, 8.0]), 10.0, GAP, ROW_H, GAP);
        assert_eq!(
            mins(&wrapped),
            vec![(0.0, 0.0), (0.0, ROW_H + GAP)],
            "the wide first item stays alone on its row rather than moving again"
        );
        assert_eq!(wrapped.height, 2.0 * ROW_H + GAP);
    }

    #[test]
    fn height_is_rows_times_row_h_plus_gaps() {
        // Three rows: 10 + 10 does not fit 15, and neither does one more.
        let wrapped = wrap(&row(&[10.0, 10.0, 10.0]), 15.0, GAP, ROW_H, GAP);
        assert_eq!(wrapped.height, 3.0 * ROW_H + 2.0 * GAP);
    }

    /// The sidebar's chip positions, captured from the inline loop before it
    /// moved onto [`wrap`]: four band widths, the sidebar's own gaps and button
    /// pitch, and the rects the real sidebar drew.
    #[test]
    fn sidebar_numbers_are_unchanged() {
        // Measured from the bundled mono face at `text::XXS`.
        let sizes = row(&[73.213, 59.994, 53.4, 66.588]);
        // Band left + S4, and the top of the chip row.
        let left = 16.0;
        let top = 193.0;

        for (band, expected) in [
            (
                254.0,
                vec![
                    (16.0, 193.0),
                    (93.213, 193.0),
                    (157.206, 193.0),
                    (16.0, 225.0),
                ],
            ),
            (
                200.0,
                vec![
                    (16.0, 193.0),
                    (93.213, 193.0),
                    (16.0, 225.0),
                    (73.4, 225.0),
                ],
            ),
            (
                140.0,
                vec![
                    (16.0, 193.0),
                    (16.0, 225.0),
                    (16.0, 257.0),
                    (16.0, 289.0),
                ],
            ),
            (
                100.0,
                vec![
                    (16.0, 193.0),
                    (16.0, 225.0),
                    (16.0, 257.0),
                    (16.0, 289.0),
                ],
            ),
        ] {
            let available = band - 2.0 * 16.0;
            let wrapped = wrap(&sizes, available, GAP, ROW_H, GAP);
            let placed: Vec<(f32, f32)> = wrapped
                .rects
                .iter()
                .map(|rect| (rect.min.x + left, rect.min.y + top))
                .collect();
            assert_eq!(
                placed.len(),
                expected.len(),
                "band width {band}: one rect per chip"
            );
            for (placed, expected) in placed.iter().zip(&expected) {
                assert!(
                    (placed.0 - expected.0).abs() < 0.01 && (placed.1 - expected.1).abs() < 0.01,
                    "band width {band}: {placed:?} is not {expected:?}"
                );
            }
        }
    }
}
