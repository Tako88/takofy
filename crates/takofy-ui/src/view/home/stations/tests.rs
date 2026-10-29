use std::time::Instant;

use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};
use takofy_model::TrackEntry;

use super::*;
use crate::cover::Art;
use crate::library::Library;
use crate::theme::{Tokens, fonts, style};
use crate::view::test_support::click_events;

/// The probe window: wide enough for two stations side by side and tall enough
/// for one of them.
const PROBE: Vec2 = Vec2::new(900.0, 600.0);
const NOW_MS: u64 = 1_700_000_000_000;

fn context() -> Context {
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, PROBE)),
        events,
        ..Default::default()
    }
}

fn track(uri: &str, title: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: title.to_owned(),
        artists: "Die Antwoord".to_owned(),
        album: None,
        album_uri: Some("spotify:album:1".to_owned()),
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

fn rows() -> Vec<TrackEntry> {
    vec![
        track("spotify:track:1", "Hey Sexy"),
        track("spotify:track:2", "I Don't Care"),
        track("spotify:track:3", "Fuck You"),
        track("spotify:track:4", "Cousin Marvin"),
    ]
}

fn shown<'a>(rows: &'a [TrackEntry]) -> Shown<'a> {
    Shown {
        kind: "Artist",
        head: "Seeded by Die Antwoord",
        art: None,
        uri: "spotify:station:artist:1",
        shown: vec![0, 2, 3],
        whole: rows,
    }
}

/// One frame that draws the whole band, and what the frame painted.
fn band_frame(ctx: &Context, band: &Band<'_>) -> egui::FullOutput {
    let tokens = Tokens::DARK;
    let library = Library::default();
    let mut art = Art::default();
    let mut select = None;
    let mut open = None;
    let mut play = None;
    let mut out = Pressed::default();
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let mut draw = MediaDraw {
            art: &mut art,
            now: Instant::now(),
            tokens: &tokens,
            library: &library,
            select: &mut select,
            open: &mut open,
            play: &mut play,
        };
        show(ui, band, NOW_MS, &tokens, &mut draw, &mut out);
    });
    output.textures_delta.clear();
    output
}

/// The text this frame painted, in paint order.
fn painted(output: &egui::FullOutput) -> Vec<String> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => Some(text.galley.text().to_owned()),
            egui::epaint::Shape::Vec(shapes) => {
                let mut words = Vec::new();
                for shape in shapes {
                    if let egui::epaint::Shape::Text(text) = shape {
                        words.push(text.galley.text().to_owned());
                    }
                }
                (!words.is_empty()).then_some(words.join("\u{0}"))
            }
            _ => None,
        })
        .flat_map(|text| {
            text.split('\u{0}')
                .map(str::to_owned)
                .collect::<Vec<String>>()
        })
        .collect()
}

/// Where the drawing put one station's play and its rows.
struct Places {
    play: Pos2,
    rows: Vec<Pos2>,
}

fn places(ctx: &Context, rect: Rect, station: &Shown<'_>) -> Places {
    let tokens = Tokens::DARK;
    let mut at = Places {
        play: Pos2::ZERO,
        rows: Vec::new(),
    };
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let head = head_rect(ui, rect, &tokens);
        at.play = play_rect(head).center();
        at.rows = row_rects(rect, head, station.shown.len())
            .iter()
            .map(Rect::center)
            .collect();
    });
    output.textures_delta.clear();
    at
}

/// Where one row's `⋯` sits inside the cell.
fn more(ctx: &Context, rect: Rect, station: &Shown<'_>, row: usize) -> Pos2 {
    let tokens = Tokens::DARK;
    let mut at = Pos2::ZERO;
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let head = head_rect(ui, rect, &tokens);
        at = media::columns(row_rects(rect, head, station.shown.len())[row])
            .more
            .center();
    });
    output.textures_delta.clear();
    at
}

/// The widget id one station row's body draws under.
fn row_id(root: egui::Id, station: &Shown<'_>, at: usize) -> egui::Id {
    let track = &station.whole[station.shown[at]];
    root.with(egui::IdSalt::new((station.uri, at)))
        .with(("library-media-row", track.uri.as_str()))
}

