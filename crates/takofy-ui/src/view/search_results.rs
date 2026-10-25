//! The search results page: the whole answer, one block per kind under its
//! query's heading.
//!
//! The brief's R15, redrawn: `Results for “q”` is the page head, with the note
//! that says the cap once (`Top 20 tracks`) and nothing else. Under it come up to
//! three blocks — Tracks as the tracklist this app already draws, Albums and
//! Artists as the library's card grid — each capped at ten and each ending in a
//! reveal row while it hides something. All three are derived from the whole
//! answer, never from the rows on screen, so one block expanding cannot move the
//! others.
//!
//! The page draws only what the search state already holds for its own query.
//! That guard is not decoration: a `Ready` answer left over from the query the
//! reader typed past must not be painted under this heading.

use std::time::Instant;

use egui::{FontId, Id, RichText, Sense, Ui};

use crate::cover::Art;
use crate::format::{format_position, search_note};
use crate::layout::{Table, table as table_layout};
use crate::library::Library;
use crate::message::Command;
use crate::player::Transport;
use crate::queue::Queue;
use crate::route::Page;
use crate::scroll;
use crate::search::{PageListing, PageView, SEARCH_CAP};
use crate::search_index::{Band, Index, Kind, PAGE};
use crate::theme::{Tokens, Weight, extent, fonts, space, text};
use crate::view::text as recipe;

use super::carry::{self, Ghost};
use super::heart;
use super::list_more;
use super::page_head;
use super::search_cards;
use super::section;
use super::skeleton;
use super::track_list::{head, list_rect};
use super::track_row::{self, Click, Row, state_of};

/// The frame's stores, so the page's blocks take one of them and one argument.
struct Stores<'a> {
    table: Table,
    transport: &'a mut Transport,
    queue: &'a mut Queue,
    art: &'a mut Art,
    now: Instant,
    /// The page a row's menu is about, so a verb that leads here is omitted.
    here: &'a Page,
    library: &'a Library,
    tokens: &'a Tokens,
    /// The write gate: a menu that would write waits for the one in flight.
    writing: bool,
}

/// What one frame of the page asked for.
///
/// A reveal and a page change are applied after the scroll area, because the
/// blocks borrow the answer they have to move away from.
#[derive(Default)]
struct Asked {
    command: Option<Command>,
    open: Option<Page>,
    /// A block's reveal was taken: show the rest, and move the keyboard to the
    /// item it reveals first.
    more: Option<(Kind, Id)>,
    retry: bool,
}

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
    let here = Page::Search(query.clone());
    let writing = library.write_in_flight();
    let mut asked = Asked::default();
    let mut stores = Stores {
        table,
        transport,
        queue,
        art,
        now,
        here: &here,
        library: &*library,
        tokens,
        writing,
    };

    // No leading gap here: `tracks_view` adds the page's one gap before it
    // dispatches, and a second would start the page a step lower than every
    // other one.
    //
    // The scroll area is salted with the query and its visit, so a page that
    // starts fresh also starts at the top; a Retry or a re-entry of the same
    // query keeps its offset. `album.rs` solves the same problem the same way.
    let visit = library.search.page_visit();
    egui::ScrollArea::vertical()
        .id_salt(("search", query.as_str(), visit))
        .show(ui, |ui| {
            scroll::follow_drag(ui);
            // The page is drawn only for its own query. The state is pointed at
            // it when the page opens, so this is the one guard that keeps a
            // superseded answer off the page. Reaching it with no read at all is
            // a bug, not a state to draw blank.
            let library: &Library = stores.library;
            let view = library.search.page_view(&query);
            debug_assert!(
                view.is_some(),
                "the search page {query:?} has no read of its own"
            );
            let Some(view) = view else {
                return;
            };
            // The heading sits in the reference's gutter, like every section
            // head; the page has no band, so nothing above it is full-bleed.
            super::inner(ui, |ui| body(ui, &view, &query, &mut stores, &mut asked));
        });

    if asked.retry {
        return library.reload_search();
    }
    if let Some((kind, focus)) = asked.more
        && library.search.take_more(&query, kind)
    {
        // The row the reveal was pressed on takes itself away, so the keyboard
        // continues on the first item the reveal shows instead of falling back
        // to the top of the page. Only a reveal that was taken has a row to
        // move to.
        ui.ctx().memory_mut(|memory| memory.request_focus(focus));
    }
    if let Some(page) = asked.open {
        return library.open(page);
    }
    asked.command
}

