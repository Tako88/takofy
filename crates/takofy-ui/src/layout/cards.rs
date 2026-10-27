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

    /// The height `items` cards occupy: their rows and the gaps between them.
    pub fn height(&self, items: usize) -> f32 {
        let rows = self.rows(items);
        match rows {
            0 => 0.0,
            _ => rows as f32 * self.card_h + (rows - 1) as f32 * self.gap,
        }
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
mod tests;
