use super::super::*;

use std::time::Instant;

use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};
use takofy_model::{FollowedShow, PlaylistEntry};

use crate::layout::Table;
use crate::message::Update;
use crate::player::Transport;
use crate::queue::Queue;
use crate::theme::{extent, fonts, style};
use crate::view::playlist::tracks_view;
use crate::view::test_support::click_events;

pub(super) const PROBE: Vec2 = Vec2::new(900.0, 800.0);
pub(super) const NOW_MS: u64 = 1_761_134_400_000;

pub(super) fn context() -> Context {
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx
}

pub(super) fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, PROBE)),
        events,
        ..Default::default()
    }
}

pub(super) fn run(
    ctx: &Context,
    events: Vec<Event>,
    library: &mut Library,
    art: &mut Art,
) -> egui::FullOutput {
    let tokens = Tokens::DARK;
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut output = ctx.run_ui(input(events), |ui| {
        let _ = tracks_view(
            ui,
            Table::Full,
            library,
            &mut transport,
            &mut queue,
            art,
            Instant::now(),
            NOW_MS,
            9,
            &tokens,
        );
    });
    output.textures_delta.clear();
    output
}

pub(super) fn frame(ctx: &Context, library: &mut Library, art: &mut Art) -> egui::FullOutput {
    run(ctx, Vec::new(), library, art)
}

pub(super) fn drawn_texts(output: &egui::FullOutput) -> Vec<&str> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect()
}

pub(super) fn text_rect(output: &egui::FullOutput, label: &str) -> Option<Rect> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some(Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
}

pub(super) fn playlist(uri: &str, name: &str) -> PlaylistEntry {
    PlaylistEntry {
        saved_at: None,
        composed: Vec::new(),
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        cover: None,
        description: None,
        owner: None,
        tracks: None,
        access: None,
    }
}

pub(super) fn show(uri: &str, name: &str) -> FollowedShow {
    FollowedShow {
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        publisher: None,
        cover: None,
        episodes: None,
    }
}

pub(super) fn library(playlists: Vec<PlaylistEntry>, shows: Vec<FollowedShow>) -> Library {
    let mut library = Library::default();
    library.apply(&Update::Playlists(playlists));
    library.apply(&Update::FollowedShows(shows));
    library
}

pub(super) fn click(
    ctx: &Context,
    pos: Pos2,
    library: &mut Library,
    art: &mut Art,
) -> Option<Command> {
    let tokens = Tokens::DARK;
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut command = None;
    for event in click_events(pos) {
        let command = &mut command;
        let library = &mut *library;
        let output = ctx.run_ui(input(vec![event]), |ui| {
            if let Some(next) = tracks_view(
                ui,
                Table::Full,
                library,
                &mut transport,
                &mut queue,
                art,
                Instant::now(),
                NOW_MS,
                9,
                &tokens,
            ) {
                *command = Some(next);
            }
        });
        let mut output = output;
        output.textures_delta.clear();
    }
    command
}

/// The rect a chip's label was painted into, plus its own padding.
pub(super) fn chip_rect(output: &egui::FullOutput, label: &str) -> Rect {
    let text = text_rect(output, &label.to_uppercase()).expect("the chip is drawn");
    text.expand2(Vec2::new(extent::CHIP_PAD_X, extent::CHIP_PAD_Y))
}

/// The rect the `index`-th card is drawn into, solved the way the view
/// solves it: the head, the chip row, then the grid at the probe's width.
pub(super) fn card_rect_for(ctx: &Context, index: usize, count: usize) -> Rect {
    let tokens = Tokens::DARK;
    let mut found = None;
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            crate::view::inner(ui, |ui| {
                page_head::show(ui, "Your Library", &library_note(count), &tokens);
                let _ = filter_row::show_with(
                    ui,
                    &tokens,
                    &library_filter::ROW,
                    RootKind::All,
                    RootKind::control,
                    root_filter_label,
                    filter_row::RowSlot::Trailing(0.0),
                );
                let solved = media_grid(ui);
                found = Some(solved.card_rect(index));
            });
        });
    });
    output.textures_delta.clear();
    found.expect("the root lays out")
}

pub(super) fn followed_artist(uri: &str, name: &str) -> takofy_model::FollowedArtist {
    takofy_model::FollowedArtist {
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        portrait: None,
        releases: None,
    }
}

pub(super) fn saved_album(uri: &str, name: &str) -> takofy_model::SavedAlbum {
    takofy_model::SavedAlbum {
        saved_at: None,
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        artists: "Nils Aker".to_owned(),
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        year: None,
        cover: None,
        tracks: None,
    }
}

/// The rect the `index`-th list row is drawn into, solved the way the view
/// solves it: the head, the chips, then the rows.
pub(super) fn row_rect_for(ctx: &Context, index: usize) -> Rect {
    let tokens = Tokens::DARK;
    let mut found = None;
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        ui.add_space(space::S3);
        crate::view::inner(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    crate::view::inner(ui, |ui| {
                        page_head::show(ui, "Your Library", "", &tokens);
                        let _ = filter_row::show_with(
                            ui,
                            &tokens,
                            &library_filter::ROW,
                            RootKind::All,
                            RootKind::control,
                            root_filter_label,
                            filter_row::RowSlot::Trailing(view_toggle::reserved()),
                        );
                        let width = ui.available_width();
                        let height = space::S2 + media::head_height(ui) + space::S2;
                        let _ = ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
                        let top = ui.cursor().min.y;
                        found = Some(Rect::from_min_size(
                            Pos2::new(ui.cursor().min.x, top + index as f32 * extent::ROW_H),
                            vec2(width, extent::ROW_H),
                        ));
                    });
                });
        });
    });
    output.textures_delta.clear();
    found.expect("the root lays out")
}
