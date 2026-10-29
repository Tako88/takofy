//! The episodes shelf: the newest episodes of the followed shows.

use egui::Ui;

use crate::home::episodes::List;
use crate::theme::Tokens;
use crate::view::playlist::root::MediaDraw;

use super::Pressed;

/// Draws the shelf, and nothing until it has something to draw.
pub(super) fn show(
    _ui: &mut Ui,
    _list: &List<'_>,
    _now_ms: u64,
    _tokens: &Tokens,
    _draw: &mut MediaDraw<'_, '_>,
    _out: &mut Pressed,
) {
}
