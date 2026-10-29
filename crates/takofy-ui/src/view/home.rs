//! Home: the greeting, and the shelf of what was saved most recently.
//!
//! The page draws what the library decided and asks for nothing of its own: a
//! press is returned as the command or the want that follows from it. The
//! greeting is always drawn, so the page is never a bare title with nothing
//! under it.

use std::time::Instant;

use egui::Ui;

use crate::cover::Art;
use crate::library::Library;
use crate::message::Command;
use crate::theme::Tokens;

mod dated;

/// Draws Home and returns the command its controls asked for.
pub(super) fn home_view(
    ui: &mut Ui,
    library: &mut Library,
    art: &mut Art,
    now: Instant,
    now_ms: u64,
    local_hour: u8,
    tokens: &Tokens,
) -> Option<Command> {
    dated::home_view(ui, library, art, now, now_ms, local_hour, tokens)
}
