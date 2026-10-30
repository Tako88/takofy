//! Home's card row: four across, sharing the width between them.
//!
//! The reference draws a shelf of cards as one row of four at the page's own
//! width. The library's grid fills as many columns as its minimum allows, which
//! on a 2500-point window is fifteen covers for a shelf that has four things to
//! say, so Home caps its shelves at four and lets them stretch edge to edge. A
//! page too narrow for four keeps the minimum width and draws fewer.

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
    solve_at(ui, COLUMNS).columns
}

/// The grid Home's card shelves lay their cards out in.
pub(super) fn solve(ui: &mut Ui) -> Grid {
    solve_at(ui, COLUMNS)
}

/// The area Home's card shelves start at, which is where the cursor is.
fn area(ui: &Ui) -> Rect {
    Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 0.0))
}

fn solve_at(ui: &Ui, max_columns: usize) -> Grid {
    cards::grid_capped(
        area(ui),
        extent::MEDIA_MIN_W,
        space::S3,
        media::extra_height(ui),
        max_columns,
    )
}

/// The rect the whole row takes, so the shelf after it starts below.
pub(super) fn row_rect(ui: &mut Ui, grid: &Grid, count: usize) -> Rect {
    root::grid_rect(ui, grid, count)
}

/// The pixels one card's art wants, at this window's scale.
pub(super) fn art_px(ui: &Ui, grid: &Grid) -> u32 {
    crate::cover::want_px(grid.card_w, ui.pixels_per_point())
}
