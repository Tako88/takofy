//! The window's draw pass: the sign-in states, the library and the transport.
//!
//! The view reads the state machine and the tokens it is handed, and returns a
//! command when the user asks for one. It never transitions state itself.

mod art;
pub mod controls;
pub mod now_playing;
pub mod playbar;
pub mod playlist;
mod shell;
pub mod sidebar;
pub mod text;
pub mod topbar;
mod track_row;

use egui::{Align, Button, Layout, Ui};

use crate::drawer::Drawer;
use crate::library::Library;
use crate::message::Command;
use crate::player::Transport;
use crate::queue::Queue;
use crate::signin::{SignIn, failure_message};
use crate::theme::{Tokens, space};

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
        SignIn::Starting | SignIn::Authorizing => {
            progress(ui);
            None
        }
        SignIn::NeedsAuthorization | SignIn::Failed(_) => sign_in(ui, screen.state),
        SignIn::SignedIn => shell::show(ui, screen, tokens),
    }
}

/// A spinner with no control, so a second click cannot queue another sign-in.
fn progress(ui: &mut Ui) {
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.add_space(space::S6);
        ui.spinner();
    });
}

/// The prompt and retry control. The button is enabled only where a command is
/// accepted, so a stale frame cannot queue a second one.
fn sign_in(ui: &mut Ui, state: &mut SignIn) -> Option<Command> {
    let (heading, body, label) = match *state {
        SignIn::Failed(failure) => ("Could not sign in", failure_message(failure), "Try again"),
        _ => ("Takofy", "Sign in with Spotify to continue.", "Authorize"),
    };

    ui.heading(heading);
    ui.label(body);

    let enabled = state.can_authorize();
    if ui.add_enabled(enabled, Button::new(label)).clicked() {
        state.authorize()
    } else {
        None
    }
}