/// The page's body: the head, then whichever of the four states is true.
fn body(ui: &mut Ui, view: &PageView<'_>, query: &str, stores: &mut Stores<'_>, asked: &mut Asked) {
    let tokens = stores.tokens;
    // The note is the page's one statement of the cap: a full answer says
    // `Top 20 tracks`, a short one its own count, and nothing true to count
    // paints nothing.
    let note = match view {
        PageView::Ready(listing) => search_note(listing.answer.len(), SEARCH_CAP),
        PageView::Empty => search_note(0, SEARCH_CAP),
        PageView::Waiting | PageView::Failed(_) => String::new(),
    };
    let title = format!("Results for “{query}”");
    page_head::show(ui, &title, &note, tokens);

    match view {
        PageView::Ready(listing) => blocks(ui, listing, query, stores, asked),
        // The loading page reserves exactly what the answer will draw, so
        // nothing jumps when it lands.
        PageView::Waiting => loading(ui, stores),
        PageView::Failed(failure) => {
            ui.label(recipe::muted(
                crate::signin::failure_message(*failure),
                tokens,
            ));
            if ui.button("Retry").clicked() {
                asked.retry = true;
            }
        }
        PageView::Empty => empty(ui, tokens),
    }
}

/// The answered page's blocks, divided by the reference's section rule.
fn blocks(
    ui: &mut Ui,
    listing: &PageListing<'_>,
    query: &str,
    stores: &mut Stores<'_>,
    asked: &mut Asked,
) {
    let tokens = stores.tokens;
    for (position, band) in listing.shape.bands.iter().enumerate() {
        if position > 0 {
            section::divide(ui, tokens);
        }
        match band.kind {
            Kind::Tracks => tracks(ui, listing, band, query, stores, asked),
            Kind::Albums | Kind::Artists => {
                let act = search_cards::show(
                    ui,
                    band,
                    listing.index,
                    &stores.library.search,
                    stores.art,
                    stores.now,
                    tokens,
                );
                match act {
                    Some(search_cards::Act::Open(index)) => {
                        asked.open = page_of(band.kind, listing.index, index);
                    }
                    Some(search_cards::Act::More { focus }) => {
                        asked.more = Some((band.kind, focus));
                    }
                    None => {}
                }
            }
        }
    }
}

