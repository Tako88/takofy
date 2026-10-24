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

use egui::{Id, Sense, Ui};

use crate::cover::Art;
use crate::format::format_position;
use crate::layout::{Table, table as table_layout};
use crate::library::{Library, Load};
use crate::message::Command;
use crate::player::Transport;
use crate::queue::Queue;
use crate::route::Page;
use crate::scroll;
use crate::theme::{Tokens, extent};
use crate::view::text as recipe;

use super::carry::{self, Ghost};
use super::heart;
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
    // The page a row's menu is about, so a verb that leads here is omitted and a
    // page change can close the menu.
    let here = Page::Search(query.clone());
    let writing = library.write_in_flight();

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
            scroll::follow_drag(ui);
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
                        let mut selection = carry::List::open(
                            ui.ctx(),
                            Id::unique(("search-tracks", query.as_str())),
                            count,
                        );
                        let gate = selection.gate(writing);

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
                                look: selection.look(index),
                                menu: gate.allows(),
                                liked: library.likes.state(&entry.uri),
                            };
                            // R16: a row plays the whole answer, entered here, so
                            // next and previous walk the results.
                            match track_row::show(ui, rect, columns, &row, tokens) {
                                Some(Click::Play) => {
                                    command = queue.start(tracks, index, transport);
                                }
                                Some(Click::Open(page)) => open = Some(page),
                                Some(Click::Select(press)) => selection.press(index, press),
                                Some(Click::Like(liked)) => {
                                    heart::post(
                                        ui.ctx(),
                                        heart::Ask {
                                            tracks: vec![entry.uri.clone()],
                                            liked,
                                        },
                                    );
                                }
                                Some(Click::Menu(door)) => selection.menu(
                                    ui.ctx(),
                                    door,
                                    index,
                                    gate,
                                    carry::Open {
                                        listing: carry::Listing::Page,
                                        here: Some(&here),
                                        likes: &library.likes,
                                    },
                                    |row| tracks.get(row).cloned(),
                                ),
                                Some(Click::Lift) => {
                                    let ghost = Ghost::Table(row.lifted(columns));
                                    selection.lift(index, rect.size(), ghost, |row| {
                                        tracks.get(row).cloned()
                                    });
                                }
                                None => {}
                            }
                        }
                        selection.finish(ui.ctx());
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
    use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};
    use takofy_model::TrackEntry;

    use super::*;
    use crate::message::Update;
    use crate::theme::{fonts, space, style};
    use crate::view::test_support::click_events;

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
            artist_names: Vec::new(),
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
    fn frame(ctx: &Context, library: &mut Library) -> (Option<Command>, egui::FullOutput) {
        frame_with(ctx, library, Vec::new())
    }

    /// One frame of the page with one pass's events.
    fn frame_with(
        ctx: &Context,
        library: &mut Library,
        events: Vec<Event>,
    ) -> (Option<Command>, egui::FullOutput) {
        let tokens = Tokens::DARK;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        art.begin_frame();
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, PROBE)),
            events,
            ..Default::default()
        };
        let mut command = None;
        let mut output = ctx.run_ui(input, |ui| {
            command = search_view(
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
        (command, output)
    }

    #[test]
    fn a_right_click_posts_the_results_menu() {
        let ctx = context();
        let mut library = answered("a", 2);
        let (_, output) = frame(&ctx, &mut library);
        let at = text_rect(&output, "t1")
            .expect("the second result is painted")
            .center();

        for event in crate::view::test_support::secondary_click_events(at) {
            let _ = frame_with(&ctx, &mut library, vec![event]);
        }
        let request = crate::view::context_menu::take_request(&ctx)
            .expect("a right-click on a result posted a request");
        assert_eq!(request.targets.len(), 1);
        assert_eq!(request.targets[0].uri, "t1");
        assert_eq!(request.page, Some(Page::Search("a".to_owned())));
    }

    /// The rect a painted label occupies, read back from the frame.
    fn text_rect(output: &egui::FullOutput, label: &str) -> Option<Rect> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some(Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
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
        let lines = painted_lines(&frame(&ctx, &mut library).1);

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
            let lines = painted_lines(&frame(&ctx, &mut library).1);

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
        let lines = painted_lines(&frame(&ctx, &mut library).1);

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

    #[test]
    fn a_single_click_selects_and_a_double_click_plays_it() {
        let ctx = context();
        let mut library = answered("never gonna", 2);
        let (_, output) = frame(&ctx, &mut library);
        let row = text_rect(&output, "t1").expect("the row's title");

        let mut command = None;
        for event in click_events(row.center()) {
            command = frame_with(&ctx, &mut library, vec![event]).0;
        }

        assert_eq!(command, None, "a single click plays nothing");
        assert!(
            crate::view::carry::List::open(
                &ctx,
                egui::Id::unique(("search-tracks", "never gonna")),
                2
            )
            .look(1)
            .selected,
            "the row under the click is picked"
        );
        assert!(
            !crate::view::carry::List::open(
                &ctx,
                egui::Id::unique(("search-tracks", "never gonna")),
                2
            )
            .look(0)
            .selected,
            "and only that row"
        );

        let mut command = None;
        // A fresh page for the double click: a third click inside the window is
        // a triple click, and that plays nothing.
        let ctx = context();
        let mut library = answered("never gonna", 2);
        for _ in 0..2 {
            for event in click_events(row.center()) {
                command = frame_with(&ctx, &mut library, vec![event]).0;
            }
        }

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "t1".to_owned()
            }),
            "a double click plays the track"
        );
    }
}
