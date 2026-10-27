use std::time::Instant;

use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};

use takofy_model::{AlbumDetail, TrackEntry};

use super::super::*;
use crate::cover::Cover;
use crate::message::Update;
use crate::theme::{fonts, space, style};
use crate::view::test_support::click_events;
use crate::view::track_list;

pub(super) const ALBUM: &str = "spotify:album:a";
/// The album a stale ready detail names while the page is on [`ALBUM`].
pub(super) const OTHER_ALBUM: &str = "spotify:album:b";
/// The probe window: 900 by 800, tall enough for the band, the head and two
/// rows in full.
pub(super) const PROBE: Vec2 = Vec2::new(900.0, 800.0);

pub(super) fn context() -> Context {
    let tokens = Tokens::DARK;
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&tokens));
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
    input: RawInput,
    draw: impl FnMut(&mut egui::Ui),
) -> egui::FullOutput {
    let mut output = ctx.run_ui(input, draw);
    output.textures_delta.clear();
    output
}

/// Every text the frame painted.
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

/// The rect a painted line occupies, read back from the frame.
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

pub(super) fn track(uri: &str, title: &str, artists: &str, duration_ms: u32) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: title.to_owned(),
        artists: artists.to_owned(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: Some(duration_ms),
        named: true,
        cover: None,
    }
}

pub(super) fn detail(tracks: Vec<TrackEntry>) -> AlbumDetail {
    AlbumDetail {
        uri: ALBUM.to_owned(),
        name: "Signal Decay".to_owned(),
        artists: "Nils Aker".to_owned(),
        artist_uri: None,
        year: Some(1997),
        cover: None,
        tracks,
    }
}

pub(super) fn library_with(detail: AlbumDetail) -> Library {
    let mut library = Library::default();
    library.open(Page::Album(ALBUM.to_owned()));
    library.apply(&Update::Album(detail));
    library
}

pub(super) fn frame(
    ctx: &Context,
    events: Vec<Event>,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
) -> (Option<Command>, egui::FullOutput) {
    let tokens = Tokens::DARK;
    let mut command = None;
    let output = run(ctx, input(events), |ui| {
        // A notch lands in the frame it arrives, as the window's own draw
        // pass makes it: egui's spread of one notch over several frames
        // would still be moving the list while a probe reads a row's place.
        crate::scroll::instant(ui.ctx());
        if let Some(next) = album_view(
            ui,
            Table::Full,
            library,
            transport,
            queue,
            art,
            Instant::now(),
            &tokens,
        ) {
            command = Some(next);
        }
    });
    (command, output)
}

/// The rect the `index`-th row is drawn into, solved the way the page solves
/// it: the band, then the head, then the whole list reserved in one block.
pub(super) fn row_rect_for(ctx: &Context, index: usize, count: usize) -> Rect {
    let tokens = Tokens::DARK;
    let mut found = None;
    run(ctx, input(Vec::new()), |ui| {
        ui.add_space(space::S3);
        let band = hero::Band {
            eyebrow: "Album",
            title: "Signal Decay",
            meta: Vec::new(),
            credit: None,
            description: "",
            cover: Cover::Missing,
            tint: None,
        };
        let _ = hero::show_with(ui, &band, &tokens, hero::Title::Plain);
        actionbar::row_rect(ui);
        crate::view::inner(ui, |ui| {
            let columns =
                table_layout::columns(ui.available_width(), Table::Full.columns().for_plays());
            track_list::head(ui, columns, &tokens);
            let list = track_list::list_rect(ui, count);
            found = Some(table_layout::row_rect(list, index, extent::ROW_H));
        });
    });
    found.expect("the head and the list lay out")
}

/// The title and artist cells and the row itself for the `index`-th row,
/// solved the way the page solves them from the band down.
pub(super) fn cells_for(ctx: &Context, index: usize, count: usize) -> (Rect, Rect, Rect) {
    let tokens = Tokens::DARK;
    let mut found = None;
    run(ctx, input(Vec::new()), |ui| {
        ui.add_space(space::S3);
        let band = hero::Band {
            eyebrow: "Album",
            title: "Signal Decay",
            meta: Vec::new(),
            credit: None,
            description: "",
            cover: Cover::Missing,
            tint: None,
        };
        let _ = hero::show_with(ui, &band, &tokens, hero::Title::Plain);
        actionbar::row_rect(ui);
        crate::view::inner(ui, |ui| {
            let columns =
                table_layout::columns(ui.available_width(), Table::Full.columns().for_plays());
            track_list::head(ui, columns, &tokens);
            let list = track_list::list_rect(ui, count);
            let row = table_layout::row_rect(list, index, extent::ROW_H);
            let placed = columns.at(row.min);
            found = Some((placed.title, placed.artist, row));
        });
    });
    found.expect("the head and the list lay out")
}

/// Clicks `pos` `times` times, one event per pass, and returns the last
/// command the view reported.
pub(super) fn click(
    ctx: &Context,
    pos: Pos2,
    times: usize,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
) -> Option<Command> {
    let mut last = None;
    for _ in 0..times {
        for event in click_events(pos) {
            last = frame(ctx, vec![event], library, transport, queue, art).0;
        }
    }
    last
}

/// The band's height: the art plus its padding, or the band's own minimum.
pub(super) fn band_height() -> f32 {
    (crate::theme::hero::ART + 2.0 * space::S5).max(crate::theme::hero::MIN_H)
}

/// The top of the band a frame painted, read back from the frame: the band's
/// wash is the one full-pane-width rect as tall as the band itself.
pub(super) fn band_top(output: &egui::FullOutput, width: f32) -> Option<f32> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(rect)
                if (rect.rect.width() - width).abs() < 0.5
                    && (rect.rect.height() - band_height()).abs() < 0.5 =>
            {
                Some(rect.rect.top())
            }
            _ => None,
        })
}