/// The track block: the tracklist, its rows, and the reveal row under them.
fn tracks(
    ui: &mut Ui,
    listing: &PageListing<'_>,
    band: &Band,
    query: &str,
    stores: &mut Stores<'_>,
    asked: &mut Asked,
) {
    let tokens = stores.tokens;
    section::head_spoken(ui, band.heading(), &band.share(), &band.spoken(), tokens);

    let width = ui.available_width();
    // A search answer is not a playlist's membership, so the date column goes;
    // every other column is the playlist's own.
    let present = stores.table.columns().for_playlist(false);
    let columns = table_layout::columns(width, present);
    head(ui, columns, tokens);

    // Only the rows the block shows are laid out and given interaction; the
    // list is reserved at the shown height, which is what a reveal grows.
    let shown = band.shown.min(listing.answer.len());
    let list = list_rect(ui, shown);
    ui.allocate_rect(list, Sense::hover());
    let visible = ui.clip_rect();
    let mut selection = carry::List::open(ui.ctx(), Id::unique(("search-tracks", query)), shown);
    let gate = selection.gate(stores.writing);

    for (index, entry) in listing.answer.iter().take(shown).enumerate() {
        let rect = table_layout::row_rect(list, index, extent::ROW_H);
        if !table_layout::on_screen(visible, rect) {
            continue;
        }
        let duration = entry.duration_ms.map(format_position);
        let row = Row {
            uri: &entry.uri,
            number: index + 1,
            title: &entry.title,
            artist: &entry.artists,
            artist_uri: entry.artist_uris.first().map(String::as_str),
            album: entry.album.as_deref(),
            album_uri: entry.album_uri.as_deref(),
            // A search result was not added to anything.
            added: None,
            duration: duration.as_deref(),
            state: state_of(&entry.uri, stores.transport),
            cover: Some(stores.art.look(entry.cover.as_ref(), stores.now)),
            look: selection.look(index),
            menu: gate.allows(),
            liked: stores.library.likes.state(&entry.uri),
        };
        // R16: a row plays the whole answer, entered here, so next and previous
        // walk the results. A merged answer is no single page, so it names no
        // context for a move to carry.
        match track_row::show(ui, rect, columns, &row, tokens) {
            Some(Click::Play) => {
                asked.command = stores
                    .queue
                    .start(listing.answer, index, None, stores.transport);
            }
            Some(Click::Open(page)) => asked.open = Some(page),
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
                    here: Some(stores.here),
                    likes: &stores.library.likes,
                },
                |row| listing.answer.get(row).cloned(),
            ),
            Some(Click::Lift) => {
                let ghost = Ghost::Table(row.lifted(columns));
                selection.lift(index, rect.size(), ghost, |row| {
                    listing.answer.get(row).cloned()
                });
            }
            None => {}
        }
    }
    selection.finish(ui.ctx());

    if list_more::show(ui, list, band.more().as_deref(), space::S3, tokens)
        && let Some(next) = listing.answer.get(shown)
    {
        // The first row the reveal shows is the one the keyboard goes to.
        asked.more = Some((Kind::Tracks, track_row::id(ui, &next.uri, shown + 1)));
    }
}

/// The loading page's three blocks, in the answered page's rhythm and at the
/// heights it will draw.
fn loading(ui: &mut Ui, stores: &Stores<'_>) {
    let tokens = stores.tokens;
    let present = stores.table.columns().for_playlist(false);
    for (position, kind) in [Kind::Tracks, Kind::Albums, Kind::Artists]
        .into_iter()
        .enumerate()
    {
        if position > 0 {
            section::divide(ui, tokens);
        }
        // Heads are painted and carry no node: there is nothing true to
        // announce yet.
        section::head(ui, kind.heading(), "", tokens);
        match kind {
            Kind::Tracks => {
                let columns = table_layout::columns(ui.available_width(), present);
                // The column-head band is reserved with the rows, so the page
                // does not move when the answer lands.
                head(ui, columns, tokens);
                skeleton::rows(ui, PAGE.tracks, present, true, tokens);
            }
            Kind::Albums | Kind::Artists => search_cards::loading(ui, kind, tokens),
        }
    }
}

/// The empty answer's two lines, under the head that still says what was asked.
fn empty(ui: &mut Ui, tokens: &Tokens) {
    let (title, hint) = crate::format::search_empty_copy();
    ui.add_space(space::S5);
    ui.label(
        RichText::new(title)
            .font(FontId::new(text::MD, fonts::sans(Weight::Semibold)))
            .color(tokens.palette.text),
    );
    ui.add_space(space::S2);
    ui.label(
        RichText::new(hint)
            .font(FontId::new(text::BASE, fonts::sans(Weight::Regular)))
            .color(tokens.palette.text_muted),
    );
}

/// The page a card of `kind` at `position` opens.
fn page_of(kind: Kind, index: &Index, position: usize) -> Option<Page> {
    match kind {
        Kind::Albums => index
            .albums
            .get(position)
            .map(|album| Page::Album(album.uri.clone())),
        Kind::Artists => index
            .artists
            .get(position)
            .map(|artist| Page::Artist(artist.uri.clone())),
        Kind::Tracks => None,
    }
}

#[cfg(test)]
mod tests {
    use egui::{Context, Event, FullOutput, Pos2, RawInput, Rect, Vec2};
    use takofy_model::{CoverUrl, TrackEntry};

