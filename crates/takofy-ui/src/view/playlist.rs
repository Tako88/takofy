//! The main pane's tracklist: the selected playlist's tracks, or the state of
//! their load.
//!
//! The header and the rows are solved from one width and one set of tokens, so the
//! columns line up by construction rather than because two pieces agree. The header
//! scrolls with the list, which is what the reference's tracklist does.

use std::time::Instant;

use egui::{Align, Color32, Id, Layout, Sense, Ui, UiBuilder, vec2};
use takofy_model::PlaylistEntry;

use crate::cover::{Art, Cover, want_px};
use crate::format::{added_label, byline, format_position, plain_description, total_duration_ms};
use crate::landing::Zone;
use crate::layout::{
    Table, actionbar as actionbar_layout, section as section_layout, table as table_layout,
};
use crate::library::{BandCover, Library, Load, TracksFor, band_cover};
use crate::menu::{Cut, LibraryItem};
use crate::message::Command;
use crate::mosaic::{self, Kept};
use crate::naming::Surface;
use crate::player::Transport;
use crate::queue::Queue;
use crate::route::Page;
use crate::scroll;
use crate::sidebar_kind::SidebarKind;
use crate::theme::{TintParams, Tokens, extent, space};

use super::actionbar;
use super::carry::{self, Ghost};
use super::context_menu;
use super::heart;
use super::hero;
use super::pin;
use super::playlist_filter;
use super::skeleton;
use super::text as recipe;
use super::track_list::{head, list_rect};
use super::track_row::{self, Click, Row, state_of};
use crate::verbs::{self, LibraryRights};

pub(in crate::view) mod root;

use root::idle;
pub use root::{FollowedRead, IdleView, LoadNote, idle_view};

