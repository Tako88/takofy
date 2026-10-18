//! The window's draw pass: the sign-in states, the library and the transport.
//!
//! The view reads the state machine and the tokens it is handed, and returns a
//! command when the user asks for one. It never transitions state itself.

use egui::{Align, Button, Layout, RichText, Ui};

use crate::layout::{self, Width};
use crate::library::{Library, Load};
use crate::message::Command;
use crate::player::{Transport, format_position};
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
        SignIn::SignedIn => shell(ui, library, transport, position_ms, tokens),
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

/// The signed-in shell. Panels take their share of the width the breakpoints
/// allow, and the central panel fills what is left.
fn shell(
    ui: &mut Ui,
    library: &mut Library,
    transport: &mut Transport,
    position_ms: u32,
    tokens: &Tokens,
) -> Option<Command> {
    let width = Width::of(ui.ctx().content_rect().width());
    let regions = layout::regions_for(width);
    let sizes = layout::shell_for(width);

    let mut command = None;
    if regions.sidebar
        && let Some(sidebar) = sizes.sidebar
    {
        let panel = egui::Panel::left("sidebar")
            .exact_size(sidebar)
            .show(ui, |ui| library_view(ui, library, tokens));
        command = command.or(panel.inner);
    }
    if regions.now_playing {
        egui::Panel::bottom("now_playing")
            .exact_size(sizes.now_playing)
            .show(ui, |ui| transport_view(ui, transport, position_ms, tokens));
    }
    if regions.main {
        let panel = egui::CentralPanel::default()
            .show(ui, |ui| tracks_view(ui, library, transport, tokens));
        command = command.or(panel.inner);
    }
    command
}

/// The sidebar: the playlists, or the state of the load that fetches them.
fn library_view(ui: &mut Ui, library: &mut Library, tokens: &Tokens) -> Option<Command> {
    ui.add_space(space::S3);
    ui.heading("Playlists");
    ui.add_space(space::S2);

    let mut clicked: Option<String> = None;
    let mut retry = false;
    match &library.playlists {
        Load::Idle => {}
        Load::Loading => {
            ui.spinner();
        }
        Load::Ready(playlists) if playlists.is_empty() => {
            ui.label(muted("No playlists yet.", tokens));
        }
        Load::Ready(playlists) => {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for entry in playlists {
                    let selected = library.selected() == Some(entry.uri.as_str());
                    if ui.selectable_label(selected, &entry.name).clicked() {
                        clicked = Some(entry.uri.clone());
                    }
                }
            });
        }
        Load::Failed(failure) => {
            ui.label(muted(failure_message(*failure), tokens));
            retry = ui.button("Retry").clicked();
        }
    }

    if let Some(uri) = clicked {
        return Some(library.select(&uri));
    }
    if retry { Some(library.reload()) } else { None }
}

/// The main pane: the selected playlist's tracks, or the state of their load.
fn tracks_view(
    ui: &mut Ui,
    library: &mut Library,
    transport: &mut Transport,
    tokens: &Tokens,
) -> Option<Command> {
    ui.add_space(space::S3);
    match &library.tracks {
        Load::Idle => {
            ui.label(muted("Select a playlist.", tokens));
            None
        }
        Load::Loading => {
            ui.spinner();
            None
        }
        Load::Ready(tracks) if tracks.tracks.is_empty() => {
            ui.label(muted("No tracks in this playlist.", tokens));
            None
        }
        Load::Ready(tracks) => {
            egui::ScrollArea::vertical()
                .show(ui, |ui| {
                    let mut command = None;
                    for entry in &tracks.tracks {
                        if ui.selectable_label(false, &entry.title).double_clicked() {
                            command = Some(transport.play(&entry.uri, &entry.title));
                        }
                    }
                    command
                })
                .inner
        }
        Load::Failed(failure) => {
            ui.label(muted(failure_message(*failure), tokens));
            if ui.button("Retry").clicked() {
                let playlist = library.selected().map(str::to_owned);
                playlist.map(|playlist| library.select(&playlist))
            } else {
                None
            }
        }
    }
}

/// The bar: what is playing, where it is, and what it is doing.
fn transport_view(ui: &mut Ui, transport: &Transport, position_ms: u32, tokens: &Tokens) {
    let Some(now) = &transport.now_playing else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(&now.title);
        ui.label(muted(format_position(position_ms).as_str(), tokens));
        ui.label(muted(transport.state.label(), tokens));
    });
}

/// Secondary text, so chrome does not read as content.
fn muted(text: &str, tokens: &Tokens) -> RichText {
    RichText::new(text).color(tokens.palette.text_muted)
}
