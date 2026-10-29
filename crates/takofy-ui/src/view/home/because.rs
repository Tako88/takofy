//! The because shelf: the artists related to one followed artist.

use egui::Ui;

use crate::home::because::Row;
use crate::theme::Tokens;
use crate::view::playlist::root::MediaDraw;

use super::Pressed;

/// Draws the shelf, and nothing until it has something to draw.
pub(super) fn show(
    _ui: &mut Ui,
    _row: &Row<'_>,
    _now_ms: u64,
    _tokens: &Tokens,
    _draw: &mut MediaDraw<'_, '_>,
    _out: &mut Pressed,
) {
}