/// The main pane: the selected playlist's tracks, or the state of their load.
#[expect(
    clippy::too_many_arguments,
    reason = "the pane, its table, the state it draws and the frame's clocks"
)]
pub fn tracks_view(
    ui: &mut Ui,
    table: Table,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
    now: Instant,
    now_ms: u64,
    tokens: &Tokens,
) -> Option<Command> {
    // Dispatch on the page, not on the tracks: the root is the grid, and the
    // tracks may still hold a ready playlist that nothing draws.
    match main_page(library) {
        MainPage::Root => return super::inner(ui, |ui| idle(ui, library, art, now, tokens)),
        MainPage::Album => {
            return super::album::album_view(
                ui, table, library, transport, queue, art, now, tokens,
            );
        }
        MainPage::Artist => {
            return super::artist::artist_view(
                ui, table, library, transport, queue, art, now, tokens,
            );
        }
        MainPage::Show => {
            return super::show::show_view(ui, library, transport, queue, art, now, now_ms, tokens);
        }
        MainPage::Search => {
            return super::search_results::search_view(
                ui, table, library, transport, queue, art, now, tokens,
            );
        }
        MainPage::Playlist => {}
    }

    // Read before the rows, which borrow the tracks these ask about.
    let movable = library.movable();
    let liftable = library.liftable();
    let writing = library.write_in_flight();
    let mut command = None;
    let mut retry = false;
    // A link click is applied after the scroll area, because the matches below
    // borrow the tracks it has to move away from.
    let mut open: Option<Page> = None;
    // The count is a strip below the list's viewport, not a line after the rows:
    // a long list would scroll it out of sight. The strip takes room only when
    // there is something to say, so a fully named list reserves nothing.
    let note = match &library.tracks {
        Load::Ready(tracks) => crate::format::unnamed_note(&tracks.tracks),
        Load::Idle | Load::Loading | Load::Failed(_) => None,
    };
    let page = ui.available_rect_before_wrap();
    let strip = note.as_ref().map(|_| {
        let font = egui::TextStyle::Body.resolve(ui.style());
        let line = ui.ctx().fonts_mut(|fonts| fonts.row_height(&font));
        section_layout::foot(page, line + space::S1).1
    });
    let mut area = egui::ScrollArea::vertical();
    if let Some(strip) = strip {
        area = area.max_height((strip.top() - page.top()).max(0.0));
    }
    area.show(ui, |ui| {
        scroll::follow_drag(ui);
        // The band heads the playlist whenever the library can name the
        // selection; with no name there is nothing to say above the rows. It
        // takes the width and starts at the cursor, so the header and the rows
        // follow it down one scroll area.
        let entry = library.selected_entry().cloned();
        let renamable = entry
            .as_ref()
            .is_some_and(|entry| library.can_rename(&entry.uri));
        if let Some(entry) = &entry {
            let (cover, picture, tint) = band_picture(
                entry,
                &library.tracks,
                &mut library.mosaic_kept,
                art,
                ui.pixels_per_point(),
                &tokens.palette.tint,
                now,
            );
            let tracks = match &library.tracks {
                Load::Ready(tracks) if !tracks.tracks.is_empty() => {
                    Some((tracks.tracks.len(), total_duration_ms(&tracks.tracks)))
                }
                Load::Idle | Load::Loading | Load::Failed(_) | Load::Ready(_) => None,
            };
            // The owner and the description come from the listing; the count and
            // the total wait for the tracks. The description is owned here so the
            // band can borrow it for as long as it is drawn.
            let meta = byline(entry.owner.as_ref(), tracks);
            // The description is drawn as the wire sends it: whitespace is
            // collapsed, but markup it carries (entities, `spotify:` links)
            // draws literally.
            let description = entry
                .description
                .as_deref()
                .and_then(plain_description)
                .unwrap_or_default();
            let renaming = std::mem::take(&mut library.renaming);
            let mut renaming = renaming;
            let title = if renaming.is_open_on(&entry.uri, Surface::Band) {
                hero::Title::Editing(&mut renaming)
            } else if renamable && !carry::carrying(ui.ctx()) {
                hero::Title::Clickable
            } else {
                hero::Title::Plain
            };
            let band = hero::Band {
                // The eyebrow stays Playlist: the wire has no list visibility.
                // `ItemAttributes.public` exists but is per item, this client
                // never reads it, and it says nothing about the list.
                eyebrow: "Playlist",
                title: &entry.name,
                meta,
                credit: None,
                description: &description,
                cover,
                tint,
            };
            let reply = hero::show_picture(ui, &band, picture, tokens, title);
            library.renaming = renaming;
            // The band takes a hand-back only for its own field: a row's return
            // belongs to the row, and a field raised on another page has no
            // title here to go back to.
            if library.renaming.wants_return()
                && library.renaming.surface() == Surface::Band
                && library.renaming.uri() == entry.uri
            {
                if let Some(id) = reply.title_id {
                    ui.memory_mut(|memory| memory.request_focus(id));
                }
                library.renaming.return_taken();
            }
            if reply.rename_clicked {
                library.begin_rename(&entry.uri, Surface::Band);
            }
        }

        // The row of actions under the band. It holds its place in the flow and
        // pins to the top of the page's viewport, so it is drawn here and the
        // rows below it are clipped clear of it.
        //
        // It is drawn for a playlist the library can name and for one it cannot,
        // as long as there are tracks to act on: with no name the row is the
        // only way to play the list or to filter it.
        let ready = match &library.tracks {
            Load::Ready(tracks) if !tracks.tracks.is_empty() => Some(tracks),
            Load::Idle | Load::Loading | Load::Failed(_) | Load::Ready(_) => None,
        };
        let mut hits = None;
        if entry.is_some() || ready.is_some() {
            // Read before the field takes the query: the heart and the menu ask
            // the library about the whole context.
            let saved = entry
                .as_ref()
                .and_then(|entry| library.is_saved(&entry.uri));
            let more_live =
                entry.is_some() && !carry::carrying(ui.ctx()) && !library.write_in_flight();
            let kind = actionbar_layout::Kind::Playlist(actionbar_layout::Form::at(
                ui.ctx().content_rect().width(),
                table,
            ));
            let playable = ready.is_some();
            let filter = ready.map(|tracks| playlist_filter::Filter {
                state: &mut library.track_filter,
                playlist: &tracks.playlist,
                entries: &tracks.tracks,
            });
            let acted = actionbar::show(
                ui,
                &actionbar::Bar {
                    title: entry.as_ref().map(|entry| entry.name.as_str()),
                    playable,
                    shuffle: true,
                    more: entry.as_ref().map(|_| more_live),
                    saved,
                    kind,
                    text: None,
                },
                filter,
                tokens,
            );
            hits = acted.hits;

            if let Some(entry) = &entry
                && let Some(door) = acted.more_door
            {
                let item = LibraryItem {
                    kind: SidebarKind::Playlists,
                    uri: entry.uri.clone(),
                    name: entry.name.clone(),
                };
                let rights = LibraryRights {
                    renamable,
                    removable: library.can_remove(SidebarKind::Playlists),
                    // The row's heart offers the same save the row's menu does.
                    saved,
                };
                context_menu::post(
                    ui.ctx(),
                    verbs::library_request(
                        item,
                        door,
                        rights,
                        library.page().cloned(),
                        Surface::Band,
                    ),
                );
            }
            if let Some(entry) = &entry
                && acted.saved.is_some()
            {
                let item = LibraryItem {
                    kind: SidebarKind::Playlists,
                    uri: entry.uri.clone(),
                    name: entry.name.clone(),
                };
                command = library.save_to_library(&item).or(command.take());
            }
            if (acted.played || acted.shuffled)
                && playable
                && let Load::Ready(tracks) = &library.tracks
            {
                command = (if acted.shuffled {
                    queue.start_with_shuffle(
                        &tracks.tracks,
                        0,
                        Some(tracks.playlist.as_str()),
                        transport,
                    )
                } else {
                    queue.start(&tracks.tracks, 0, Some(tracks.playlist.as_str()), transport)
                })
                .or(command.take());
            }
        }

        super::inner(ui, |ui| {
            let width = ui.available_width();
            // A rename can fail on a list with no rows, so its sentence is drawn
            // above the table arms rather than inside the non-empty one.
            let notice = match &library.tracks {
                Load::Ready(tracks) => tracks.notice,
                Load::Idle | Load::Loading | Load::Failed(_) => None,
            };
            if let Some(failure) = notice {
                ui.label(recipe::muted(
                    crate::signin::failure_message(failure),
                    tokens,
                ));
            }
            match &library.tracks {
                Load::Idle => {}
                Load::Loading => {
                    // The listing already names how many tracks there are, so the
                    // page holds that many rows open rather than a screenful.
                    let count = skeleton::row_count(
                        library.selected_entry().and_then(|entry| entry.tracks),
                    );
                    skeleton::rows(ui, count, table.columns().for_playlist(true), true, tokens);
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
                Load::Ready(tracks) if tracks.tracks.is_empty() => {
                    ui.label(recipe::muted("No tracks in this playlist.", tokens));
                }
                Load::Ready(tracks) => {
                    let columns = table_layout::columns(
                        width,
                        table
                            .columns()
                            .for_playlist(table_layout::is_playlist(&tracks.playlist)),
                    );
                    // A failed like is drawn here and nowhere else, so a flip
                    // started on an album, artist, search or release-block
                    // heart reverts without a word; a page change clears what
                    // is left.
                    if let Some(failure) = library.likes.notice() {
                        ui.label(recipe::muted(
                            crate::signin::failure_message(failure),
                            tokens,
                        ));
                    }
                    let count = tracks.tracks.len();
                    let shown = hits.as_ref().map_or(count, Vec::len);
                    // The page is in the id: a selection belongs to a page, not to a row index.
                    let list_id = Id::unique(("playlist-tracks", tracks.playlist.as_str()));
                    // Only a page that may be written offers the destructive
                    // verb and the key; a list that is not ours has neither.
                    let listing = if movable {
                        carry::Listing::Playlist {
                            uri: tracks.playlist.clone(),
                            list: list_id,
                        }
                    } else {
                        carry::Listing::Page
                    };
                    let mut selection = carry::List::open(ui.ctx(), list_id, count);
                    // A filtered list has no hidden picks: they are pruned before
                    // a menu, a lift or the Delete gate can read them.
                    let keep = |row: usize| {
                        hits.as_ref()
                            .is_none_or(|hits| hits.binary_search(&row).is_ok())
                    };
                    selection.restrict(keep);
                    // The menu's gate is the list's: a carried block or a write in
                    // flight holds the rows still, so no menu opens over them.
                    let gate = selection.gate(writing);

                    if hits.as_ref().is_some_and(|hits| hits.is_empty()) {
                        // Nothing matches: the head and the rows would only
                        // repeat the note, so the note stands in their place.
                        let query = library
                            .track_filter
                            .edit(&tracks.playlist)
                            .trim()
                            .to_owned();
                        ui.label(recipe::muted(&crate::format::list_nothing(&query), tokens));
                        selection.restrict(|_| false);
                        selection.finish(ui.ctx());
                    } else {
                        head(ui, columns, tokens);
                        // The whole list is reserved at once, so the scrollbar is right,
                        // and only the rows the viewport can show are laid out and given
                        // interaction: a thousand-track playlist paints the dozen rows on
                        // screen.
                        let list = list_rect(ui, shown);
                        ui.allocate_rect(list, Sense::hover());
                        let visible = ui.clip_rect();

                        // The id the rows draw under, so a focused row is read from
                        // the same `Ui` that drew them.
                        let focused_now = ui.memory(|memory| memory.focused());
                        let mut focused = None;
                        // The focused row's box, read before the clip culls it, so
                        // a row wholly under the pin can still be brought out.
                        let mut reveal = None;

                        for slot in 0..shown {
                            let index = hits.as_ref().map_or(slot, |hits| hits[slot]);
                            let entry = &tracks.tracks[index];
                            let rect = table_layout::row_rect(list, slot, extent::ROW_H);
                            let holds_keyboard =
                                focused_now == Some(track_row::id(ui, &entry.uri, index + 1));
                            if !table_layout::on_screen(visible, rect) {
                                // A row the cull skipped draws nothing, so it has
                                // no reveal of its own; one that holds the keyboard
                                // is asked for once the loop is done.
                                if holds_keyboard {
                                    reveal = Some(rect);
                                }
                                continue;
                            }
                            if holds_keyboard {
                                focused = Some(index);
                            }
                            let duration = entry.duration_ms.map(format_position);
                            let added = entry.added_at.and_then(|added| added_label(added, now_ms));
                            let row = Row {
                                uri: &entry.uri,
                                number: index + 1,
                                title: &entry.title,
                                artist: &entry.artists,
                                artist_uri: entry.artist_uris.first().map(String::as_str),
                                album: entry.album.as_deref(),
                                album_uri: entry.album_uri.as_deref(),
                                added: added.as_deref(),
                                duration: duration.as_deref(),
                                state: state_of(&entry.uri, transport),
                                cover: Some(art.look(
                                    entry.cover.as_ref(),
                                    want_px(
                                        crate::theme::extent::LIBRARY_ART,
                                        ui.pixels_per_point(),
                                    ),
                                    now,
                                )),
                                look: selection.look(index),
                                menu: gate.allows(),
                                liked: library.likes.state(&entry.uri),
                            };
                            match track_row::show(ui, rect, columns, &row, tokens) {
                                Some(Click::Play) => {
                                    command = queue.start(
                                        &tracks.tracks,
                                        index,
                                        Some(tracks.playlist.as_str()),
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
                                        listing: listing.clone(),
                                        here: library.page(),
                                        likes: &library.likes,
                                    },
                                    |row| tracks.tracks.get(row).cloned(),
                                ),
                                // A write in flight holds the page still; a page it
                                // may not write still lifts.
                                Some(Click::Lift) if liftable => {
                                    let ghost = Ghost::Table(row.lifted(columns));
                                    selection.lift(index, rect.size(), ghost, |row| {
                                        tracks.tracks.get(row).cloned()
                                    });
                                }
                                Some(Click::Lift) => {}
                                None => {}
                            }
                        }
                        // `rows::body` brings a focused row into view knowing
                        // nothing of what pins above the list: it asks for the
                        // row's whole rect at the scroll area's own top edge, so
                        // the row comes to rest behind the pinned rows, with
                        // only the bottom sliver of its 44 points showing and no
                        // scroll left to bring it out. A row the cull skipped —
                        // one that starts above the region the rows may paint in
                        // — is asked for again through the seam, which shifts
                        // the request clear of everything pinned.
                        if let Some(row) = reveal
                            && row.top() < ui.clip_rect().top()
                        {
                            pin::reveal(ui, row);
                        }
                        // A span a shift-click opened in true index space may
                        // name hidden rows; prune it before Del reads it.
                        selection.restrict(keep);
                        // Del belongs to a focused row of a page that may be
                        // written, and only while the menu is not holding the key:
                        // a consumed event is gone, so the menu's own Del would
                        // starve behind it.
                        if movable
                            && gate.allows()
                            && !context_menu::is_open(ui.ctx())
                            && let Some(row) = focused
                            && context_menu::take_delete(ui)
                        {
                            let rows = selection.acting_rows(row);
                            let entries: Vec<takofy_model::TrackEntry> = rows
                                .iter()
                                .filter_map(|row| tracks.tracks.get(*row).cloned())
                                .collect();
                            if rows.len() == entries.len() {
                                context_menu::post_cut(
                                    ui.ctx(),
                                    Cut {
                                        listing: listing.clone(),
                                        rows,
                                        entries,
                                    },
                                );
                            }
                        }
                        // Only a page that may take the write says where its own
                        // rows land, and only while no query narrows what the
                        // user sees: a filtered drop would name a place the
                        // hidden rows make ambiguous.
                        let reorderable = movable && hits.is_none();
                        if reorderable {
                            selection.offer(
                                &tracks.playlist,
                                Zone {
                                    hit: list.intersect(visible),
                                    list,
                                    row_h: extent::ROW_H,
                                    first: 0,
                                    count,
                                },
                            );
                        }
                        selection.finish(ui.ctx());
                    }
                }
            }
        });
    });

    if let (Some(note), Some(strip)) = (note.as_deref(), strip) {
        let line = strip.shrink2(vec2(space::GUTTER, 0.0));
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(line)
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.label(recipe::muted(note, tokens));
            },
        );
    }

    if retry {
        let playlist = library.selected().map(str::to_owned);
        return playlist.map(|playlist| library.select(&playlist));
    }
    if let Some(page) = open {
        return library.open(page);
    }
    command
}

