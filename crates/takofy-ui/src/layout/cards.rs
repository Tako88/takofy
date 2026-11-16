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

use egui::{Pos2, Rect, Vec2, pos2, vec2};

use crate::theme::{extent, space};

use super::CARD_KIND_STACK_MAX;

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
    grid_capped(available, card_min, gap, card_extra_h, usize::MAX)
}

/// [`grid`] with an exact column count.
///
/// The columns stretch to fill the area, so a fixed grid is a row of `columns`
/// cards sharing whatever width there is. A row whose column count is decided by
/// the body rather than by the available width uses this.
pub fn grid_fixed(available: Rect, columns: usize, gap: f32, card_extra_h: f32) -> Grid {
    let columns = columns.max(1);
    let avail = usable(available.width());
    let gap = usable(gap);
    let extra = usable(card_extra_h);
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

/// [`grid`] with a ceiling on the column count.
///
/// The columns still stretch to fill the area, so a capped grid is a row of
/// `max_columns` cards sharing whatever width there is rather than a row that
/// stops short of its own right edge. Home's card shelves are capped: a shelf is
/// a handful of things, and an uncapped grid draws as many columns as the width
/// allows rather than as many as the shelf has to say.
pub fn grid_capped(
    available: Rect,
    card_min: f32,
    gap: f32,
    card_extra_h: f32,
    max_columns: usize,
) -> Grid {
    let avail = usable(available.width());
    let card_min = usable(card_min);
    let gap = usable(gap);
    let columns = columns_for(avail, card_min, gap).min(max_columns.max(1));
    grid_fixed(available, columns, gap, card_extra_h)
}

/// How much of the picture a corner tile takes.
///
/// A fixed tile looks right on a card the size the reference draws and turns
/// into a chip on a card twice that, because the picture grows and the tile does
/// not. Sixteen percent keeps the proportion the design was judged at.
const TILE_OF_ART: f32 = 0.16;

/// One corner control, `space-2` in from the picture's own corner.
///
/// The tiles sit on one line at the picture's top right, and `steps_left` counts
/// in from the outermost, so the row is the same shape at every card width.
pub fn corner(art: Rect, size: f32, steps_left: f32) -> Rect {
    let right = art.right() - space::S2 - steps_left * (size + space::S1);
    let top = art.top() + space::S2;
    Rect::from_min_size(pos2(right - size, top), Vec2::splat(size))
}

/// The size of the corner tiles a picture of this width carries.
pub fn corner_tile(art: Rect) -> f32 {
    (art.width() * TILE_OF_ART).clamp(extent::CARD_PLAY, extent::CARD_PLAY_MAX)
}

/// The `⋯` a picture offers, outermost on its upper right.
pub fn corner_more(art: Rect) -> Rect {
    corner(art, corner_tile(art), 0.0)
}

/// Whether a card's kind word stacks above its sub-line at `width` points.
///
/// The reference drops the separator and gives the kind its own line at the same
/// query that turns the sidebar into the rail. A width the solver cannot answer
/// takes the stacked form, as the hero's tiers do.
pub fn kind_stacked(width: f32) -> bool {
    !width.is_finite() || width <= CARD_KIND_STACK_MAX
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
