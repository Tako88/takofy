use std::time::Instant;

use egui::{Context, Pos2, RawInput, Rect, Vec2};
use takofy_model::TrackEntry;

use super::*;
use crate::cover::Art;
use crate::library::Library;
use crate::theme::{Tokens, fonts, style};

const PROBE: Vec2 = Vec2::new(600.0, 300.0);
const ROW: Rect = Rect {
    min: Pos2::new(0.0, 20.0),
    max: Pos2::new(400.0, 68.0),
};

fn context() -> Context {
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx
}

fn input() -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, PROBE)),
        ..Default::default()
    }
}

fn track(uri: &str, title: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: title.to_owned(),
        artists: "Vela Moreno".to_owned(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: Some(232_000),
        named: true,
        cover: None,
    }
}

#[test]
fn one_track_in_two_salts_draws_two_distinct_rows() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let library = Library::default();
    let mut art = Art::default();
    let mut select = None;
    let mut open = None;
    let mut play = None;
    let mut first = None;
    let mut second = None;
    let track = track("spotify:track:1", "Ashlight");

    let mut output = ctx.run_ui(input(), |ui| {
        {
            let mut draw = MediaDraw {
                art: &mut art,
                now: Instant::now(),
                tokens: &tokens,
                library: &library,
                select: &mut select,
                open: &mut open,
                play: &mut play,
            };
            first = Some(show(
                ui,
                ROW,
                Row {
                    salt: "spotify:station:1",
                    track: &track,
                    duration: duration_of(&track, &draw),
                },
                &mut draw,
                &tokens,
            ));
        }
        let mut draw = MediaDraw {
            art: &mut art,
            now: Instant::now(),
            tokens: &tokens,
            library: &library,
            select: &mut select,
            open: &mut open,
            play: &mut play,
        };
        second = Some(show(
            ui,
            ROW.translate(Vec2::new(0.0, 60.0)),
            Row {
                salt: "spotify:station:2",
                track: &track,
                duration: duration_of(&track, &draw),
            },
            &mut draw,
            &tokens,
        ));
    });
    output.textures_delta.clear();

    let first = first.expect("the first row is a widget");
    let second = second.expect("the second row is a widget");
    assert_ne!(
        first.id, second.id,
        "two rows showing one track must draw two rows"
    );
}

#[test]
fn the_duration_is_taken_from_the_label_read_or_the_wire() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let library = Library::default();
    let mut art = Art::default();
    let mut select = None;
    let mut open = None;
    let mut play = None;
    let track = track("spotify:track:1", "Ashlight");
    let mut duration = None;

    let mut output = ctx.run_ui(input(), |ui| {
        let draw = MediaDraw {
            art: &mut art,
            now: Instant::now(),
            tokens: &tokens,
            library: &library,
            select: &mut select,
            open: &mut open,
            play: &mut play,
        };
        duration = duration_of(&track, &draw);
        let _ = ui;
    });
    output.textures_delta.clear();

    assert_eq!(duration.as_deref(), Some("3:52"));
}
