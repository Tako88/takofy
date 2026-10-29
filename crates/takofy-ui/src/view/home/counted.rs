//! The counted shelf: the artists the library holds the most of.

use egui::Ui;

use crate::home::counted::Tiles;
use crate::theme::Tokens;
use crate::view::playlist::root::MediaDraw;

use super::Pressed;

/// Draws the shelf, and nothing until it has something to draw.
pub(super) fn show(
    _ui: &mut Ui,
    _tiles: &Tiles<'_>,
    _now_ms: u64,
    _tokens: &Tokens,
    _draw: &mut MediaDraw<'_, '_>,
    _out: &mut Pressed,
) {
}