/// The cover the band draws this frame.
///
/// Once the tracks have settled this is the entry's own cover. While they load
/// it is `Pending`, but a texture the store already holds is kept on screen
/// rather than flashing the placeholder: a reload that re-selected the same
/// playlist shows the picture it already had, and only an entry with nothing
/// held draws `Loading`.
fn band_cover_for(
    entry: &PlaylistEntry,
    settled: bool,
    art: &mut Art,
    want: u32,
    params: &TintParams,
    now: Instant,
) -> (Cover, Option<Color32>) {
    let tint = art.tint_held(entry.cover_set(), want, params);
    let cover = match band_cover(entry, settled) {
        BandCover::Pending => entry
            .cover_set()
            .and_then(|cover| art.look_held(Some(cover), want, now))
            .unwrap_or(Cover::Loading),
        BandCover::Entry(cover) => art.look(cover, want, now),
    };
    (cover, tint)
}

/// The cover, the picture and the tint the band draws this frame.
///
/// A ready list for this page asks for the first four distinct album covers and
/// remembers them; a list that asks for what the memo already holds leaves it
/// alone. Any other state keeps the remembered URLs without fetching, so a
/// reload shows the mosaic it already held rather than one cover and a second
/// round of fetches.
///
/// The quadrants are looked at first, and the single cover only once they cannot
/// tile the box: a band drawing the mosaic does not also fetch and pin the
/// picture it replaced. The cover is therefore unread while the mosaic draws,
/// and the first quadrant stands in for its tint.
fn band_picture(
    entry: &PlaylistEntry,
    tracks: &Load<TracksFor>,
    kept: &mut Option<Kept>,
    art: &mut Art,
    pixels_per_point: f32,
    params: &TintParams,
    now: Instant,
) -> (Cover, hero::Picture, Option<Color32>) {
    let settled = !matches!(tracks, Load::Loading);
    let want = want_px(crate::theme::hero::ART, pixels_per_point);
    let half = want.div_ceil(2);
    let ready = match tracks {
        Load::Ready(ready) if ready.playlist == entry.uri => Some(ready),
        Load::Idle | Load::Loading | Load::Failed(_) | Load::Ready(_) => None,
    };
    if let Some(ready) = ready {
        let urls = mosaic::wanted(
            entry.cover.as_ref().map(|cover| cover.origin),
            &mosaic::urls(&ready.tracks),
        );
        if kept.as_ref().and_then(|kept| kept.urls_for(&entry.uri)) != Some(urls.as_slice()) {
            *kept = Some(Kept::new(entry.uri.clone(), urls));
        }
    }
    let wanted = kept
        .as_ref()
        .and_then(|kept| kept.urls_for(&entry.uri))
        .unwrap_or_default();
    let drawn: Vec<Cover> = wanted
        .iter()
        .map(|cover| match ready {
            Some(_) => art.look(Some(cover), half, now),
            None => art
                .look_held(Some(cover), half, now)
                .unwrap_or(Cover::Loading),
        })
        .collect();
    match mosaic::arrange(&drawn) {
        Some(quadrants) => (
            quadrants[0],
            hero::Picture::Mosaic(quadrants),
            wanted
                .first()
                .and_then(|cover| art.tint_held(Some(cover), half, params)),
        ),
        None => {
            let (cover, tint) = band_cover_for(entry, settled, art, want, params, now);
            (cover, hero::Picture::Cover, tint)
        }
    }
}

/// What the page on screen asks the main pane to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MainPage {
    /// The library root: the playlists grid.
    Root,
    /// A playlist's tracklist.
    Playlist,
    /// An album's band and table.
    Album,
    /// An artist's hero, Popular list and Discography.
    Artist,
    /// A show's band and its episodes.
    Show,
    /// A search's results, under their query's heading.
    Search,
}

/// Reads the pane's dispatch from the current page.
fn main_page(library: &Library) -> MainPage {
    match library.page() {
        None => MainPage::Root,
        Some(Page::Playlist(_)) => MainPage::Playlist,
        Some(Page::Album(_)) => MainPage::Album,
        Some(Page::Artist(_)) => MainPage::Artist,
        Some(Page::Show(_)) => MainPage::Show,
        Some(Page::Search(_)) => MainPage::Search,
    }
}

#[cfg(test)]
mod tests;