/// What one cell's presses asked for, run in the passes egui needs to report a
/// click at `at`.
fn press(ctx: &Context, rect: Rect, station: &Shown<'_>, at: Pos2) -> (Pressed, Option<Page>) {
    let tokens = Tokens::DARK;
    let library = Library::default();
    let mut art = Art::default();
    let mut out = Pressed::default();
    let mut open = None;
    for event in click_events(at) {
        let mut select = None;
        let mut play = None;
        let mut output = ctx.run_ui(input(vec![event]), |ui| {
            let mut draw = MediaDraw {
                art: &mut art,
                now: Instant::now(),
                tokens: &tokens,
                library: &library,
                select: &mut select,
                open: &mut open,
                play: &mut play,
            };
            cell_of(ui, rect, station, &tokens, &mut draw, &mut out);
        });
        output.textures_delta.clear();
    }
    (out, open)
}

const CELL: Rect = Rect {
    min: Pos2::new(0.0, 40.0),
    max: Pos2::new(420.0, 220.0),
};

#[test]
fn the_band_draws_its_title_and_every_station_it_holds() {
    let ctx = context();
    let rows = rows();
    let band = Band {
        stations: vec![shown(&rows)],
    };
    let words = painted(&band_frame(&ctx, &band));
    assert!(words.iter().any(|word| word == HOME_STATIONS_TITLE));
    assert!(words.iter().any(|word| word == "Seeded by Die Antwoord"));
    assert!(words.iter().any(|word| word == "ARTIST"));
    for title in ["Hey Sexy", "Fuck You", "Cousin Marvin"] {
        assert!(words.iter().any(|word| word == title), "{title}");
    }
    assert!(
        !words.iter().any(|word| word == "I Don't Care"),
        "the band draws only the rows it shows"
    );
}

#[test]
fn a_press_on_a_stations_play_hands_the_whole_station_to_the_queue() {
    let ctx = context();
    let rows = rows();
    let station = shown(&rows);
    let at = places(&ctx, CELL, &station);
    let (out, open) = press(&ctx, CELL, &station, at.play);
    assert_eq!(
        out.station,
        Some(("spotify:station:artist:1".to_owned(), rows.clone()))
    );
    assert_eq!(open, None, "the play opens nothing");
}

#[test]
fn a_press_on_a_row_opens_the_album_its_label_named() {
    let ctx = context();
    let rows = rows();
    let station = shown(&rows);
    let at = places(&ctx, CELL, &station);
    let (out, open) = press(&ctx, CELL, &station, at.rows[1]);
    assert_eq!(out.station, None);
    assert_eq!(open, Some(Page::Album("spotify:album:1".to_owned())));
}

#[test]
fn a_press_on_a_rows_more_posts_its_track_menu() {
    let ctx = context();
    let rows = rows();
    let station = shown(&rows);
    let at = more(&ctx, CELL, &station, 1);
    let (out, open) = press(&ctx, CELL, &station, at);
    assert_eq!(out.station, None);
    assert_eq!(open, None, "a menu opens no page");
    let request = crate::view::context_menu::take_request(&ctx).expect("a menu was posted");
    assert_eq!(request.targets.len(), 1);
    assert_eq!(request.targets[0].uri, "spotify:track:3");
    assert_eq!(request.listing, Listing::Page);
    assert_eq!(request.rows, vec![0]);
    assert!(
        request
            .items
            .iter()
            .any(|item| item.verb == crate::menu::Verb::GoToAlbum),
        "a track's menu offers the album it came from"
    );
}

#[test]
fn a_track_in_two_stations_draws_two_distinct_rows() {
    let ctx = context();
    let rows = rows();
    let first = shown(&rows);
    let second = Shown {
        uri: "spotify:station:artist:2",
        ..shown(&rows)
    };
    let band = Band {
        stations: vec![first, second],
    };
    let tokens = Tokens::DARK;
    let library = Library::default();
    let mut art = Art::default();
    let mut select = None;
    let mut open = None;
    let mut play = None;
    let mut out = Pressed::default();
    let mut a = None;
    let mut b = None;
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let root = ui.scope_id();
        let mut draw = MediaDraw {
            art: &mut art,
            now: Instant::now(),
            tokens: &tokens,
            library: &library,
            select: &mut select,
            open: &mut open,
            play: &mut play,
        };
        show(ui, &band, NOW_MS, &tokens, &mut draw, &mut out);
        a = ui.ctx().read_response(row_id(root, &band.stations[0], 0));
        b = ui.ctx().read_response(row_id(root, &band.stations[1], 0));
    });
    output.textures_delta.clear();
    let a = a.expect("the first station's row is a widget");
    let b = b.expect("the second station's row is a widget");
    assert_ne!(
        a.id, b.id,
        "two stations showing one track must draw two rows"
    );
}
