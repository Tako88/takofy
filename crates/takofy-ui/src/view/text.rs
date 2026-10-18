//! Text helpers shared by the views.

use egui::RichText;

use crate::theme::Tokens;

/// Secondary text, so chrome does not read as content.
pub fn muted(text: &str, tokens: &Tokens) -> RichText {
    RichText::new(text).color(tokens.palette.text_muted)
}
