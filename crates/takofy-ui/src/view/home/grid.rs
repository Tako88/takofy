//! Home's card row: four across, sharing the width between them.
//!
//! A shelf is a handful of things, so Home caps its row at four columns rather
//! than letting the grid's auto-fill draw as many as the width allows. The four
//! share the width between them, edge to edge. A page too narrow for four at the
//! minimum card width draws fewer, and those stretch the same way.

use egui::{Rect, Ui, vec2};

use crate::layout::cards::{self, Grid};
use crate::theme::{extent, space};
use crate::view::media;

use super::super::playlist::root;

/// How many cards a Home shelf draws, which is also its column ceiling.
pub(super) const COLUMNS: usize = 4;

/// The columns this page's width draws: four, or fewer when it cannot hold four
/// at the minimum card width.
pub(super) fn columns(ui: &Ui) -> usize {
    solve_at(ui, COLUMNS, media::extra_height(ui)).columns
}

/// The grid Home's card shelves lay their cards out in.
pub(super) fn solve(ui: &mut Ui) -> Grid {
    solve_at(ui, COLUMNS, media::extra_height(ui))
}

/// The grid a shelf lays its cards out in when each card's kind word takes its
/// own line above the artist's.
pub(super) fn solve_lead(ui: &mut Ui) -> Grid {
    solve_at(ui, COLUMNS, media::extra_height_stacked(ui))
}

/// The area Home's card shelves start at, which is where the cursor is.
fn area(ui: &Ui) -> Rect {
    Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 0.0))
}

fn solve_at(ui: &Ui, max_columns: usize, extra: f32) -> Grid {
    cards::grid_capped(area(ui), extent::MEDIA_MIN_W, space::S3, extra, max_columns)
}

/// The rect the whole row takes, so the shelf after it starts below.
pub(super) fn row_rect(ui: &mut Ui, grid: &Grid, count: usize) -> Rect {
    root::grid_rect(ui, grid, count)
}

/// The pixels one card's art wants, at this window's scale.
pub(super) fn art_px(ui: &Ui, grid: &Grid) -> u32 {
    crate::cover::want_px(grid.card_w, ui.pixels_per_point())
}

#[cfg(test)]
mod tests;
