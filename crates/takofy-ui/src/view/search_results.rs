//! The search results page: the whole answer, one block per kind under its
//! query's heading.
//!
//! `Results for “q”` is the page head, with the note
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

use crate::cover::{Art, want_px};
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
    let want = want_px(crate::theme::extent::LIBRARY_ART, ui.pixels_per_point());

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
            cover: Some(stores.art.look(entry.cover.as_ref(), want, stores.now)),
            look: selection.look(index),
            menu: gate.allows(),
            liked: stores.library.likes.state(&entry.uri),
        };
        // A row plays the whole answer, entered here, so next and previous
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
mod tests;
