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
mod tests;
