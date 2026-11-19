//! The settings page.
//!
//! The page the router now reaches. Its rows — the sound engine, the Soloist
//! setup block, autoplay — land with the milestones that draw and wire them.

use egui::Ui;

use crate::message::Command;
use crate::theme::Tokens;
use crate::view::page_head;

/// Draws the page. The rows answer with commands once they exist.
pub(crate) fn settings_view(ui: &mut Ui, tokens: &Tokens) -> Option<Command> {
    page_head::show(ui, "Settings", "Sound · playback", tokens);
    None
}
