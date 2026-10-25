//! The Lyrics tab's pane: the lines, the one being sung, and the states.
//!
//! Unlike the queue, this pane lays every line out: a lyric is a few hundred
//! lines at most, and scrolling to the current one needs its rect to exist. The
//! pane scrolls itself once per line rather than once per frame, so a reader who
//! scrolls away is not pulled back while the same line is still being sung.

use egui::{Align, Layout, Rect, ScrollArea, Sense, Ui, UiBuilder, Vec2};
use takofy_model::TrackLyrics;

use crate::library::Load;
use crate::lyrics::{LyricsPanel, current_line};
use crate::signin::failure_message;
use crate::theme::{Tokens, space};

use super::text as recipe;

/// What a track with no lyrics says. The endpoint answered, and its answer was
/// that there is nothing to sing.
pub const NONE: &str = "No lyrics for this track.";

/// What the pane says while the fetch is in flight.
const LOADING: &str = "Loading the lyrics…";

/// Draws the lyrics pane and reports whether its Retry was pressed.
pub fn show(
    ui: &mut Ui,
    pane: Rect,
    panel: &LyricsPanel,
    position_ms: u32,
    track: Option<&str>,
    tokens: &Tokens,
) -> bool {
    if pane.width() <= 0.0 || pane.height() <= 0.0 {
        return false;
    }

    match panel.load() {
        Load::Idle | Load::Loading => state(ui, pane, LOADING, tokens, None),
        Load::Failed(failure) => state(ui, pane, failure_message(*failure), tokens, Some("Retry")),
        Load::Ready(lyrics) if lyrics.lines.is_empty() => state(ui, pane, NONE, tokens, None),
        Load::Ready(lyrics) => {
            lines(ui, pane, lyrics, position_ms, track, tokens);
            false
        }
    }
}

/// One of the pane's three one-line states, with its Retry when it has one.
fn state(ui: &mut Ui, pane: Rect, copy: &str, tokens: &Tokens, retry: Option<&str>) -> bool {
    let mut pressed = false;
    ui.scope_builder(
        UiBuilder::new()
            .id_salt("lyrics-state")
            .max_rect(pane)
            .layout(Layout::top_down(Align::Center)),
        |ui| {
            ui.add_space(pane.height() / 3.0);
            ui.label(recipe::muted(copy, tokens));
            if let Some(label) = retry
                && ui.button(label).clicked()
            {
                pressed = true;
            }
        },
    );
    pressed
}

/// The lines themselves: the one being sung in the text colour, the rest muted.
///
/// An unsynced lyric has no line being sung, so every line reads the same and
/// nothing scrolls.
fn lines(
    ui: &mut Ui,
    pane: Rect,
    lyrics: &TrackLyrics,
    position_ms: u32,
    track: Option<&str>,
    tokens: &Tokens,
) {
    let current = current_line(lyrics, position_ms);
    let wanted = ScrollTarget {
        track: track.map(str::to_owned),
        line: current,
    };
    // Read before the area runs: the scroll has to happen inside it, while the
    // current line's rect exists.
    let key = ScrollTarget::key(ui);
    let last = ui.data(|data| data.get_temp::<ScrollTarget>(key));

    ui.scope_builder(
        UiBuilder::new()
            .id_salt("lyrics-pane")
            .max_rect(pane)
            .layout(Layout::top_down(Align::Min)),
        |ui| {
            ui.set_clip_rect(pane);
            ScrollArea::vertical()
                // A track's lines start at the top: without its own state the
                // next track would open at the last one's offset.
                .id_salt(track.unwrap_or("no-track"))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = space::S2;
                    for (index, line) in lyrics.lines.iter().enumerate() {
                        let lit = lyrics.synced && current == Some(index);
                        let colour = if lyrics.synced && !lit {
                            tokens.palette.text_subtle
                        } else {
                            tokens.palette.text
                        };
                        let galley = recipe::clamped(
                            ui,
                            &line.words,
                            recipe::lyric_line(),
                            colour,
                            0.0,
                            ui.available_width(),
                            usize::MAX,
                        );
                        let (rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), galley.size().y.max(1.0)),
                            Sense::hover(),
                        );
                        ui.painter().galley(rect.min, galley, colour);
                        // One scroll per (track, line): a frame that re-scrolled
                        // would fight a reader who has moved away while the same
                        // line is still being sung.
                        if lit && scroll_decided(last.as_ref(), &wanted) {
                            ui.scroll_to_rect(rect, Some(Align::Center));
                            ui.data_mut(|data| data.insert_temp(key, wanted.clone()));
                        }
                    }
                });
        },
    );
}

/// Whether this frame is the one that brings `wanted` into view.
///
/// One scroll per (track, line), not one per frame.
fn scroll_decided(last: Option<&ScrollTarget>, wanted: &ScrollTarget) -> bool {
    last != Some(wanted)
}

/// What the pane remembers between frames: the line it last brought into view.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ScrollTarget {
    track: Option<String>,
    line: Option<usize>,
}

impl ScrollTarget {
    /// One id for the whole pane: it belongs to the window, not to a track.
    fn key(ui: &Ui) -> egui::Id {
        ui.scope_id().with("lyrics-scroll-target")
    }
}

#[cfg(test)]
mod tests {
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
}