    use super::*;
    use crate::message::{Failure, Update};
    use crate::theme::{fonts, space, style};
    use crate::view::test_support::click_events;

    /// The probe window: wide enough for the page's table and tall enough for
    /// every block.
    const PROBE: Vec2 = Vec2::new(900.0, 4_000.0);

    fn context() -> Context {
        let tokens = Tokens::DARK;
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));
        ctx.enable_accesskit();
        ctx
    }

    fn cover(index: usize) -> CoverUrl {
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{index}")).expect("a cover url")
    }

    /// One answer track for `index`, on `album` and by `artist`.
    fn track(index: usize, album: Option<usize>, artist: Option<usize>) -> TrackEntry {
        TrackEntry {
            uri: format!("spotify:track:{index}"),
            title: format!("t{index}"),
            artists: artist
                .map(|artist| format!("Artist {artist}"))
                .unwrap_or_default(),
            album: album.map(|album| format!("Album {album}")),
            album_uri: album.map(|album| format!("spotify:album:{album}")),
            artist_uris: artist
                .map(|artist| vec![format!("spotify:artist:{artist}")])
                .unwrap_or_default(),
            artist_names: artist
                .map(|artist| vec![format!("Artist {artist}")])
                .unwrap_or_default(),
            added_at: None,
            duration_ms: None,
            named: true,
            cover: Some(cover(index)),
        }
    }

    /// The results page for `query`, answered with `tracks` tracks spread over
    /// `albums` albums and `artists` artists, cycling.
    fn page(query: &str, tracks: usize, albums: usize, artists: usize) -> Library {
        let answer = (0..tracks)
            .map(|index| {
                track(
                    index,
                    (albums > 0).then(|| index % albums),
                    (artists > 0).then(|| index % artists),
                )
            })
            .collect();
        answered_with(query, answer)
    }

    /// The results page for `query`, answered with `count` bare tracks.
    fn answered(query: &str, count: usize) -> Library {
        answered_with(
            query,
            (0..count).map(|index| track(index, None, None)).collect(),
        )
    }

    /// The results page for `query`, answered with `answer`.
    fn answered_with(query: &str, answer: Vec<TrackEntry>) -> Library {
        let mut library = Library::default();
        let _ = library.open(Page::Search(query.to_owned()));
        // The frame that lands on the page is also the one that records it as
        // the surface a portrait reply is judged against. It runs before the
        // answer, so there is no window to ask about yet.
        let _ = library.search.request_portraits(Some(query));
        library.apply(&Update::Search {
            query: query.to_owned(),
            tracks: answer,
        });
        library
    }

    /// One frame of the page, at a fixed clock.
    fn frame(ctx: &Context, library: &mut Library) -> (Option<Command>, FullOutput) {
        frame_with(ctx, library, Vec::new())
    }

    /// One frame of the page with one pass's events.
    fn frame_with(
        ctx: &Context,
        library: &mut Library,
        events: Vec<Event>,
    ) -> (Option<Command>, FullOutput) {
        let mut queue = Queue::default();
        frame_with_queue(ctx, library, &mut queue, events)
    }

    /// One frame of the page, with a queue the caller keeps across frames.
    fn frame_with_queue(
        ctx: &Context,
        library: &mut Library,
        queue: &mut Queue,
        events: Vec<Event>,
    ) -> (Option<Command>, FullOutput) {
        let tokens = Tokens::DARK;
        let mut transport = Transport::default();
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
                queue,
                &mut art,
                Instant::now(),
                &tokens,
            );
        });
        output.textures_delta.clear();
        (command, output)
    }

    /// One frame of the page's body, without the scroll area, and the room it
    /// took.
    fn body_frame(ctx: &Context, library: &mut Library, query: &str) -> (f32, FullOutput) {
        let tokens = Tokens::DARK;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        art.begin_frame();
        let here = Page::Search(query.to_owned());
        let writing = library.write_in_flight();
        let mut asked = Asked::default();
        let now = Instant::now();
        let mut stores = Stores {
            table: Table::Full,
            transport: &mut transport,
            queue: &mut queue,
            art: &mut art,
            now,
            here: &here,
            library: &*library,
            tokens: &tokens,
            writing,
        };
        let library: &Library = stores.library;
        let view = library
            .search
            .page_view(query)
            .expect("the page holds its own read");
        let mut advance = 0.0;
        let mut output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, PROBE)),
                ..Default::default()
            },
            |ui| {
                let top = ui.cursor().top();
                body(ui, &view, query, &mut stores, &mut asked);
                advance = ui.cursor().top() - top;
            },
        );
        output.textures_delta.clear();
        (advance, output)
    }

    /// The fetches one frame of the page asked for.
    fn fetched(ctx: &Context, library: &mut Library) -> Vec<Command> {
        let tokens = Tokens::DARK;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let now = Instant::now();
        art.begin_frame();
        let mut output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, PROBE)),
                ..Default::default()
            },
            |ui| {
                let _ = search_view(
                    ui,
                    Table::Full,
                    library,
                    &mut transport,
                    &mut queue,
                    &mut art,
                    now,
                    &tokens,
                );
            },
        );
        output.textures_delta.clear();
        art.requests(now)
    }

    /// One press of the reveal row `label`, then a quiet frame. Answers where
    /// the press landed and what the page drew afterwards.
    fn take(ctx: &Context, library: &mut Library, label: &str) -> (Pos2, FullOutput) {
        let (_, output) = frame(ctx, library);
        let at = text_rect(&output, label)
            .unwrap_or_else(|| panic!("{label:?} was painted"))
            .center();
        for event in click_events(at) {
            let _ = frame_with(ctx, library, vec![event]);
        }
        (at, frame(ctx, library).1)
    }

    /// A click at `at`, in the same three passes every other view's test uses.
    fn click(ctx: &Context, library: &mut Library, at: Pos2) -> Option<Command> {
        let mut command = None;
        for event in click_events(at) {
            command = frame_with(ctx, library, vec![event]).0;
        }
        command
    }

    /// The rect a painted label occupies, read back from the frame.
    fn text_rect(output: &FullOutput, label: &str) -> Option<Rect> {
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
    fn painted_lines(output: &FullOutput) -> Vec<(String, f32)> {
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

    /// The label nodes a frame announced, as they read.
    fn labels(output: &FullOutput) -> Vec<String> {
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("accesskit is on")
            .nodes
            .iter()
            .filter(|(_, node)| node.role() == egui::accesskit::Role::Label)
            .filter_map(|(_, node)| node.value().map(str::to_owned))
            .collect()
    }

    fn painted(output: &FullOutput, line: &str) -> bool {
        painted_lines(output)
            .iter()
            .any(|(painted, _)| painted == line)
    }

    #[test]
    fn the_head_states_the_answer_and_the_cap_once() {
        let ctx = context();
        let mut library = page("q", 20, 2, 2);
        let lines = painted_lines(&frame(&ctx, &mut library).1);
        assert!(
            lines.iter().any(|(line, _)| line == "Results for “q”"),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|(line, _)| line == "TOP 20 TRACKS"),
            "{lines:?}"
        );

        let mut one = answered("never gonna", 1);
        assert!(painted(&frame(&ctx, &mut one).1, "1 TRACK"));

        let mut none = answered("never gonna", 0);
        assert!(painted(&frame(&ctx, &mut none).1, "0 TRACKS"));
    }

    #[test]
    fn three_blocks_hide_their_rest_behind_one_row_each() {
        let ctx = context();
        let mut library = page("q", 25, 12, 11);
        let output = frame(&ctx, &mut library).1;
        for share in ["10 OF 25", "10 OF 12", "10 OF 11"] {
            assert!(painted(&output, share), "the block head reads {share:?}");
        }
        for row in [
            "Show 15 more tracks",
            "Show 2 more albums",
            "Show 1 more artist",
        ] {
            assert!(painted(&output, row), "the reveal row reads {row:?}");
        }
    }

    #[test]
    fn taking_the_track_reveal_shows_the_rest_and_leaves_the_others_alone() {
        let ctx = context();
        let mut library = page("q", 25, 12, 11);
        let (at, output) = take(&ctx, &mut library, "Show 15 more tracks");

        assert!(painted(&output, "t24"), "the last row is drawn");
        assert!(painted(&output, "25"), "the head reads the total alone");
        assert!(
            !painted(&output, "Show 15 more tracks"),
            "the row took itself away"
        );
        assert!(painted(&output, "10 OF 12"), "the album block did not move");
        assert!(
            painted(&output, "10 OF 11"),
            "the artist block did not move"
        );

        // Nothing is left to ask for, so a second press on the place the row was
        // changes nothing.
        let _ = click(&ctx, &mut library, at);
        let after = frame(&ctx, &mut library).1;
        assert!(painted(&after, "t24"));
        assert!(painted(&after, "25"));
        assert!(
            !painted(&after, "Show 15 more tracks"),
            "and no row comes back"
        );
    }

    #[test]
    fn a_new_query_starts_at_ten_and_the_same_query_keeps_what_was_shown() {
        let ctx = context();
        let mut library = page("q", 25, 0, 0);
        let (_, output) = take(&ctx, &mut library, "Show 15 more tracks");
        assert!(painted(&output, "t24"));

        // Coming back to the same query keeps it, because the page still holds
        // the read the reveal belongs to.
        let _ = library.open(Page::Search("q".to_owned()));
        assert!(painted(&frame(&ctx, &mut library).1, "t24"));

        // Another query starts every block at ten again.
        let _ = library.open(Page::Search("other".to_owned()));
        library.apply(&Update::Search {
            query: "other".to_owned(),
            tracks: (0..25).map(|index| track(index, None, None)).collect(),
        });
        let other = frame(&ctx, &mut library).1;
        assert!(painted(&other, "10 OF 25"), "the new query caps at ten");
        assert!(painted(&other, "Show 15 more tracks"));

        // And the first query, entered again after that detour, starts at ten
        // too: a reveal belongs to the read on screen, and that read is gone.
        let _ = library.open(Page::Search("q".to_owned()));
        library.apply(&Update::Search {
            query: "q".to_owned(),
            tracks: (0..25).map(|index| track(index, None, None)).collect(),
        });
        let back = frame(&ctx, &mut library).1;
        assert!(painted(&back, "10 OF 25"));
        assert!(painted(&back, "Show 15 more tracks"));
    }

    #[test]
    fn a_kind_the_answer_lacks_is_not_drawn() {
        let ctx = context();
        let mut library = page("q", 3, 0, 0);
        let output = frame(&ctx, &mut library).1;
        assert!(painted(&output, "Tracks"), "the track block is drawn");
        assert!(
            !painted(&output, "Albums"),
            "an empty album band is no block"
        );
        assert!(!painted(&output, "Artists"));
    }

    #[test]
    fn exactly_one_node_carries_the_note() {
        let ctx = context();
        let mut library = page("q", 20, 2, 2);
        let output = frame(&ctx, &mut library).1;
        let carrying: Vec<_> = labels(&output)
            .into_iter()
            .filter(|label| label.contains("Top 20 tracks"))
            .collect();
        assert_eq!(
            carrying,
            ["Results for “q”, Top 20 tracks"],
            "the note is spoken once, in the page head"
        );
    }

    #[test]
    fn an_album_card_opens_its_page() {
        let ctx = context();
        let mut library = page("q", 3, 3, 0);
        let output = frame(&ctx, &mut library).1;
        let at = text_rect(&output, "Album 1")
            .expect("the album card's name")
            .center();
        assert_eq!(
            click(&ctx, &mut library, at),
            Some(Command::LoadAlbum {
                album: "spotify:album:1".to_owned()
            })
        );
    }

    #[test]
    fn an_artist_card_opens_its_page() {
        let ctx = context();
        let mut library = page("q", 3, 0, 3);
        let output = frame(&ctx, &mut library).1;
        let at = text_rect(&output, "Artist 2")
            .expect("the artist card's name")
            .center();
        assert_eq!(
            click(&ctx, &mut library, at),
            Some(Command::LoadArtist {
                artist: "spotify:artist:2".to_owned()
            })
        );
    }

    #[test]
    fn a_row_still_plays_the_whole_answer_at_its_own_index() {
        let ctx = context();
        let mut library = page("q", 25, 12, 11);
        let output = frame(&ctx, &mut library).1;
        let at = text_rect(&output, "t8").expect("the ninth row").center();
        let mut command = None;
        // A fresh page for the double click: a third click inside the window is a
        // triple click, and that plays nothing.
        for _ in 0..2 {
            for event in click_events(at) {
                command = frame_with(&ctx, &mut library, vec![event]).0;
            }
        }
        assert_eq!(
            command,
            Some(Command::Play {
                uri: "spotify:track:8".to_owned()
            }),
            "the row plays its own place in the whole answer"
        );
    }

    #[test]
    fn a_search_results_play_carries_no_context() {
        let ctx = context();
        let mut library = answered("q", 2);
        let mut queue = Queue::default();
        let (_, output) = frame_with_queue(&ctx, &mut library, &mut queue, Vec::new());
        let row = text_rect(&output, "t1").expect("the row's title");

        for _ in 0..2 {
            for event in click_events(row.center()) {
                let _ = frame_with_queue(&ctx, &mut library, &mut queue, vec![event]);
            }
        }

        assert!(queue.current().is_some(), "the double click started a row");
        assert_eq!(
            queue.origin(),
            None,
            "a merged answer is no page, so it names no context"
        );
    }

    #[test]
    fn the_empty_answer_keeps_the_head_and_draws_two_lines() {
        let ctx = context();
        let mut library = answered("nothing", 0);
        let lines = painted_lines(&frame(&ctx, &mut library).1);
        let texts: Vec<&str> = lines.iter().map(|(line, _)| line.as_str()).collect();
        assert!(texts.contains(&"Results for “nothing”"), "{texts:?}");
        assert!(texts.contains(&"0 TRACKS"), "{texts:?}");
        assert!(
            texts.contains(&"Nothing in the catalogue matches that"),
            "{texts:?}"
        );
        assert!(
            texts.contains(&"Check the spelling, drop the year, or search an artist instead."),
            "{texts:?}"
        );
        assert!(
            !texts.contains(&"Retry"),
            "an empty answer offers nothing: {texts:?}"
        );
    }

    #[test]
    fn the_failed_read_keeps_the_head_and_offers_retry() {
        let ctx = context();
        let mut library = Library::default();
        let _ = library.open(Page::Search("q".to_owned()));
        library.apply(&Update::SearchFailed {
            query: "q".to_owned(),
            failure: Failure::Search,
        });
        let output = frame(&ctx, &mut library).1;
        let lines = painted_lines(&output);
        let texts: Vec<&str> = lines.iter().map(|(line, _)| line.as_str()).collect();
        assert!(texts.contains(&"Results for “q”"), "{texts:?}");
        assert!(
            texts.contains(&crate::signin::failure_message(Failure::Search)),
            "{texts:?}"
        );
        assert!(texts.contains(&"Retry"), "{texts:?}");
    }

    #[test]
    fn the_loading_page_reserves_what_ten_of_each_will_draw() {
        let ctx = context();
        // Nothing answered yet, so the page waits.
        let mut loading = Library::default();
        let _ = loading.open(Page::Search("q".to_owned()));
        let (waiting, output) = body_frame(&ctx, &mut loading, "q");
        assert!(
            painted(&output, "TITLE") && painted(&output, "ALBUM"),
            "the column-head band is reserved: {:?}",
            painted_lines(&output)
        );

        // Exactly ten of each draws no reveal row, so the two frames reserve the
        // same room: the loading state adds nothing and takes nothing away.
        let mut ready = page("q", 10, 10, 10);
        let (drawn, _) = body_frame(&ctx, &mut ready, "q");
        assert!(
            (waiting - drawn).abs() < 0.5,
            "the loading page reserved {waiting}, the answered one {drawn}"
        );

        // More than ten reveals the reveal rows' own room.
        let mut more = page("q", 25, 12, 11);
        let (grown, _) = body_frame(&ctx, &mut more, "q");
        assert!(grown > drawn);
    }

    /// The first row a track reveal shows is where the keyboard lands.
    #[test]
    fn a_track_reveal_moves_the_keyboard_to_the_first_new_row() {
        let ctx = context();
        let mut library = page("q", 25, 0, 0);
        let _ = take(&ctx, &mut library, "Show 15 more tracks").1;
        let (_, output) = frame(&ctx, &mut library);
        let focused = ctx
            .memory(|memory| memory.focused())
            .expect("the reveal moved the keyboard");
        let rect = ctx
            .read_response(focused)
            .expect("the focused row drew this frame")
            .rect;
        let first = text_rect(&output, "t10")
            .expect("the eleventh row")
            .center();
        let last = text_rect(&output, "t9").expect("the tenth row").center();
        assert!(rect.contains(first), "focus is on the first new row");
        assert!(!rect.contains(last), "and not on the last old one");
    }

    /// The same for a card block.
    #[test]
    fn a_card_reveal_moves_the_keyboard_to_the_first_new_card() {
        let ctx = context();
        let mut library = page("q", 25, 12, 0);
        let _ = take(&ctx, &mut library, "Show 2 more albums").1;
        let (_, output) = frame(&ctx, &mut library);
        let focused = ctx
            .memory(|memory| memory.focused())
            .expect("the reveal moved the keyboard");
        let rect = ctx
            .read_response(focused)
            .expect("the focused card drew this frame")
            .rect;
        let first = text_rect(&output, "Album 10")
            .expect("the eleventh album")
            .center();
        let last = text_rect(&output, "Album 9")
            .expect("the tenth album")
            .center();
        assert!(rect.contains(first), "focus is on the first new card");
        assert!(!rect.contains(last), "and not on the last old one");
    }

    /// An answer whose rows carry no art and whose artists are credited only by
    /// rows the track block does not show, so an artist card is the only surface
    /// that can look its cover up.
    fn portrait_page(query: &str) -> Library {
        let answer = (0..20)
            .map(|index| {
                if index < 10 {
                    let mut entry = track(index, None, None);
                    entry.cover = None;
                    entry
                } else {
                    track(index, None, Some(index - 10))
                }
            })
            .collect();
        answered_with(query, answer)
    }

    /// The three faces an artist card can wear, read through the covers the
    /// frame fetched.
    #[test]
    fn an_artist_card_follows_the_portrait_rule() {
        let ctx = context();
        let portrait = cover(900);
        // The artist's record cover: the cover of the first track crediting it,
        // which is a row the track block does not show.
        let record = cover(10);

        // Unread: a raised slot that looks at nothing.
        let mut unread = portrait_page("q");
        assert!(
            fetched(&ctx, &mut unread).is_empty(),
            "an unread face looked a cover up"
        );

        // Absent: the record the match sits on.
        let mut absent = portrait_page("q");
        absent.apply(&Update::Portraits {
            query: "q".to_owned(),
            portraits: vec![takofy_model::ArtistPortrait {
                artist: "spotify:artist:0".to_owned(),
                portrait: None,
            }],
        });
        assert_eq!(
            fetched(&ctx, &mut absent),
            vec![Command::Fetch { url: record }],
            "an artist with no portrait shows the record's cover"
        );

        // Known: the portrait itself.
        let mut known = portrait_page("q");
        known.apply(&Update::Portraits {
            query: "q".to_owned(),
            portraits: vec![takofy_model::ArtistPortrait {
                artist: "spotify:artist:0".to_owned(),
                portrait: Some(portrait.clone()),
            }],
        });
        assert_eq!(
            fetched(&ctx, &mut known),
            vec![Command::Fetch {
                url: portrait.clone()
            }],
            "a known portrait is fetched instead"
        );
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
        assert_eq!(request.targets[0].uri, "spotify:track:1");
        assert_eq!(request.page, Some(Page::Search("a".to_owned())));
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
                uri: "spotify:track:1".to_owned()
            }),
            "a double click plays the track"
        );
    }
}
