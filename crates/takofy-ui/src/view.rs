//! The window's draw pass: the sign-in states, the library and the transport.
//!
//! The view reads the state machine and the tokens it is handed, and returns a
//! command when the user asks for one. It never transitions state itself.

mod art;
pub mod card;
pub mod controls;
mod hero;
pub mod now_playing;
pub mod playbar;
pub mod playlist;
mod shell;
pub mod sidebar;
mod sign_in;
pub mod text;
pub mod topbar;
mod track_row;

use egui::Ui;

use crate::drawer::Drawer;
use crate::library::Library;
use crate::message::Command;
use crate::player::Transport;
use crate::queue::Queue;
use crate::signin::SignIn;
use crate::theme::Tokens;

/// The state the window draws from, borrowed for one pass.
pub struct Screen<'a> {
    pub state: &'a mut SignIn,
    pub library: &'a mut Library,
    pub transport: &'a mut Transport,
    pub queue: &'a mut Queue,
    pub drawer: &'a mut Drawer,
    pub position_ms: u32,
}

/// Draws the current state, returning a command when the user asks for one.
pub fn show(ui: &mut Ui, screen: &mut Screen<'_>, tokens: &Tokens) -> Option<Command> {
    match *screen.state {
        // Starting, authorizing and a prompt are one card: with words when there
        // are words, with a spinner while the flow works.
        SignIn::Starting | SignIn::Authorizing | SignIn::NeedsAuthorization | SignIn::Failed(_) => {
            sign_in::show(ui, screen.state, tokens)
        }
        SignIn::SignedIn => shell::show(ui, screen, tokens),
    }
}
