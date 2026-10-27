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
        rects.push(Rect::from_min_size(
            pos2(x, rows as f32 * (row_h + gap_y)),
            *size,
        ));
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

/// Lays `count` cells in a fixed `columns`-wide grid across `width`.
///
/// Every cell is the same width — a grid, not a wrap — because the sidebar's
/// four filters are the primary navigation and a ragged row of them reads as an
/// afterthought. The last row is filled left to right like the others.
pub fn grid(count: usize, width: f32, columns: usize, row_h: f32, gap: f32) -> Wrapped {
    let columns = columns.max(1);
    let cell = ((width - gap * (columns as f32 - 1.0)) / columns as f32).max(0.0);
    let rects = (0..count)
        .map(|index| {
            let column = index % columns;
            let row = index / columns;
            Rect::from_min_size(
                pos2(column as f32 * (cell + gap), row as f32 * (row_h + gap)),
                Vec2::new(cell, row_h),
            )
        })
        .collect::<Vec<_>>();
    let rows = count.div_ceil(columns);
    let height = if rows == 0 {
        0.0
    } else {
        rows as f32 * row_h + (rows as f32 - 1.0) * gap
    };

    Wrapped { rects, height }
}

#[cfg(test)]
mod tests;
