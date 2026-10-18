//! The signed-in shell: the panel grid and the three regions it holds.
//!
//! `egui::Panel` is deliberately not used. Its frame margins do not collapse, so
//! a gap between two panels would be the sum of two margins, and its separator
//! line reserves stroke width inside the size that was asked for. The grid is
//! solved as rects instead, which is what the reference's CSS grid does.

use egui::{Align, Color32, Layout, Rect, Stroke, StrokeKind, Ui, UiBuilder, Vec2};

use crate::layout::{self, Grid, Sidebar};
use crate::library::{Library, Load};
use crate::message::Command;
use crate::player::{Transport, format_position};
use crate::signin::failure_message;
use crate::theme::{Tokens, radius, space, stroke};

use super::text::muted;

/// The signed-in shell: three columns over a playbar, each panel a hairline box
/// on the window background.
pub fn show(
    ui: &mut Ui,
    library: &mut Library,
    transport: &mut Transport,
    position_ms: u32,
    tokens: &Tokens,
) -> Option<Command> {
    let content = ui.ctx().content_rect();
    let regions = layout::regions_for(content.width());
    let shell = layout::shell_for(&regions);
    let grid = layout::grid_for(content, &shell);

    // Chrome first, so each region draws on top of its own panel. The playbar
    // has none: it sits straight on the window.
    panel(ui, grid.sidebar, tokens.palette.surface, tokens);
    panel(ui, main_panel(&grid), tokens.palette.surface, tokens);
    if let Some(now_playing) = grid.now_playing {
        panel(ui, now_playing, tokens.palette.panel, tokens);
    }

    let mut command = match regions.sidebar {
        Sidebar::Full => region(ui, "sidebar", grid.sidebar, |ui| {
            library_view(ui, library, tokens)
        }),
        // The reference drops the wordmark, the nav labels and the whole library
        // block below 900; the rail keeps only the icons, which arrive with the
        // chrome.
        Sidebar::Rail => None,
    };
    command = command.or(region(ui, "view", grid.view, |ui| {
        tracks_view(ui, library, transport, tokens)
    }));
    region(ui, "playbar", grid.playbar, |ui| {
        transport_view(ui, transport, position_ms, tokens)
    });

    command
}

/// The topbar and the view share one panel, so they share one frame.
fn main_panel(grid: &Grid) -> Rect {
    Rect::from_min_max(grid.topbar.min, grid.view.max)
}

/// Paints one panel: a hairline box on the window background.
fn panel(ui: &Ui, rect: Rect, fill: Color32, tokens: &Tokens) {
    ui.painter().rect(
        rect,
        radius::LG,
        fill,
        Stroke::new(stroke::HAIRLINE, tokens.palette.border),
        StrokeKind::Inside,
    );
}

/// Draws one region into its rect, clipped to the panel it sits on.
///
/// The inset stands in for the reference's per-region padding, which arrives
/// with the chrome.
fn region<R>(ui: &mut Ui, id: &str, panel: Rect, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let inset = Vec2::splat(space::S3);
    let content = panel.shrink(stroke::HAIRLINE).shrink2(inset);

    ui.scope_builder(
        UiBuilder::new()
            .id_salt(id)
            .max_rect(content)
            .layout(Layout::top_down(Align::Min)),
        |ui| {
            // A child inherits its parent's clip rect, so without this a panel's
            // contents would spill into the channels beside it.
            ui.set_clip_rect(panel);
            add_contents(ui)
        },
    )
    .inner
}

/// The sidebar: the playlists, or the state of the load that fetches them.
fn library_view(ui: &mut Ui, library: &mut Library, tokens: &Tokens) -> Option<Command> {
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
