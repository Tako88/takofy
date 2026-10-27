//! The album page's body: the reference's album band and one album table.
//!
//! The band says which record this is, and the table lists its tracks in the
//! album's own order, numbered by their place in that one continuous list. No
//! per-row art is drawn: the band holds the record's one picture, so a row is the
//! name in the title column with its credit in the artist column.
//!
//! The page draws only what the library already has. `Load::Idle` has nothing to
//! say because the route asks for the album when it opens, so it draws nothing at
//! all rather than a second spinner. A ready record that does not name this page
//! is another album's, so it draws the spinner instead of that record's band.

use std::time::Instant;

use egui::{Id, Sense, Ui};

use crate::cover::{Art, want_px};
use crate::format::{album_meta, format_position, total_duration_ms};
use crate::layout::actionbar::Kind;
use crate::layout::{Table, table as table_layout};
use crate::library::{Library, Load};
use crate::menu::LibraryItem;
use crate::message::Command;
use crate::player::Transport;
use crate::queue::Queue;
use crate::route::Page;
use crate::scroll;
use crate::sidebar_kind::SidebarKind;
use crate::theme::{Tokens, extent};
use crate::view::text as recipe;

use crate::verbs;

use super::actionbar;
use super::carry::{self, Ghost};
use super::context_menu;
use super::heart;
use super::hero;
use super::link::Target;
use super::pin;
use super::skeleton;
use super::track_list::{head, list_rect};
use super::track_row::{self, Click, Row, state_of};

