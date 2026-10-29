//! The stations shelf: a two-column band built from the library's own seeds.

use egui::Ui;

use crate::home::stations::Band;
use crate::theme::Tokens;
use crate::view::playlist::root::MediaDraw;

use super::Pressed;

/// Draws the shelf, and nothing until it has something to draw.
pub(super) fn show(
    _ui: &mut Ui,
    _band: &Band<'_>,
    _now_ms: u64,
    _tokens: &Tokens,
    _draw: &mut MediaDraw<'_, '_>,
    _out: &mut Pressed,
) {
}
