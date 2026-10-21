//! The search results page: the whole answer, under its query's heading.
//!
//! The brief's R15: `Results for “q”` as the heading, `1 result` or `N results`
//! in the head's right slot where every other view keeps its size readout, no
//! count line under it, and no band — a query has no artwork to tint from. The
//! table is the track table this app already draws, which is the one the
//! playlist page draws with the membership date dropped.
//!
//! The page draws only what the search state already holds for its own query.
//! That guard is not decoration: a `Ready` answer left over from the query the
//! reader typed past must not be painted under this heading.

use std::time::Instant;

use egui::{Sense, Ui};

use crate::cover::Art;
use crate::format::format_position;
use crate::layout::{Table, table as table_layout};
use crate::library::{Library, Load};
use crate::message::Command;
use crate::player::Transport;
use crate::queue::Queue;
use crate::route::Page;
use crate::theme::{Tokens, extent};
use crate::view::text as recipe;

use super::section;
use super::skeleton;
use super::track_list::{head, list_rect};
use super::track_row::{self, Click, Row, state_of};

/// Draws the results page and returns the command its controls asked for.
#[expect(
    clippy::too_many_arguments,
    reason = "the pane, its table, the state it draws and the frame's cover store"
)]
pub fn search_view(
    ui: &mut Ui,
    table: Table,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Option<Command> {
    // The page's own query, so the read, the heading and the scroll area are all
    // keyed by it rather than by whichever query happens to be in the state.
    let Some(Page::Search(query)) = library.page() else {
        return None;
    };
    let query = query.clone();

    // No leading gap here: `tracks_view` adds the page's one gap before it
    // dispatches, and a second would start the page a step lower than every
    // other one.
    let mut command = None;
    let mut retry = false;
    // A link click is applied after the scroll area, because the matches below
    // borrow the answer it has to move away from.
    let mut open: Option<Page> = None;

    // The scroll area is salted with the query, so a new query starts at the top
    // instead of where the last one was left; `album.rs` solves the same problem
    // the same way.
    egui::ScrollArea::vertical()
        .id_salt(("search", query.as_str()))
        .show(ui, |ui| {
            // The page is drawn only for its own query. The state is pointed at
            // it when the page opens, so this is the one guard that keeps a
            // superseded answer off the page. Reaching it with no read at all is
            // a bug, not a state to draw blank.
            let answer = library.search.page_answer(&query);
            debug_assert!(
                answer.is_some(),
                "the search page {query:?} has no read of its own"
            );
            let Some(load) = answer else {
                return;
            };

            let aside = match load {
                // A count of one is a result, not "1 results": the head reads
                // as a sentence.
                Load::Ready(tracks) if tracks.len() == 1 => "1 result".to_owned(),
                Load::Ready(tracks) => format!("{} results", tracks.len()),
                // Nothing true to count yet, and the head already draws nothing
                // for an empty aside.
                Load::Idle | Load::Loading | Load::Failed(_) => String::new(),
            };
            // The heading sits in the reference's gutter, like every section
            // head; the page has no band, so nothing above it is full-bleed.
            super::inner(ui, |ui| {
                section::head(ui, &format!("Results for “{query}”"), &aside, tokens);

                match load {
                    Load::Idle | Load::Loading => {
                        skeleton::rows(ui, 5, table.columns().for_playlist(false), tokens);
                    }
                    Load::Failed(failure) => {
                        ui.label(recipe::muted(
                            crate::signin::failure_message(*failure),
                            tokens,
                        ));
                        if ui.button("Retry").clicked() {
                            retry = true;
                        }
                    }
                    Load::Ready(tracks) if tracks.is_empty() => {
                        ui.label(recipe::muted(
                            &format!("No tracks match “{query}”."),
                            tokens,
                        ));
                    }
                    Load::Ready(tracks) => {
                        let width = ui.available_width();
                        // A search answer is not a playlist's membership, so the
                        // date column goes; every other column is the playlist's own.
                        let columns =
                            table_layout::columns(width, table.columns().for_playlist(false));
                        head(ui, columns, tokens);
                        let count = tracks.len();
                        // The whole list is reserved at once, so the scrollbar is
                        // right, and only the rows the viewport can show are laid
                        // out and given interaction.
                        let list = list_rect(ui, count);
                        ui.allocate_rect(list, Sense::hover());
                        let visible = ui.clip_rect();

                        for (index, entry) in tracks.iter().enumerate() {
                            let rect = table_layout::row_rect(list, index, extent::ROW_H);
                            if !visible.intersects(rect) {
                                continue;
                            }
                            let duration = entry.duration_ms.map(format_position);
                            let row = Row {
                                uri: &entry.uri,
                                number: index + 1,
                                title: &entry.title,
                                title_cell: track_row::TitleCell::ArtThenName,
                                artist: &entry.artists,
                                artist_uri: entry.artist_uris.first().map(String::as_str),
                                album: entry.album.as_deref(),
                                album_uri: entry.album_uri.as_deref(),
                                // A search result was not added to anything.
                                added: None,
                                duration: duration.as_deref(),
                                state: state_of(&entry.uri, transport),
                                cover: art.look(entry.cover.as_ref(), now),
                            };
                            // R16: a row plays the whole answer, entered here, so
                            // next and previous walk the results.
                            match track_row::show(ui, rect, columns, &row, tokens) {
                                Some(Click::Play) => {
                                    command = queue.start(tracks, index, transport);
                                }
                                Some(Click::Open(page)) => open = Some(page),
                                None => {}
                            }
                        }
                    }
                }
            });
        });

    if retry {
        return library.reload_search();
    }
    if let Some(page) = open {
        return library.open(page);
    }
    command
}

#[cfg(test)]
mod tests {
    use egui::{Context, Pos2, RawInput, Rect, Vec2};
    use takofy_model::TrackEntry;

    use super::*;
    use crate::message::Update;
    use crate::theme::{fonts, space, style};

    /// The probe window: wide enough for the page's table.
    const PROBE: Vec2 = Vec2::new(900.0, 400.0);

    fn context() -> Context {
        let tokens = Tokens::DARK;
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));
        ctx
    }

    fn track(uri: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: uri.to_owned(),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
        }
    }

    /// The results page for `query`, answered with `count` tracks.
    fn answered(query: &str, count: usize) -> Library {
        let mut library = Library::default();
        let _ = library.open(Page::Search(query.to_owned()));
        library.apply(&Update::Search {
            query: query.to_owned(),
            tracks: (0..count)
                .map(|index| track(&format!("t{index}")))
                .collect(),
        });
        library
    }

    /// One frame of the page, at a fixed clock.
    fn frame(ctx: &Context, library: &mut Library) -> egui::FullOutput {
        let tokens = Tokens::DARK;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        art.begin_frame();
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, PROBE)),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            let _ = search_view(
                ui,
                Table::Full,
                library,
                &mut transport,
                &mut queue,
                &mut art,
                Instant::now(),
                &tokens,
            );
        });
        output.textures_delta.clear();
        output
    }

    /// Every line the frame painted, as it reads: the visible run of an elided
    /// line, and the width of its box.
    fn painted_lines(output: &egui::FullOutput) -> Vec<(String, f32)> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => text
                    .galley
                    .rows
                    .first()
                    .map(|row| (row.row.text().to_owned(), text.galley.size().x)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_head_counts_one_hit_in_the_singular() {
        let ctx = context();
        let mut library = answered("never gonna", 1);
        let lines = painted_lines(&frame(&ctx, &mut library));

        assert!(
            lines.iter().any(|(line, _)| line == "1 RESULT"),
            "one hit is one result: {lines:?}"
        );
        assert!(
            !lines.iter().any(|(line, _)| line == "1 RESULTS"),
            "and never \"1 results\": {lines:?}"
        );
    }

    #[test]
    fn the_head_counts_every_other_answer_in_the_plural() {
        for count in [0, 2, 20] {
            let ctx = context();
            let mut library = answered("never gonna", count);
            let lines = painted_lines(&frame(&ctx, &mut library));

            let expected = format!("{count} RESULTS");
            assert!(
                lines.iter().any(|(line, _)| *line == expected),
                "{count} hits read as {expected:?}: {lines:?}"
            );
        }
    }

    /// The heading is laid out by `recipe::line`, which wraps at the pane's
    /// width on one row and elides the rest, so a long query truncates instead
    /// of overflowing the page.
    #[test]
    fn a_very_long_query_truncates_its_heading() {
        let ctx = context();
        let query = "never gonna give you up, never gonna let you down, never gonna run \
            around and desert you, never gonna make you cry, never gonna say goodbye, never \
            gonna tell a lie and hurt you";
        let mut library = answered(query, 1);
        let lines = painted_lines(&frame(&ctx, &mut library));

        let (heading, width) = lines
            .iter()
            .find(|(line, _)| line.starts_with("Results for \u{201c}"))
            .unwrap_or_else(|| panic!("the heading was painted: {lines:?}"));
        assert!(
            heading.ends_with('\u{2026}'),
            "the long heading elides: {heading:?}"
        );
        assert!(
            *width <= PROBE.x - 2.0 * space::GUTTER + 0.5,
            "the heading stays inside the page's gutter: {width} in {}",
            PROBE.x
        );
    }

    /// The page draws only what the state holds for its own query. A page with
    /// no read of its own is unreachable today — `Library::settle` points it at
    /// its query whenever it opens — so reaching it is a bug and must fail
    /// loudly in tests rather than paint a blank page.
    #[test]
    #[should_panic(expected = "has no read of its own")]
    fn a_page_without_its_own_read_fails_loudly() {
        let ctx = context();
        let mut library = answered("never gonna", 1);
        // Point the read at another query behind the page's back.
        library.search.retry_page("give you up");

        let _ = frame(&ctx, &mut library);
    }
}