/// Draws the album page and returns the command its controls asked for.
#[expect(
    clippy::too_many_arguments,
    reason = "the pane, its table, the state it draws and the frame's cover store"
)]
pub fn album_view(
    ui: &mut Ui,
    table: Table,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Option<Command> {
    // The page's own URI, so Retry asks for the album this page is on rather than
    // whichever one happens to be in the load state.
    let Some(Page::Album(uri)) = library.page() else {
        return None;
    };
    let uri = uri.clone();
    // The page a row's menu is about, so a verb that leads here is omitted and a
    // page change can close the menu.
    let here = Page::Album(uri.clone());
    let writing = library.write_in_flight();

    // No leading gap here: `tracks_view` adds the page's one gap before it
    // dispatches, and a second would start the album a step lower than every
    // other page.
    let mut command = None;
    let mut retry = false;
    // A link click is applied after the scroll area, because the matches below
    // borrow the album it has to move away from.
    let mut open: Option<Page> = None;
    // The band's heart asks for the library, which the album is borrowed from.
    // The value the heart asked for, and the name the write needs.
    let mut save: Option<(String, bool)> = None;

    // The scroll area is salted with the album's own URI, so the offset it keeps
    // is this album's and not the playlist page's that shares the pane.
    egui::ScrollArea::vertical()
        .id_salt(uri.as_str())
        .show(ui, |ui| {
            scroll::follow_drag(ui);
            match &library.album {
                Load::Idle => {}
                Load::Loading => {
                    // The rows hold the table's own gutter, so the title does not
                    // shift when the list lands.
                    super::inner(ui, |ui| {
                        skeleton::rows(
                            ui,
                            skeleton::SCREENFUL,
                            table.columns().for_plays(),
                            false,
                            tokens,
                        );
                    });
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
                Load::Ready(_) => {
                    // A ready detail that does not name this page is another album's:
                    // the page keeps its loading state rather than painting that
                    // album's band and rows.
                    let Some(album) = library.current_album() else {
                        // The gutter is the loaded table's, so nothing shifts
                        // when this page's own album arrives.
                        super::inner(ui, |ui| {
                            skeleton::rows(
                                ui,
                                skeleton::SCREENFUL,
                                table.columns().for_plays(),
                                false,
                                tokens,
                            );
                        });
                        return;
                    };
                    // The one cover lookup on this page: the rows carry none, so the
                    // band's picture is the only fetch a frame can ask for.
                    let want = want_px(crate::theme::hero::ART, ui.pixels_per_point());
                    let cover = art.look(album.cover.as_ref(), want, now);
                    let tint = art.tint_held(album.cover.as_ref(), want, &tokens.palette.tint);
                    let meta = album_meta(
                        album.year,
                        (album.tracks.len(), total_duration_ms(&album.tracks)),
                    );
                    let credit = hero::Credit::new(
                        &album.artists,
                        album.artist_uri.as_deref().map(Target::Credit),
                    );
                    let band = hero::Band {
                        eyebrow: "Album",
                        title: &album.name,
                        meta,
                        credit,
                        // An album's label and copyright are out of this slice, so
                        // there is no second byline to draw.
                        description: "",
                        cover,
                        tint,
                    };
                    let reply = hero::show_with(ui, &band, tokens, hero::Title::Plain);
                    open = open.take().or(reply.open);
                    let saved = library.is_saved(&uri);
                    let acted = actionbar::show(
                        ui,
                        &actionbar::Bar {
                            title: Some(&album.name),
                            playable: !album.tracks.is_empty(),
                            // The reference's album action bar has like and more,
                            // not shuffle.
                            shuffle: false,
                            more: Some(!writing),
                            saved,
                            kind: Kind::Other,
                            text: None,
                        },
                        None,
                        tokens,
                    );
                    if let Some(door) = acted.more_door {
                        let item = LibraryItem {
                            kind: SidebarKind::Albums,
                            uri: uri.clone(),
                            name: album.name.clone(),
                        };
                        context_menu::post(
                            ui.ctx(),
                            verbs::library_request(
                                item,
                                door,
                                verbs::LibraryRights {
                                    renamable: false,
                                    removable: saved == Some(true),
                                    saved,
                                },
                                Some(Page::Album(uri.clone())),
                                crate::naming::Surface::Band,
                            ),
                        );
                    }
                    if acted.played && !album.tracks.is_empty() {
                        command =
                            queue.start(&album.tracks, 0, Some(album.uri.as_str()), transport);
                    }
                    if let Some(asked) = acted.saved {
                        // The library is borrowed for the album for the whole
                        // draw, so the write is asked for once the borrow ends.
                        save = Some((album.name.clone(), asked));
                    }

                    super::inner(ui, |ui| {
                        let width = ui.available_width();
                        if album.tracks.is_empty() {
                            ui.label(recipe::muted("No tracks on this album.", tokens));
                            return;
                        }

                        // The album's own columns, solved once and handed to the head and
                        // every row, so the two cannot disagree about where a column is.
                        let columns = table_layout::columns(width, table.columns().for_plays());
                        head(ui, columns, tokens);
                        let count = album.tracks.len();
                        // The whole list is reserved at once, so the scrollbar is right,
                        // and only the rows the viewport can show are laid out and given
                        // interaction.
                        let list = list_rect(ui, count);
                        ui.allocate_rect(list, Sense::hover());
                        let visible = ui.clip_rect();
                        let mut selection = carry::List::open(
                            ui.ctx(),
                            Id::unique(("album-tracks", album.uri.as_str())),
                            count,
                        );
                        let gate = selection.gate(writing);
                        // The id the rows draw under, so a focused row is read from
                        // the same `Ui` that drew them.
                        let focused_now = ui.memory(|memory| memory.focused());
                        let mut reveal = None;

                        for (index, entry) in album.tracks.iter().enumerate() {
                            let rect = table_layout::row_rect(list, index, extent::ROW_H);
                            if !table_layout::on_screen(visible, rect) {
                                // A row the cull skipped draws nothing, so it has
                                // no reveal of its own; one that holds the keyboard
                                // is asked for once the loop is done.
                                if focused_now == Some(track_row::id(ui, &entry.uri, index + 1)) {
                                    reveal = Some(rect);
                                }
                                continue;
                            }
                            let duration = entry.duration_ms.map(format_position);
                            let row = Row {
                                uri: &entry.uri,
                                // The number is the position in this one continuous list,
                                // which is what makes a multi-disc album read 1..N.
                                number: index + 1,
                                title: &entry.title,
                                artist: &entry.artists,
                                // The artist cell opens the artist it names, which
                                // on a compilation or a feature is not the album's.
                                artist_uri: entry.artist_uris.first().map(String::as_str),
                                // No album cell and no added date in an album's own table.
                                album: None,
                                album_uri: None,
                                added: None,
                                duration: duration.as_deref(),
                                state: state_of(&entry.uri, transport),
                                // The band already holds the record's one picture,
                                // so the rows carry no art box of their own.
                                cover: None,
                                look: selection.look(index),
                                menu: gate.allows(),
                                liked: library.likes.state(&entry.uri),
                            };
                            // A row plays; its artist cell, when the album named a
                            // resolvable artist, opens that artist's page.
                            match track_row::show(ui, rect, columns, &row, tokens) {
                                Some(Click::Play) => {
                                    command = queue.start(
                                        &album.tracks,
                                        index,
                                        Some(album.uri.as_str()),
                                        transport,
                                    );
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
                                    |row| album.tracks.get(row).cloned(),
                                ),
                                Some(Click::Lift) => {
                                    let ghost = Ghost::Table(row.lifted(columns));
                                    selection.lift(index, rect.size(), ghost, |row| {
                                        album.tracks.get(row).cloned()
                                    });
                                }
                                None => {}
                            }
                        }
                        // A focused row the cull skipped is asked for again through
                        // the seam, which shifts the request clear of the action row
                        // pinned above the list; the row's own reveal knows nothing
                        // of it and would leave the row behind it.
                        if let Some(row) = reveal
                            && row.top() < ui.clip_rect().top()
                        {
                            pin::reveal(ui, row);
                        }
                        selection.finish(ui.ctx());
                    });
                }
            }
        });

    if retry {
        return library.open(Page::Album(uri));
    }
    if let Some(page) = open {
        return library.open(page);
    }
    if let Some((name, asked)) = save {
        let item = LibraryItem {
            kind: SidebarKind::Albums,
            uri: uri.clone(),
            name,
        };
        if command.is_none() {
            command = library.set_saved(&item, asked);
        }
    }
    command
}

#[cfg(test)]
mod tests;
