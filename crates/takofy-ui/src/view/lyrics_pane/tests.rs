use super::*;
use crate::lyrics::Tab;
use crate::message::{Failure, Update};
use crate::theme::{fonts, style};
use takofy_model::LyricLine;

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx
}

fn lyrics() -> TrackLyrics {
    TrackLyrics {
        lines: vec![
            LyricLine {
                start_ms: Some(1_000),
                end_ms: Some(2_000),
                words: "the first line".to_owned(),
            },
            LyricLine {
                start_ms: Some(5_000),
                end_ms: Some(6_000),
                words: "the second line".to_owned(),
            },
        ],
        synced: true,
    }
}

/// A panel that has asked for a track's lyrics and been answered with them.
fn answered(track: &str, lyrics: TrackLyrics) -> LyricsPanel {
    let mut library = crate::library::Library::default();
    library.lyrics.select(Tab::Lyrics);
    let _ = library.lyrics.request(Some(&now_playing(track)));
    library.apply(&Update::Lyrics {
        track: track.to_owned(),
        lyrics,
    });
    library.lyrics
}

fn now_playing(track: &str) -> crate::player::NowPlaying {
    crate::player::NowPlaying {
        uri: track.to_owned(),
        title: "A track".to_owned(),
        artists: String::new(),
        ..crate::player::NowPlaying::default()
    }
}

fn painted(output: &egui::FullOutput) -> Vec<(String, egui::Color32)> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => {
                Some((text.galley.text().to_owned(), text.fallback_color))
            }
            _ => None,
        })
        .collect()
}

fn run(panel: &LyricsPanel, position_ms: u32) -> egui::FullOutput {
    let tokens = Tokens::DARK;
    let ctx = context();
    let pane = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(312.0, 400.0));
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(400.0, 500.0),
            )),
            ..Default::default()
        },
        |ui| {
            let _ = show(
                ui,
                pane,
                panel,
                position_ms,
                Some("spotify:track:a"),
                &tokens,
            );
        },
    );
    output.textures_delta.clear();
    output
}

#[test]
fn a_track_with_no_lyrics_says_so() {
    let panel = answered("spotify:track:a", TrackLyrics::default());
    let texts: Vec<String> = painted(&run(&panel, 0))
        .into_iter()
        .map(|(text, _)| text)
        .collect();
    assert!(texts.contains(&NONE.to_owned()), "{texts:?}");
}

#[test]
fn a_failed_load_offers_its_retry() {
    let mut library = crate::library::Library::default();
    library.lyrics.select(Tab::Lyrics);
    let _ = library
        .lyrics
        .request(Some(&now_playing("spotify:track:a")));
    library.apply(&Update::LyricsFailed {
        track: "spotify:track:a".to_owned(),
        failure: Failure::Lyrics,
    });
    let texts: Vec<String> = painted(&run(&library.lyrics, 0))
        .into_iter()
        .map(|(text, _)| text)
        .collect();
    assert!(
        texts.iter().any(|text| text.contains("lyrics")),
        "the failure's own copy: {texts:?}"
    );
    assert!(texts.contains(&"Retry".to_owned()), "{texts:?}");
}

#[test]
fn the_line_being_sung_is_the_only_one_in_the_text_colour() {
    let tokens = Tokens::DARK;
    let panel = answered("spotify:track:a", lyrics());
    let drawn = painted(&run(&panel, 5_500));
    let lit: Vec<&String> = drawn
        .iter()
        .filter(|(_, colour)| *colour == tokens.palette.text)
        .map(|(text, _)| text)
        .collect();
    assert_eq!(lit, vec!["the second line"], "{drawn:?}");
}

/// The scroll happens once per (track, line), which is what keeps it from
/// fighting a reader who has moved away while the same line is sung.
#[test]
fn the_scroll_is_decided_once_per_line() {
    let line = |track: &str, index: usize| ScrollTarget {
        track: Some(track.to_owned()),
        line: Some(index),
    };
    assert!(
        scroll_decided(None, &line("a", 3)),
        "the first frame scrolls"
    );
    assert!(
        !scroll_decided(Some(&line("a", 3)), &line("a", 3)),
        "the same line holds"
    );
    assert!(
        scroll_decided(Some(&line("a", 3)), &line("a", 4)),
        "the next line"
    );
    assert!(
        scroll_decided(Some(&line("a", 3)), &line("b", 3)),
        "a new track starts at its own line"
    );
    assert!(
        scroll_decided(Some(&line("a", 3)), &line("a", 3)).eq(&false),
        "no second scroll for the same pair"
    );
}

#[test]
fn an_unsynced_lyric_lights_every_line() {
    let tokens = Tokens::DARK;
    let mut unsynced = lyrics();
    unsynced.synced = false;
    let panel = answered("spotify:track:a", unsynced);
    let drawn = painted(&run(&panel, 5_500));
    let words: Vec<&String> = drawn.iter().map(|(text, _)| text).collect();
    assert!(
        words.contains(&&"the first line".to_owned())
            && words.contains(&&"the second line".to_owned()),
        "every line is drawn: {words:?}"
    );
    let muted = drawn
        .iter()
        .filter(|(_, colour)| *colour == tokens.palette.text_subtle)
        .count();
    assert_eq!(muted, 0, "an unsynced lyric has no current line");
}
