//! The window's draw pass: the sign-in states and the signed-in shell.
//!
//! The view reads the state machine and the tokens it is handed, and returns a
//! command when the user asks for one. It never transitions state itself.

use egui::{Align, Button, Layout, Ui};

use crate::layout::{self, Width};
use crate::message::Command;
use crate::signin::{SignIn, failure_message};
use crate::theme::{Tokens, space};

/// Draws the current state, returning a command when the user asks for one.
pub fn show(ui: &mut Ui, state: &mut SignIn, tokens: &Tokens) -> Option<Command> {
    match *state {
        SignIn::Starting | SignIn::Authorizing => {
            progress(ui);
            None
        }
        SignIn::NeedsAuthorization | SignIn::Failed(_) => sign_in(ui, state),
        SignIn::SignedIn => {
            shell(ui, tokens);
            None
        }
    }
}

/// A spinner with no control, so a second click cannot queue another sign-in.
fn progress(ui: &mut Ui) {
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.add_space(space::XL);
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

/// The empty shell once signed in. Panels take their share of the width the
/// breakpoints allow, and the central panel fills what is left.
fn shell(ui: &mut Ui, tokens: &Tokens) {
    let width = Width::of(ui.ctx().content_rect().width());
    let regions = layout::regions_for(width);
    let sizes = layout::shell_for(width, tokens);

    if regions.sidebar
        && let Some(sidebar) = sizes.sidebar
    {
        egui::Panel::left("sidebar")
            .exact_size(sidebar)
            .show(ui, |_ui| {});
    }
    if regions.now_playing {
        egui::Panel::bottom("now_playing")
            .exact_size(sizes.now_playing)
            .show(ui, |_ui| {});
    }
    if regions.main {
        egui::CentralPanel::default().show(ui, |_ui| {});
    }
}
