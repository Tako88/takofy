//! The window's draw pass: the sign-in states, the library and the transport.
//!
//! The view reads the state machine and the tokens it is handed, and returns a
//! command when the user asks for one. It never transitions state itself.

mod shell;
mod text;

use egui::{Align, Button, Layout, Ui};

use crate::library::Library;
use crate::message::Command;
use crate::player::Transport;
use crate::signin::{SignIn, failure_message};
use crate::theme::{Tokens, space};

/// Draws the current state, returning a command when the user asks for one.
pub fn show(
    ui: &mut Ui,
    state: &mut SignIn,
    library: &mut Library,
    transport: &mut Transport,
    position_ms: u32,
    tokens: &Tokens,
) -> Option<Command> {
    match *state {
        SignIn::Starting | SignIn::Authorizing => {
            progress(ui);
            None
        }
        SignIn::NeedsAuthorization | SignIn::Failed(_) => sign_in(ui, state),
        SignIn::SignedIn => shell::show(ui, library, transport, position_ms, tokens),
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
