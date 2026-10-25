//! The main pane's tracklist: the selected playlist's tracks, or the state of
//! their load.
//!
//! The header and the rows are solved from one width and one set of tokens, so the
//! columns line up by construction rather than because two pieces agree. The header
//! scrolls with the list, which is what the reference's tracklist does.

use std::time::Instant;

use egui::{Align, Id, Layout, Pos2, Rect, Sense, Ui, UiBuilder, vec2};
use takofy_model::{CoverUrl, Owner, PlaylistEntry};

use crate::cover::{Art, Cover};
use crate::format::{
    added_label, byline, format_position, library_kind, library_note, media_size, media_type,
    plain_description, root_empty_label, root_filter_label, root_loading_label, total_duration_ms,
};
use crate::landing::Zone;
use crate::layout::{Table, actionbar as actionbar_layout, cards, table as table_layout};
use crate::library::{BandCover, Library, Load, MediaLayout, TracksFor, band_cover};
use crate::library_filter::{self, Item, RootKind};
use crate::menu::{Cut, Door, LibraryItem};
use crate::message::{Command, Failure};
use crate::mosaic::{self, Kept};
use crate::naming::Surface;
use crate::player::Transport;
use crate::queue::Queue;
use crate::route::Page;
use crate::scroll;
use crate::sidebar_kind::SidebarKind;
use crate::theme::{Tokens, extent, space};

use super::actionbar;
use super::carry::{self, Ghost};
use super::context_menu;
use super::filter_row;
use super::heart;
use super::hero;
use super::media;
use super::page_head;
use super::pin;
use super::playlist_filter;
use super::skeleton;
use super::text as recipe;
use super::track_list::{head, list_rect};
use super::track_row::{self, Click, Row, state_of};
use super::view_toggle;
use crate::verbs::{self, LibraryRights};

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
        let height = line + space::S1;
        Rect::from_min_max(
            Pos2::new(page.left(), (page.bottom() - height).max(page.top())),
            page.max,
        )
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
            let (cover, picture) =
                band_picture(entry, &library.tracks, &mut library.mosaic_kept, art, now);
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
                    library.track_filter.clear();
                    ui.label(recipe::muted(
                        crate::signin::failure_message(*failure),
                        tokens,
                    ));
                    if ui.button("Retry").clicked() {
                        retry = true;
                    }
                }
                Load::Ready(tracks) if tracks.tracks.is_empty() => {
                    library.track_filter.clear();
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
                                cover: Some(art.look(entry.cover.as_ref(), now)),
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
fn band_cover_for(entry: &PlaylistEntry, settled: bool, art: &mut Art, now: Instant) -> Cover {
    match band_cover(entry, settled) {
        BandCover::Pending => entry
            .cover_url()
            .and_then(|url| art.look_held(url, now))
            .unwrap_or(Cover::Loading),
        BandCover::Entry(url) => art.look(url, now),
    }
}

/// The cover and the picture the band draws this frame.
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
/// and the first quadrant stands in for it.
fn band_picture(
    entry: &PlaylistEntry,
    tracks: &Load<TracksFor>,
    kept: &mut Option<Kept>,
    art: &mut Art,
    now: Instant,
) -> (Cover, hero::Picture) {
    let settled = !matches!(tracks, Load::Loading);
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
        .map(|url| match ready {
            Some(_) => art.look(Some(url), now),
            None => art.look_held(url, now).unwrap_or(Cover::Loading),
        })
        .collect();
    match mosaic::arrange(&drawn) {
        Some(quadrants) => (quadrants[0], hero::Picture::Mosaic(quadrants)),
        None => (
            band_cover_for(entry, settled, art, now),
            hero::Picture::Cover,
        ),
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

/// What the root's main view shows: the cards for the chosen chip, or the state
/// of the load that fetches them.
///
/// The main view draws the cards whenever the page is the root, whatever the
/// tracks hold: after a trip to the root a ready playlist is kept for the page
/// Back returns to, and nothing draws it. It is dropped or reloaded when a
/// playlist page is next entered. The root reads both loads, because its cards
/// are playlists and shows, and it never competes with a tracklist for a
/// selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdleView<'a> {
    /// The playlists are in flight, or have not been asked for yet.
    Loading,
    /// The playlist fetch failed; the failure is what the retry needs.
    Failed(Failure),
    /// The chosen chip has nothing to show.
    Empty(RootKind),
    /// These cards, and the state of the list read under them.
    Grid {
        items: Vec<Item<'a>>,
        note: LoadNote,
    },
}

/// What the root says about the list under its cards.
///
/// The list is a second read with its own failure: a failed playlist fetch is
/// the page's failure, while a failed followed read only costs that kind's
/// cards and gets one muted line and its own Retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadNote {
    /// The list arrived, or the chosen chip draws no followed list.
    None,
    /// The list is in flight; the cards will appear when it lands.
    Loading(FollowedRead),
    /// The list failed; the note names the read to restart.
    Failed(FollowedRead, Failure),
}

/// The followed read a chip shows, and the only ones the root reads under its
/// cards. The playlists chip reads no followed list, so it has no value here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowedRead {
    /// The followed shows, under All and Podcasts.
    Shows,
    /// The followed artists.
    Artists,
    /// The saved albums.
    Albums,
}

impl FollowedRead {
    /// What the root says under its cards while this read is in flight.
    fn loading(self) -> &'static str {
        root_loading_label(match self {
            Self::Shows => RootKind::Podcasts,
            Self::Artists => RootKind::Artists,
            Self::Albums => RootKind::Albums,
        })
    }

    /// The command that asks for this read again.
    fn reload(self, library: &mut Library) -> Command {
        match self {
            Self::Shows => library.reload_followed(),
            Self::Artists => library.reload_followed_artists(),
            Self::Albums => library.reload_saved_albums(),
        }
    }
}

/// Reads the root view from both loads and the chosen chip.
///
/// The two unstaged states are one `Loading`: a list nobody has asked for yet
/// and a list being fetched both have nothing to draw, and the reference gives
/// both the same treatment. A failed playlist load wins over every chip: with no
/// session there is nothing to narrow.
pub fn idle_view(library: &Library) -> IdleView<'_> {
    match &library.playlists {
        Load::Idle | Load::Loading => IdleView::Loading,
        Load::Failed(failure) => IdleView::Failed(*failure),
        Load::Ready(playlists) => {
            let chosen = library.root_filter.chosen();
            let shows = slice(&library.followed);
            let artists = slice(&library.followed_artists);
            let albums = slice(&library.saved_albums);
            let items = library_filter::items(chosen, playlists, shows, artists, albums);
            let note = note_for(library, chosen);
            if items.is_empty() && note == LoadNote::None {
                IdleView::Empty(chosen)
            } else {
                IdleView::Grid { items, note }
            }
        }
    }
}

/// The items of a list that has arrived, or none while it has not.
fn slice<T>(load: &Load<Vec<T>>) -> &[T] {
    match load {
        Load::Ready(items) => items.as_slice(),
        Load::Idle | Load::Loading | Load::Failed(_) => &[],
    }
}

/// The note under the root's cards for the chosen chip.
///
/// A chip that draws one followed list says what that read is doing. All draws
/// every one of them, so it names the first that is still owed, in the order the
/// cards are drawn.
fn note_for(library: &Library, chosen: RootKind) -> LoadNote {
    match chosen {
        RootKind::Playlists => LoadNote::None,
        RootKind::All => [
            index_note(FollowedRead::Shows, &library.followed),
            index_note(FollowedRead::Artists, &library.followed_artists),
            index_note(FollowedRead::Albums, &library.saved_albums),
        ]
        .into_iter()
        .find(|note| *note != LoadNote::None)
        .unwrap_or(LoadNote::None),
        RootKind::Podcasts => index_note(FollowedRead::Shows, &library.followed),
        RootKind::Artists => index_note(FollowedRead::Artists, &library.followed_artists),
        RootKind::Albums => index_note(FollowedRead::Albums, &library.saved_albums),
    }
}

/// What one followed read is doing, named by the chip it belongs to.
fn index_note<T>(read: FollowedRead, load: &Load<Vec<T>>) -> LoadNote {
    match load {
        Load::Idle | Load::Loading => LoadNote::Loading(read),
        Load::Failed(failure) => LoadNote::Failed(read, *failure),
        Load::Ready(_) => LoadNote::None,
    }
}

/// Draws the idle main view and returns the command its controls asked for.
///
/// This is the whole root wiring in one place: the page head, the chip row and
/// the cards scroll as one column, the way the reference's view does. A pressed
/// chip is applied after the scroll area, because the matches inside borrow the
/// cards it has to change.
fn idle(
    ui: &mut Ui,
    library: &mut Library,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Option<Command> {
    let mut choose: Option<RootKind> = None;
    let mut relayout: Option<MediaLayout> = None;
    let mut select: Option<String> = None;
    let mut open: Option<Page> = None;
    let mut play: Option<(String, SidebarKind)> = None;
    let mut retry_playlists = false;
    let mut retry: Option<FollowedRead> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            super::inner(ui, |ui| {
                let view = idle_view(library);
                let count = match &view {
                    IdleView::Grid { items, .. } => items.len(),
                    IdleView::Loading | IdleView::Failed(_) | IdleView::Empty(_) => 0,
                };
                let note = if count > 0 {
                    library_note(count)
                } else {
                    String::new()
                };
                page_head::show(ui, "Your Library", &note, tokens);

                let shown = filter_row::show_with(
                    ui,
                    tokens,
                    &library_filter::ROW,
                    library.root_filter.chosen(),
                    RootKind::control,
                    root_filter_label,
                    filter_row::RowSlot::Trailing(view_toggle::reserved()),
                );
                choose = shown.picked;
                if let Some(layout) = view_toggle::show(ui, tokens, shown.area, library.layout) {
                    relayout = Some(layout);
                }
                // Under the chips, where the page's own failures are said: the
                // press that failed is the retry, so there is no button.
                if let Some(failure) = library.play_notice {
                    ui.label(recipe::muted(
                        crate::signin::failure_message(failure),
                        tokens,
                    ));
                }

                match view {
                    IdleView::Loading => {
                        let solved = media_grid(ui);
                        let count = skeleton::SCREENFUL;
                        let list = grid_rect(ui, &solved, count);
                        ui.allocate_rect(list, egui::Sense::hover());
                        skeleton::tiles(ui, &solved, count, tokens);
                    }
                    IdleView::Failed(failure) => {
                        ui.label(recipe::muted(
                            crate::signin::failure_message(failure),
                            tokens,
                        ));
                        // The sidebar retries a failed playlist load with
                        // `reload`; the root offers the same recovery here.
                        if ui.button("Retry").clicked() {
                            retry_playlists = true;
                        }
                    }
                    IdleView::Empty(kind) => {
                        ui.label(recipe::muted(root_empty_label(kind), tokens));
                    }
                    IdleView::Grid { items, note } => {
                        grid(
                            ui,
                            &items,
                            library.layout,
                            &mut MediaDraw {
                                art,
                                now,
                                tokens,
                                library: &*library,
                                select: &mut select,
                                open: &mut open,
                                play: &mut play,
                            },
                        );
                        load_note(ui, note, tokens, &mut retry);
                    }
                }
            });
        });

    if let Some(layout) = relayout {
        library.layout = layout;
    }
    // The press is a want, not a command: the rows it needs are asked for by
    // `logic`, which runs before the next draw.
    if let Some((uri, kind)) = play
        && library.want_play(&uri, kind)
    {
        ui.ctx().request_repaint();
    }
    if let Some(kind) = choose {
        library.root_filter.choose(kind);
    }
    if retry_playlists {
        return Some(library.reload());
    }
    if let Some(read) = retry {
        return Some(read.reload(library));
    }
    if let Some(uri) = select {
        return select_playlist(library, &uri);
    }
    if let Some(page) = open {
        return library.open(page);
    }
    None
}

/// The saved things for the chosen chip, in the density the user chose.
///
/// One scroll area either way, so switching density keeps the place the reader
/// had reached. A tile selects its playlist or opens its page; a row does the
/// same, and its more button takes its own clicks.
fn grid(ui: &mut Ui, items: &[Item<'_>], layout: MediaLayout, draw: &mut MediaDraw<'_, '_>) {
    if layout == MediaLayout::List {
        return list(ui, items, draw);
    }

    let solved = media_grid(ui);
    let list = grid_rect(ui, &solved, items.len());
    // The whole grid is reserved at once, so the scrollbar is right, and only the
    // tiles the viewport can show are laid out and given interaction.
    ui.allocate_rect(list, egui::Sense::hover());
    let visible = ui.clip_rect();

    for (index, item) in items.iter().enumerate() {
        let rect = solved.card_rect(index);
        if !table_layout::on_screen(visible, rect) {
            continue;
        }
        let media = draw.media_of(item, layout);
        match media::tile(ui, rect, &media.as_media(), draw.tokens) {
            Some(media::Press::Open) => draw.activate(item, &media.uri),
            Some(media::Press::Play) => draw.want_play(&media),
            Some(media::Press::Menu(_)) | None => {}
        }
    }
}

/// The same items as rows, under the columns' names.
fn list(ui: &mut Ui, items: &[Item<'_>], draw: &mut MediaDraw<'_, '_>) {
    let width = ui.available_width();
    let height = space::S2 + media::head_height(ui) + space::S2;
    let (head_rect, _) = ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
    media::head(ui, head_rect, draw.tokens);

    let top = ui.cursor().min.y;
    let rows = Rect::from_min_size(
        Pos2::new(ui.cursor().min.x, top),
        vec2(width, items.len() as f32 * extent::ROW_H),
    );
    ui.allocate_rect(rows, egui::Sense::hover());
    let visible = ui.clip_rect();
    for (index, item) in items.iter().enumerate() {
        let rect = Rect::from_min_size(
            Pos2::new(rows.left(), rows.top() + index as f32 * extent::ROW_H),
            vec2(width, extent::ROW_H),
        );
        if !table_layout::on_screen(visible, rect) {
            continue;
        }
        let media = draw.media_of(item, MediaLayout::List);
        let words = media.as_media();
        match media::row(ui, rect, &words, draw.tokens) {
            Some(media::Press::Open) => draw.activate(item, &media.uri),
            Some(media::Press::Play) => draw.want_play(&media),
            Some(media::Press::Menu(door)) => draw.post_menu(ui.ctx(), &media, door),
            None => {}
        }
    }
}

/// Where a click on one item goes, and the frame's drawing context.
struct MediaDraw<'a, 'b> {
    art: &'a mut Art,
    now: Instant,
    tokens: &'a Tokens,
    library: &'b Library,
    select: &'b mut Option<String>,
    open: &'b mut Option<Page>,
    /// The object whose play was pressed, and the list it belongs to.
    play: &'b mut Option<(String, SidebarKind)>,
}

impl MediaDraw<'_, '_> {
    /// Records a press on an item's play. The want is taken after the scroll
    /// area, so the borrow of the items has ended by the time it is held.
    fn want_play(&mut self, media: &MediaWords) {
        *self.play = Some((media.uri.clone(), media.shelf));
    }

    /// What a click on one item does: a playlist selects, the rest open their
    /// page.
    fn activate(&mut self, item: &Item<'_>, uri: &str) {
        match item {
            Item::Playlist(_) => *self.select = Some(uri.to_owned()),
            Item::Show(_) => *self.open = Some(Page::Show(uri.to_owned())),
            Item::Artist(_) => *self.open = Some(Page::Artist(uri.to_owned())),
            Item::Album(_) => *self.open = Some(Page::Album(uri.to_owned())),
        }
    }

    /// The menu a row's `⋯` asked for: the sidebar row's own verbs, built for
    /// the item it names.
    fn post_menu(&self, ctx: &egui::Context, media: &MediaWords, door: Door) {
        let item = LibraryItem {
            kind: media.shelf,
            uri: media.uri.clone(),
            name: media.name.clone(),
        };
        let rights = LibraryRights {
            // The name field lives in the sidebar, so the root offers no rename.
            renamable: false,
            removable: self.library.can_remove(media.shelf),
            // Every root row comes from a loaded list, so there is nothing to
            // put back.
            saved: Some(true),
        };
        // The page is `None` at the root, and is read anyway so the request stays
        // right if the root is ever drawn over a page.
        context_menu::post(
            ctx,
            verbs::library_request(
                item,
                door,
                rights,
                self.library.page().cloned(),
                Surface::Row,
            ),
        );
    }

    /// One item's words and picture, owned so a borrow of the item is not needed.
    ///
    /// Only the grid composes: a four-up at a row's art column is mush, so a
    /// list never looks its quadrants up.
    fn media_of(&mut self, item: &Item<'_>, layout: MediaLayout) -> MediaWords {
        let (uri, name, kind, sub, size, round, cover, shelf) = match item {
            Item::Playlist(entry) => (
                &entry.uri,
                &entry.name,
                media_type(RootKind::Playlists),
                library_kind("Playlist", entry.owner.as_ref().and_then(Owner::name)),
                media_size(RootKind::Playlists, entry.tracks),
                false,
                entry.cover_url(),
                SidebarKind::Playlists,
            ),
            Item::Show(show) => (
                &show.uri,
                &show.name,
                media_type(RootKind::Podcasts),
                library_kind("Podcast", show.publisher.as_deref()),
                media_size(RootKind::Podcasts, show.episodes),
                false,
                show.cover.as_ref(),
                SidebarKind::Podcasts,
            ),
            Item::Artist(artist) => (
                &artist.uri,
                &artist.name,
                "Artist",
                "Artist".to_owned(),
                media_size(
                    RootKind::Artists,
                    artist.releases.map(|count| count as usize),
                ),
                true,
                artist.portrait.as_ref(),
                SidebarKind::Artists,
            ),
            Item::Album(album) => (
                &album.uri,
                &album.name,
                media_type(RootKind::Albums),
                library_kind(
                    "Album",
                    (!album.artists.is_empty()).then_some(album.artists.as_str()),
                ),
                media_size(RootKind::Albums, album.tracks.map(|count| count as usize)),
                false,
                album.cover.as_ref(),
                SidebarKind::Albums,
            ),
        };
        let composed = match (layout, item) {
            (MediaLayout::Grid, Item::Playlist(entry)) => entry.composed.as_slice(),
            (MediaLayout::Grid, _) | (MediaLayout::List, _) => &[],
        };
        MediaWords {
            uri: uri.clone(),
            name: name.clone(),
            kind,
            sub,
            size,
            round,
            shelf,
            playable: Library::playable(shelf),
            cover: self.art.look(cover, self.now),
            picture: self.composed_picture(composed),
        }
    }

    /// What a tile draws in its art box: the four covers it tiles, once every
    /// one is held, else the single picture the words already carry.
    fn composed_picture(&mut self, composed: &[CoverUrl]) -> hero::Picture {
        let drawn: Vec<Cover> = composed
            .iter()
            .map(|url| self.art.look(Some(url), self.now))
            .collect();
        match mosaic::arrange(&drawn) {
            Some(covers) => hero::Picture::Mosaic(covers),
            None => hero::Picture::Cover,
        }
    }
}

/// One item's words and pictures, owned so a borrow of the item is not needed.
struct MediaWords {
    uri: String,
    name: String,
    kind: &'static str,
    sub: String,
    size: String,
    round: bool,
    shelf: SidebarKind,
    playable: bool,
    cover: Cover,
    picture: hero::Picture,
}

impl MediaWords {
    fn as_media(&self) -> media::Media<'_> {
        media::Media {
            uri: &self.uri,
            name: &self.name,
            kind: self.kind,
            sub: &self.sub,
            size: &self.size,
            round: self.round,
            shelf: self.shelf,
            playable: self.playable,
            cover: self.cover,
            picture: self.picture,
        }
    }
}

/// The solved media grid for the current width.
///
/// The reference's library grid is `minmax(11rem, 1fr)`, which is wider than the
/// grid the album page's releases use.
pub(super) fn media_grid(ui: &mut Ui) -> cards::Grid {
    let area = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 0.0));
    cards::grid(
        area,
        extent::MEDIA_MIN_W,
        space::S3,
        media::extra_height(ui),
    )
}

/// The rect that holds every tile: the width the grid solved and the rows'
/// total height, gaps included.
fn grid_rect(ui: &mut Ui, grid: &cards::Grid, count: usize) -> Rect {
    let height = if count == 0 {
        0.0
    } else {
        let rows = grid.rows(count);
        rows as f32 * grid.card_rect(0).height() + (rows - 1) as f32 * space::S3
    };
    Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), height))
}

/// The note under the cards while the followed list is loading or failed.
///
/// The copy and the Retry are the same pair the playlists' failure draws, in the
/// same order: the words first, then the control that asks again.
fn load_note(ui: &mut Ui, note: LoadNote, tokens: &Tokens, retry: &mut Option<FollowedRead>) {
    match note {
        LoadNote::Loading(read) => {
            ui.add_space(space::S3);
            ui.label(recipe::muted(read.loading(), tokens));
        }
        LoadNote::Failed(read, failure) => {
            ui.add_space(space::S3);
            ui.label(recipe::muted(
                crate::signin::failure_message(failure),
                tokens,
            ));
            if ui.button("Retry").clicked() {
                *retry = Some(read);
            }
        }
        LoadNote::None => {}
    }
}

/// Starts a playlist's load from an idle-grid card click.
///
/// `Library::select` always starts a load and returns its command, and that is
/// all this does. The grid is drawn only at the root, so the card under a click
/// is never the playlist whose tracks are loading; there is nothing to suppress
/// here.
///
/// Going to the root is the one way the same playlist can be clicked again while
/// its first fetch is still coming back: it clears the stale-reply guard and the
/// in-flight load, and the library keeps no record of the request, so the second
/// click does start it again. The first reply is discarded as stale, so the
/// repeat costs a fetch and nothing else.
fn select_playlist(library: &mut Library, uri: &str) -> Option<Command> {
    Some(library.select(uri))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use egui::{
        Context, Event, Modifiers, MouseWheelSource, MouseWheelUnit, Pos2, RawInput, TouchPhase,
        Vec2,
    };

    use super::*;
    use crate::library::Library;
    use crate::message::Update;
    use crate::theme::{fonts, hero as band, stroke, style};
    use crate::view::rows::Look;
    use crate::view::track_row::State;
    use takofy_model::{
        AddedAt, AlbumDetail, CoverImage, CoverUrl, Owner, OwnerName, PlaylistAccess,
        PlaylistCover, PlaylistEntry, Revision, TrackEntry,
    };

    /// The probe window: 900 by 400, the size every bare-row test runs in.
    const PROBE: Vec2 = Vec2::new(900.0, 400.0);
    /// The frame's wall clock in the tests: 2023-11-14T22:13:20Z.
    const NOW_MS: u64 = 1_700_000_000_000;
    /// One day in milliseconds.
    const DAY: u64 = 86_400_000;
    /// A probe window tall enough to hold the band, the header and two rows in
    /// full, so a click under the band is not clipped away by the viewport.
    const TALL: Vec2 = Vec2::new(900.0, 800.0);

    fn input(events: Vec<Event>) -> RawInput {
        sized_input(PROBE, events)
    }

    fn sized_input(size: Vec2, events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            events,
            ..Default::default()
        }
    }

    /// Runs one pass, consumes the font-atlas delta it may have produced, and
    /// hands back the frame for reading.
    fn run(ctx: &Context, input: RawInput, draw: impl FnMut(&mut Ui)) -> egui::FullOutput {
        let mut output = ctx.run_ui(input, draw);
        output.textures_delta.clear();
        output
    }

    /// Every galley the frame painted: its text, the top of its box and its
    /// height. The band's stack is only visible in these numbers.
    fn drawn_galleys(output: &egui::FullOutput) -> Vec<(String, f32, f32)> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) => Some((
                    text.galley.text().to_owned(),
                    text.pos.y,
                    text.galley.size().y,
                )),
                _ => None,
            })
            .collect()
    }

    /// The top of the galley whose text is exactly `label`.
    fn galley_top(drawn: &[(String, f32, f32)], label: &str) -> f32 {
        drawn
            .iter()
            .find(|(text, _, _)| text == label)
            .unwrap_or_else(|| panic!("{label:?} was not painted: {drawn:?}"))
            .1
    }

    /// The height of the galley whose text is exactly `label`.
    fn galley_height(drawn: &[(String, f32, f32)], label: &str) -> f32 {
        drawn
            .iter()
            .find(|(text, _, _)| text == label)
            .unwrap_or_else(|| panic!("{label:?} was not painted: {drawn:?}"))
            .2
    }

    /// The galley whose text is exactly `label`, from whatever painted shapes
    /// hold it.
    fn galley_of(output: &egui::FullOutput, label: &str) -> std::sync::Arc<egui::Galley> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.galley.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{label:?} was not painted"))
    }

    /// Where the galley whose text is exactly `label` was painted.
    fn galley_at(output: &egui::FullOutput, label: &str) -> Pos2 {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => Some(text.pos),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{label:?} was not painted"))
    }

    /// The text the frame actually painted, in paint order.
    fn drawn_texts(output: &egui::FullOutput) -> Vec<&str> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            })
            .collect()
    }

    /// The row indices whose title the frame painted. A row that is laid out
    /// but clipped away paints nothing, so this is the row set the window shows.
    fn painted_rows(output: &egui::FullOutput) -> Vec<usize> {
        // A row the scroll area clips away is not painted: the table hands the
        // page a row that starts above the fold, and the clip is what keeps it
        // off the screen.
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) => {
                    let index = text.galley.text().strip_prefix('t')?.parse().ok()?;
                    let rect = Rect::from_min_size(text.pos, text.galley.size());
                    rect.intersects(clipped.clip_rect).then_some(index)
                }
                _ => None,
            })
            .collect()
    }

    /// The row a cover URL names, read from the tail the test's `track_with_cover`
    /// wrote into the URL itself.
    fn cover_row(url: &CoverUrl) -> usize {
        url.as_str()
            .rsplit('c')
            .next()
            .and_then(|tail| tail.parse().ok())
            .expect("a cover URL names its row")
    }

    fn track(uri: &str, title: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: title.to_owned(),
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

    /// A track whose album and first artist both name a page, so both cells link.
    fn linkable(uri: &str) -> TrackEntry {
        TrackEntry {
            artists: "Band".to_owned(),
            album: Some("Record".to_owned()),
            album_uri: Some("spotify:album:rec".to_owned()),
            artist_uris: vec!["spotify:artist:band".to_owned()],
            artist_names: vec!["Band".to_owned()],
            ..track(uri, "t0")
        }
    }

    /// One press of `key` with no modifiers.
    fn key(key: egui::Key) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    fn date(millis: u64) -> AddedAt {
        AddedAt::from_wire(&millis.to_string()).expect("a date above the floor")
    }

    fn dated(uri: &str, title: &str, added_at: Option<AddedAt>) -> TrackEntry {
        TrackEntry {
            added_at,
            ..track(uri, title)
        }
    }

    /// A library whose rows belong to `playlist`, with `tracks` loaded.
    fn library_selected(playlist: &str, tracks: Vec<TrackEntry>) -> Library {
        let mut library = Library::default();
        library.select(playlist);
        library.apply(&Update::Tracks {
            playlist: playlist.to_owned(),
            tracks,
            cover: None,
            access: None,
        });
        library
    }

    /// One frame of the tracklist at the test's fixed clock.
    fn tracklist_frame(ctx: &Context, table: Table, library: &mut Library) -> egui::FullOutput {
        let tokens = Tokens::DARK;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        run(ctx, input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                table,
                library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        })
    }

    /// One frame of the tracklist in a window of `size`.
    fn tracklist_frame_in(ctx: &Context, size: Vec2, library: &mut Library) -> egui::FullOutput {
        let tokens = Tokens::DARK;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        run(ctx, sized_input(size, Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        })
    }

    /// Draws one frame of the tracklist and returns the text it painted.
    fn painted_lines(ctx: &Context, table: Table, library: &mut Library) -> Vec<String> {
        drawn_texts(&tracklist_frame(ctx, table, library))
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    /// The visible run of every painted line: an elided line is read as what it
    /// shows, not as the string it was given.
    fn painted_runs(ctx: &Context, table: Table, library: &mut Library) -> Vec<String> {
        tracklist_frame(ctx, table, library)
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) => text.galley.rows.first(),
                _ => None,
            })
            .map(|row| row.row.text().to_owned())
            .collect()
    }

    /// The access a read hands a page: what it may change, and the version it was
    /// read at.
    fn access(editable: bool) -> Option<PlaylistAccess> {
        Some(PlaylistAccess {
            revision: Revision::from_wire(b"rev-1").expect("a non-empty revision"),
            editable,
        })
    }

    fn library_with(tracks: Vec<TrackEntry>) -> Library {
        let mut library = Library::default();
        library.select("spotify:playlist:a");
        library.apply(&Update::Tracks {
            playlist: "spotify:playlist:a".to_owned(),
            tracks,
            cover: None,
            access: access(true),
        });
        library
    }

    /// The same page, read at a revision the user may not write.
    fn read_only_library_with(tracks: Vec<TrackEntry>) -> Library {
        let mut library = Library::default();
        library.select("spotify:playlist:a");
        library.apply(&Update::Tracks {
            playlist: "spotify:playlist:a".to_owned(),
            tracks,
            cover: None,
            access: access(false),
        });
        library
    }

    /// The same selection, with the playlist list ready so the library can name
    /// it. `selected_name` is the switch the hero hangs on.
    fn library_named(tracks: Vec<TrackEntry>, name: &str) -> Library {
        let mut library = library_with(tracks);
        library.apply(&Update::Playlists(vec![PlaylistEntry {
            composed: Vec::new(),
            uri: "spotify:playlist:a".to_owned(),
            name: name.to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: None,
            access: None,
        }]));
        library
    }

    /// The same selection, with a playlist entry that carries a description and
    /// an owner, so the band has both to draw.
    fn library_detailed(
        tracks: Vec<TrackEntry>,
        name: &str,
        description: Option<&str>,
        owner: Option<&str>,
    ) -> Library {
        let mut library = library_with(tracks);
        library.apply(&Update::Playlists(vec![listed(name, description, owner)]));
        library
    }

    /// The selection listed but its tracks not asked for yet: the band draws
    /// before anything is ready.
    fn library_listed(name: &str, description: Option<&str>, owner: Option<&str>) -> Library {
        let mut library = Library::default();
        library.select("spotify:playlist:a");
        library.apply(&Update::Playlists(vec![listed(name, description, owner)]));
        library
    }

    /// The same page, heading a list of our own.
    fn library_ours(tracks: Vec<TrackEntry>, name: &str) -> Library {
        let mut library = library_with(tracks);
        let mut entry = listed(name, None, None);
        entry.owner = Some(Owner::You);
        library.apply(&Update::Playlists(vec![entry]));
        library
    }

    fn listed(name: &str, description: Option<&str>, owner: Option<&str>) -> PlaylistEntry {
        PlaylistEntry {
            composed: Vec::new(),
            uri: "spotify:playlist:a".to_owned(),
            name: name.to_owned(),
            named: true,
            cover: None,
            description: description.map(str::to_owned),
            owner: owner.and_then(OwnerName::from_wire).map(Owner::Named),
            tracks: None,
            access: None,
        }
    }

    /// `count` tracks, all `duration_ms` long, so the band's total is exact.
    fn timed_tracks(count: usize, duration_ms: u32) -> Vec<TrackEntry> {
        (0..count)
            .map(|index| TrackEntry {
                duration_ms: Some(duration_ms),
                ..track(&format!("t{index}"), &format!("t{index}"))
            })
            .collect()
    }

    fn playlist(uri: &str, name: &str) -> PlaylistEntry {
        PlaylistEntry {
            composed: Vec::new(),
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: None,
            access: None,
        }
    }

    fn show(uri: &str, name: &str) -> takofy_model::FollowedShow {
        takofy_model::FollowedShow {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            publisher: None,
            cover: None,
            episodes: None,
        }
    }

    fn cover_url(tail: &str) -> CoverUrl {
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
    }

    fn playlist_with_cover(uri: &str, name: &str, cover: CoverUrl) -> PlaylistEntry {
        PlaylistEntry {
            composed: Vec::new(),
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            cover: Some(PlaylistCover::derived(cover)),
            description: None,
            owner: None,
            tracks: None,
            access: None,
        }
    }

    fn track_with_cover(index: usize) -> TrackEntry {
        TrackEntry {
            uri: format!("t{index}"),
            title: format!("t{index}"),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            artist_names: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: true,
            cover: Some(cover_url(&format!("c{index}"))),
        }
    }

    fn image() -> CoverImage {
        CoverImage::new(1, 1, vec![0; 4]).expect("a 1x1 image")
    }

    /// A playlist page whose band heads `covers`, one per track, ready.
    fn mosaic_library(covers: &[&str]) -> Library {
        let tracks = covers
            .iter()
            .enumerate()
            .map(|(index, tail)| TrackEntry {
                cover: Some(cover_url(tail)),
                ..track(&format!("t{index}"), &format!("t{index}"))
            })
            .collect();
        library_detailed(tracks, "Evening", Some(DESCRIPTION), None)
    }

    /// The same page, with a picture of its own to fall back to.
    fn mosaic_library_with_picture(covers: &[&str]) -> Library {
        let mut library = mosaic_library(covers);
        library.apply(&Update::Playlists(vec![PlaylistEntry {
            composed: Vec::new(),
            cover: Some(PlaylistCover::derived(cover_url("m0"))),
            ..listed("Evening", Some(DESCRIPTION), None)
        }]));
        library
    }

    /// A long list whose first four tracks carry distinct covers, so the band has
    /// a mosaic to draw beside a full byline and description.
    fn mosaic_full() -> Library {
        let mut tracks = timed_tracks(62, 223_548);
        for (index, track) in tracks.iter_mut().enumerate().take(mosaic::QUADRANTS) {
            track.cover = Some(cover_url(&format!("m{index}")));
        }
        library_detailed(tracks, "Evening", Some(DESCRIPTION), Some("alice"))
    }

    /// Holds every URL in `art`, as the fetches would have by the time the band
    /// draws.
    ///
    /// One URL at a time: a batch larger than the store's in-flight cap would
    /// leave its tail wanted and never sent.
    fn hold(ctx: &Context, art: &mut Art, urls: &[CoverUrl], now: Instant) {
        for url in urls {
            art.begin_frame();
            let _ = art.look(Some(url), now);
            while !art.requests(now).is_empty() {}
            art.apply(
                &Update::Cover {
                    url: url.clone(),
                    image: image(),
                },
                now,
            );
            art.upload(ctx);
        }
    }

    /// Drives `url` to a failure it will not retry, as three failed answers do.
    ///
    /// The second and third attempts are only sent once their retry delay is
    /// due, so the clock moves with the store's own schedule.
    fn fail_permanently(art: &mut Art, url: &CoverUrl, now: Instant) -> Instant {
        let mut at = now;
        for delay in [
            Duration::ZERO,
            Duration::from_secs(5),
            Duration::from_secs(30),
        ] {
            at += delay;
            art.begin_frame();
            let _ = art.look(Some(url), at);
            while !art.requests(at).is_empty() {}
            art.apply(&Update::CoverFailed { url: url.clone() }, at);
        }
        at
    }

    /// One frame of the tracklist with a store the test owns.
    fn band_frame_with(
        ctx: &Context,
        library: &mut Library,
        art: &mut Art,
        now: Instant,
    ) -> egui::FullOutput {
        let tokens = Tokens::DARK;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        run(ctx, input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                library,
                &mut transport,
                &mut queue,
                art,
                now,
                NOW_MS,
                &tokens,
            );
        })
    }

    /// The covers the band paints, read from the art box's own hairline.
    fn band_art(output: &egui::FullOutput) -> Rect {
        let tokens = Tokens::DARK;
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape)
                    if (shape.rect.width() - band::ART).abs() < 0.01
                        && (shape.rect.height() - band::ART).abs() < 0.01
                        && (shape.stroke.width - stroke::HAIRLINE).abs() < 0.01
                        && shape.stroke.color == tokens.palette.border_strong =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
            .expect("the band draws its art box")
    }

    /// The textured rects inside `area`, in paint order.
    fn textured(output: &egui::FullOutput, area: Rect) -> Vec<Rect> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape)
                    if shape.brush.is_some() && area.contains(shape.rect.center()) =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
            .collect()
    }

    /// The sunken holes inside `area`, in paint order.
    fn holes(output: &egui::FullOutput, area: Rect) -> Vec<Rect> {
        let tokens = Tokens::DARK;
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape)
                    if shape.brush.is_none()
                        && shape.fill == tokens.palette.surface_sunken
                        && area.contains(shape.rect.center()) =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
            .collect()
    }

    /// A held cover's drawing, so a quadrant can be named without a texture id.
    fn drawn_cover(art: &mut Art, tail: &str, now: Instant) -> Cover {
        art.look(Some(&cover_url(tail)), now)
    }

    fn fetch_urls(commands: Vec<Command>) -> Vec<CoverUrl> {
        commands
            .into_iter()
            .filter_map(|command| match command {
                Command::Fetch { url } => Some(url),
                _ => None,
            })
            .collect()
    }

    /// A library whose playlists are ready and whose tracks are untouched: the
    /// idle state the grid fills.
    fn library_idle(playlists: Vec<PlaylistEntry>) -> Library {
        let mut library = Library::default();
        library.apply(&Update::Playlists(playlists));
        library
    }

    /// The rect the idle grid draws the `index`-th card into, solved the same
    /// way the view solves it: the leading space, then the grid at the current
    /// width. Nothing here is a coordinate copied out of the view.
    fn card_rect_for(ctx: &Context, index: usize) -> Rect {
        let tokens = Tokens::DARK;
        let mut found = None;
        run(ctx, input(Vec::new()), |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                crate::view::inner(ui, |ui| {
                    page_head::show(ui, "Your Library", &library_note(2), &tokens);
                    let _ = filter_row::show_with(
                        ui,
                        &tokens,
                        &library_filter::ROW,
                        RootKind::All,
                        RootKind::control,
                        root_filter_label,
                        filter_row::RowSlot::Trailing(0.0),
                    );
                    let solved = media_grid(ui);
                    found = Some(solved.card_rect(index));
                });
            });
        });
        found.expect("the idle grid lays out")
    }

    /// The grid sits one gutter inside the pane, like every other page's content.
    #[test]
    fn the_grid_sits_one_gutter_inside_the_pane() {
        let ctx = context();
        let card = card_rect_for(&ctx, 0);
        assert!(
            (card.left() - space::GUTTER).abs() < 0.01,
            "the first card starts at {}, the pane's gutter is {}",
            card.left(),
            space::GUTTER
        );
    }

    /// A context with the app's fonts and style installed.
    fn context() -> Context {
        let tokens = Tokens::DARK;
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));
        ctx
    }

    /// The rect `tracks_view` draws the `index`-th row into, solved the same way
    /// the view solves it: the header, then the whole list reserved in one block,
    /// then `layout::table::row_rect` for the row.
    ///
    /// `Table::Full` is the width that gives every column room, so the row is
    /// solved with all seven tracks present. Nothing here is a coordinate copied
    /// out of the view: the header and the row pitch are the view's own solver.
    fn row_rect_for(ctx: &Context, count: usize, index: usize) -> Rect {
        let tokens = Tokens::DARK;
        let table = Table::Full;
        let mut found = None;
        run(ctx, input(Vec::new()), |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                crate::view::inner(ui, |ui| {
                    let width = ui.available_width();
                    actionbar::row_rect(ui);
                    let columns = table_layout::columns(width, table.columns());
                    head(ui, columns, &tokens);
                    let list = list_rect(ui, count);
                    found = Some(table_layout::row_rect(list, index, extent::ROW_H));
                });
            });
        });
        found.expect("the header and the list lay out")
    }

    /// Makes the library's liked set these tracks, as a landed read would.
    fn liked(library: &mut Library, uris: &[&str]) {
        assert!(library.likes.request(true));
        library.likes.apply(&crate::likes::Event::Set(
            uris.iter().map(|uri| (*uri).to_owned()).collect(),
        ));
    }

    /// The centre of the like column of the `index`-th row, solved the way the
    /// view solves its columns.
    fn heart_point_for(ctx: &Context, count: usize, index: usize) -> Pos2 {
        let tokens = Tokens::DARK;
        let table = Table::Full;
        let mut found = None;
        run(ctx, input(Vec::new()), |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                crate::view::inner(ui, |ui| {
                    let width = ui.available_width();
                    actionbar::row_rect(ui);
                    let columns = table_layout::columns(width, table.columns());
                    head(ui, columns, &tokens);
                    let list = list_rect(ui, count);
                    let row = table_layout::row_rect(list, index, extent::ROW_H);
                    found = Some(columns.at(row.min).like.center());
                });
            });
        });
        found.expect("the heart's column lays out")
    }

    /// The rect `tracks_view` draws the `index`-th row's `cell` into, solved the
    /// way the painter solves it: the row's own rect, then the columns at its
    /// origin.
    ///
    /// The cell is the whole column, padding included, but a credit's link is the
    /// words painted at its left edge: a probe past those words is the row's.
    fn cell_rect_for(
        ctx: &Context,
        count: usize,
        index: usize,
        cell: fn(table_layout::Columns) -> Rect,
    ) -> Rect {
        let row = row_rect_for(ctx, count, index);
        cell(table_layout::columns(row.width(), Table::Full.columns()).at(row.min))
    }

    /// The rect the painted words of `label` occupy in one frame of the
    /// tracklist, read back from the galleys the view painted.
    fn words_of(ctx: &Context, library: &mut Library, label: &str) -> Rect {
        let output = tracklist_frame(ctx, Table::Full, library);
        text_rect(&output, label).unwrap_or_else(|| panic!("{label:?} was not painted"))
    }

    /// The rect `tracks_view` draws the `index`-th row into while the band is
    /// drawn, solved the same way the view solves it: the band and its action row
    /// first, then the header, then the whole list reserved in one block, then
    /// `layout::table::row_rect` for the row.
    ///
    /// The band pushes the rows down. Every other geometry test builds no named
    /// playlist, so the band never appears in them, and this is the only solve
    /// that sees its offset.
    fn row_rect_below_hero(
        ctx: &Context,
        screen: Vec2,
        count: usize,
        index: usize,
        title: &str,
    ) -> Rect {
        let tokens = Tokens::DARK;
        let table = Table::Full;
        let mut found = None;
        run(ctx, sized_input(screen, Vec::new()), |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let band = hero::Band {
                    eyebrow: "Playlist",
                    title,
                    meta: Vec::new(),
                    credit: None,
                    description: "",
                    cover: Cover::Missing,
                };
                let _ = hero::show_with(ui, &band, &tokens, hero::Title::Plain);
                actionbar::row_rect(ui);
                crate::view::inner(ui, |ui| {
                    let width = ui.available_width();
                    let columns = table_layout::columns(width, table.columns());
                    head(ui, columns, &tokens);
                    let list = list_rect(ui, count);
                    found = Some(table_layout::row_rect(list, index, extent::ROW_H));
                });
            });
        });
        found.expect("the hero, header and list lay out")
    }

    /// The action row's tiles, solved by the row itself. The probe window is the
    /// page's own width, so the form is the one the page would have there.
    fn tiles_of(row: Rect) -> crate::layout::actionbar::Placed {
        crate::layout::actionbar::solve(
            row,
            crate::layout::actionbar::Kind::Playlist(crate::layout::actionbar::Form::at(
                PROBE.x,
                Table::Full,
            )),
            crate::layout::actionbar::Parts {
                shuffle: true,
                field: true,
                follow: None,
            },
        )
    }

    /// The rect `tracks_view` draws the action row's play control into, solved
    /// the way the view solves it: the band, then the row's slot in the flow, then
    /// the row's own solver.
    fn play_tile_rect_for(ctx: &Context, title: &str) -> Rect {
        let tokens = Tokens::DARK;
        let mut found = None;
        run(ctx, input(Vec::new()), |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let band = hero::Band {
                    eyebrow: "Playlist",
                    title,
                    meta: Vec::new(),
                    credit: None,
                    description: "",
                    cover: Cover::Missing,
                };
                let _ = hero::show_with(ui, &band, &tokens, hero::Title::Plain);
                let row = actionbar::row_rect(ui);
                found = Some(tiles_of(row).play);
            });
        });
        found.expect("the band and its action row lay out")
    }

    /// Clicks `centre` `times` times, one event per pass, and returns the command
    /// the view reported while they happened. `screen` is the probe window, so a
    /// test that needs the band and two whole rows can ask for a taller one.
    fn clicks(
        screen: Vec2,
        ctx: &Context,
        centre: Pos2,
        times: usize,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
    ) -> Option<Command> {
        let tokens = Tokens::DARK;
        let mut command = None;
        // Warm up so hit-testing has a previous pass to read.
        run(ctx, sized_input(screen, Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                library,
                transport,
                queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });
        for _ in 0..times {
            for event in crate::view::test_support::click_events(centre) {
                let command = &mut command;
                let library = &mut *library;
                let transport = &mut *transport;
                let queue = &mut *queue;
                run(ctx, sized_input(screen, vec![event]), |ui| {
                    if let Some(next) = tracks_view(
                        ui,
                        Table::Full,
                        library,
                        transport,
                        queue,
                        &mut Art::default(),
                        Instant::now(),
                        NOW_MS,
                        &tokens,
                    ) {
                        *command = Some(next);
                    }
                });
            }
        }
        command
    }

    /// One click on `centre`, and the command the view returned while it happened.
    fn click(
        ctx: &Context,
        centre: Pos2,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
    ) -> Option<Command> {
        clicks(PROBE, ctx, centre, 1, library, transport, queue)
    }

    /// The same click, in a window of `screen`: a point read from a frame drawn
    /// in a wider window only lands on the same widget where it was drawn.
    fn click_in(
        screen: Vec2,
        ctx: &Context,
        centre: Pos2,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
    ) -> Option<Command> {
        clicks(screen, ctx, centre, 1, library, transport, queue)
    }

    /// The x the header paints the upper-cased `label` at, read back from the
    /// frame's text shapes. The header paints through the painter, so this is
    /// where a heading actually landed rather than a re-solve of its columns.
    fn heading_left(
        ctx: &Context,
        label: &str,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
    ) -> f32 {
        let tokens = Tokens::DARK;
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                library,
                transport,
                queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });
        output.textures_delta.clear();
        let label = label.to_uppercase();
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => Some(text.pos.x),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the header painted {label:?}"))
    }

    /// One double click on `centre`, and the command the view returned while it
    /// happened.
    fn double_click(
        ctx: &Context,
        centre: Pos2,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
    ) -> Option<Command> {
        clicks(PROBE, ctx, centre, 2, library, transport, queue)
    }

    /// Whether `playlist`'s list holds `index` as part of its selection.
    fn picked(ctx: &Context, playlist: &str, count: usize, index: usize) -> bool {
        crate::view::carry::List::open(ctx, egui::Id::unique(("playlist-tracks", playlist)), count)
            .look(index)
            .selected
    }

    /// One pass of the tracklist with one event, and the command it reported.
    fn pass(
        ctx: &Context,
        event: Event,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
    ) -> Option<Command> {
        let tokens = Tokens::DARK;
        let mut command = None;
        run(ctx, input(vec![event]), |ui| {
            command = tracks_view(
                ui,
                Table::Full,
                library,
                transport,
                queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });
        command
    }

    /// A press on `from` and a nine-point drag: the passes that lift a row.
    fn lift_row(
        ctx: &Context,
        from: Pos2,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
    ) {
        let mut events = vec![
            Event::PointerMoved(from),
            crate::view::test_support::button(from, true),
        ];
        for step in 1..=3 {
            events.push(Event::PointerMoved(
                from + Vec2::new(step as f32 * 3.0, 0.0),
            ));
        }
        for event in events {
            let _ = pass(ctx, event, library, transport, queue);
        }
    }

    #[test]
    fn a_single_click_selects_and_a_double_click_plays_it() {
        let ctx = context();
        let mut library = library_with(vec![track("a", "t0"), track("b", "t1")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let centre = row_rect_for(&ctx, 2, 1).center();
        let command = click(&ctx, centre, &mut library, &mut transport, &mut queue);

        assert_eq!(command, None, "a single click plays nothing");
        assert!(transport.now_playing.is_none(), "and starts nothing");
        assert!(
            picked(&ctx, "spotify:playlist:a", 2, 1),
            "the row under the click is picked"
        );
        assert!(
            !picked(&ctx, "spotify:playlist:a", 2, 0),
            "and only that row"
        );

        // A fresh page for the double click: a third click inside the window is
        // a triple click, and that plays nothing.
        let ctx = context();
        let mut library = library_with(vec![track("a", "t0"), track("b", "t1")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let command = double_click(&ctx, centre, &mut library, &mut transport, &mut queue);

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "b".to_owned()
            }),
            "a double click starts the track"
        );
    }

    #[test]
    fn a_selection_stays_on_the_playlist_it_was_made_on() {
        let ctx = context();
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut first = library_with(vec![track("a", "t0"), track("b", "t1")]);
        let centre = row_rect_for(&ctx, 2, 1).center();
        let _ = click(&ctx, centre, &mut first, &mut transport, &mut queue);

        let mut second = Library::default();
        let _ = second.select("spotify:playlist:b");
        second.apply(&Update::Tracks {
            playlist: "spotify:playlist:b".to_owned(),
            tracks: vec![track("a", "t0"), track("b", "t1")],
            cover: None,
            access: None,
        });
        let tokens = Tokens::DARK;
        run(&ctx, input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut second,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });

        assert!(
            picked(&ctx, "spotify:playlist:a", 2, 1),
            "the first playlist keeps its pick"
        );
        assert!(
            !picked(&ctx, "spotify:playlist:b", 2, 1),
            "the second playlist does not show it against the same index"
        );
    }

    #[test]
    fn lifting_a_row_puts_that_track_in_the_payload() {
        let ctx = context();
        let mut library = library_with(vec![track("a", "t0"), track("b", "t1")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let centre = row_rect_for(&ctx, 2, 1).center();
        lift_row(&ctx, centre, &mut library, &mut transport, &mut queue);

        let carried = crate::view::carry::carried(&ctx).expect("a carried track");
        assert_eq!(
            carried.source,
            crate::view::carry::Source::List(egui::Id::unique((
                "playlist-tracks",
                "spotify:playlist:a"
            ))),
            "the playlist names itself as the source"
        );
        assert_eq!(carried.rows, vec![1], "one row travels");
        assert_eq!(carried.entries.len(), 1);
        assert_eq!(carried.entries[0].uri, "b", "the row under the pointer");
        assert!(
            matches!(carried.ghost, crate::view::carry::Ghost::Table(_)),
            "a track row's own snapshot is the ghost"
        );
    }

    #[test]
    fn a_carried_list_scrolls_at_its_edge() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let tracks: Vec<TrackEntry> = (0..40)
            .map(|index| track(&format!("t{index}"), &format!("t{index}")))
            .collect();
        let mut library = library_with(tracks);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let mut drawn = |raw: RawInput, library: &mut Library| -> Vec<String> {
            let output = run(&ctx, raw, |ui| {
                let _ = tracks_view(
                    ui,
                    Table::Full,
                    library,
                    &mut transport,
                    &mut queue,
                    &mut Art::default(),
                    Instant::now(),
                    NOW_MS,
                    &tokens,
                );
            });
            drawn_texts(&output)
                .into_iter()
                .map(str::to_owned)
                .collect()
        };

        let before = drawn(sized_input(PROBE, Vec::new()), &mut library);
        egui::DragAndDrop::set_payload(&ctx, ());

        // The pointer holds the list's bottom edge for a quarter of a second.
        let edge = Pos2::new(PROBE.x / 2.0, PROBE.y - 1.0);
        let mut time = 1.0;
        for _ in 0..24 {
            time += 0.016;
            let mut raw = sized_input(PROBE, vec![Event::PointerMoved(edge)]);
            raw.time = Some(time);
            let _ = drawn(raw, &mut library);
        }
        let after = drawn(sized_input(PROBE, Vec::new()), &mut library);

        assert!(
            after
                .iter()
                .any(|line| line.starts_with('t') && !before.contains(line)),
            "the carried list scrolls at its edge: {after:?}"
        );
    }

    #[test]
    fn an_editable_page_offers_its_rows() {
        let ctx = context();
        let mut library = library_with(vec![track("a", "t0"), track("b", "t1")]);
        let output = tracklist_frame_in(&ctx, TALL, &mut library);

        let offer = crate::view::carry::take_offer(&ctx).expect("an editable page offers its rows");
        assert_eq!(
            offer.list,
            egui::Id::unique(("playlist-tracks", "spotify:playlist:a")),
            "the offer names the list its rows lift under"
        );
        assert_eq!(offer.playlist, "spotify:playlist:a");
        assert_eq!(offer.zone.first, 0, "the page numbers its rows from zero");
        assert_eq!(offer.zone.count, 2);
        assert_eq!(offer.zone.row_h, extent::ROW_H);
        for row in ["t0", "t1"] {
            assert!(
                offer.zone.hit.contains(galley_at(&output, row)),
                "{row} is inside the drop target"
            );
        }
    }

    #[test]
    fn a_read_only_page_offers_nothing() {
        let ctx = context();
        let mut library = read_only_library_with(vec![track("a", "t0"), track("b", "t1")]);
        let _ = tracklist_frame(&ctx, Table::Full, &mut library);

        assert_eq!(
            crate::view::carry::take_offer(&ctx),
            None,
            "a page the user may not write takes no reorder"
        );
    }

    #[test]
    fn a_read_only_page_still_lifts_and_carries() {
        let ctx = context();
        let mut library = read_only_library_with(vec![track("a", "t0"), track("b", "t1")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let centre = row_rect_for(&ctx, 2, 1).center();
        lift_row(&ctx, centre, &mut library, &mut transport, &mut queue);

        let carried = crate::view::carry::carried(&ctx).expect("a read-only page still lifts");
        assert_eq!(carried.rows, vec![1]);
        assert_eq!(
            carried.entries[0].uri, "b",
            "the lifted track travels to the queue"
        );
    }

    #[test]
    fn a_heart_click_posts_one_ask_with_the_rows_uri() {
        let ctx = context();
        let tracks = vec![
            track("spotify:track:a", "t0"),
            track("spotify:track:b", "t1"),
        ];
        let mut library = library_with(tracks);
        liked(&mut library, &[]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let point = heart_point_for(&ctx, 2, 1);

        let command = click(&ctx, point, &mut library, &mut transport, &mut queue);

        assert_eq!(command, None, "a heart is not a command of its own");
        let ask = crate::view::heart::take(&ctx).expect("an ask was posted");
        assert_eq!(ask.tracks, vec!["spotify:track:b".to_owned()]);
        assert!(ask.liked, "an unsaved row asks to be saved");
    }

    #[test]
    fn a_heart_click_inside_a_pick_acts_on_that_row_only_and_keeps_the_pick() {
        let ctx = context();
        let tracks = vec![
            track("spotify:track:a", "t0"),
            track("spotify:track:b", "t1"),
        ];
        let mut library = library_with(tracks);
        liked(&mut library, &[]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let list = egui::Id::unique(("playlist-tracks", "spotify:playlist:a"));
        crate::view::carry::select_rows(&ctx, list, 0..2);
        let point = heart_point_for(&ctx, 2, 1);

        let _ = click(&ctx, point, &mut library, &mut transport, &mut queue);

        let ask = crate::view::heart::take(&ctx).expect("an ask was posted");
        assert_eq!(
            ask.tracks,
            vec!["spotify:track:b".to_owned()],
            "the pick is the menu's, not the heart's"
        );
        let kept = crate::view::carry::List::open(&ctx, list, 2);
        assert!(
            kept.look(0).selected && kept.look(1).selected,
            "the heart changed the pick"
        );
    }

    #[test]
    fn a_track_listed_twice_shows_one_state_on_both_rows() {
        let ctx = context();
        let tracks = vec![
            track("spotify:track:a", "t0"),
            track("spotify:track:a", "t0 again"),
        ];
        let mut library = library_with(tracks);
        liked(&mut library, &["spotify:track:a"]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        for index in 0..2 {
            let point = heart_point_for(&ctx, 2, index);
            let _ = click(&ctx, point, &mut library, &mut transport, &mut queue);
            let ask = crate::view::heart::take(&ctx).expect("an ask was posted");
            assert_eq!(
                ask.tracks,
                vec!["spotify:track:a".to_owned()],
                "row {index}"
            );
            assert!(!ask.liked, "row {index} did not know the track was liked");
        }
    }

    #[test]
    fn a_failed_like_draws_its_sentence_in_the_pages_slot() {
        let ctx = context();
        let tracks = vec![track("spotify:track:a", "t0")];
        let mut library = library_with(tracks);
        liked(&mut library, &[]);
        let change = library
            .likes
            .set(&["spotify:track:a".to_owned()], true)
            .expect("a change");
        library.likes.apply(&crate::likes::Event::Failed {
            tracks: change.tracks,
            failure: Failure::Save,
        });
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let tokens = Tokens::DARK;

        let output = run(&ctx, input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });

        assert!(
            drawn_texts(&output).contains(&"Could not save that change. Try again."),
            "the failed like drew no sentence: {:?}",
            drawn_texts(&output)
        );
    }

    #[test]
    fn a_right_click_posts_the_menu_its_row_asked_for() {
        let ctx = context();
        let tracks = vec![track("a", "t0"), linkable("b")];
        let mut library = library_with(tracks.clone());
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let centre = row_rect_for(&ctx, 2, 1).center();

        for event in crate::view::test_support::secondary_click_events(centre) {
            let _ = pass(&ctx, event, &mut library, &mut transport, &mut queue);
        }
        let request = crate::view::context_menu::take_request(&ctx)
            .expect("a right-click on a row posted a request");
        assert_eq!(request.targets.len(), 1);
        assert_eq!(request.targets[0].uri, "b", "the row under the pointer");
        assert_eq!(
            request.page,
            Some(crate::route::Page::Playlist(
                "spotify:playlist:a".to_owned()
            )),
            "the page it was opened on, so a page change can close it"
        );
        assert!(
            request
                .items
                .iter()
                .any(|item| item.verb == crate::menu::Verb::GoToAlbum),
            "the row's own album is one of its verbs"
        );
    }

    #[test]
    fn a_page_with_a_write_in_flight_posts_no_menu() {
        let ctx = context();
        let tracks = vec![track("a", "t0"), track("b", "t1")];
        let mut library = library_with(tracks.clone());
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        assert!(
            library
                .move_tracks("spotify:playlist:a", &[0], 2, &[tracks[0].clone()])
                .is_some(),
            "the page takes the move first"
        );
        let centre = row_rect_for(&ctx, 2, 1).center();

        for event in crate::view::test_support::secondary_click_events(centre) {
            let _ = pass(&ctx, event, &mut library, &mut transport, &mut queue);
        }
        assert_eq!(
            crate::view::context_menu::take_request(&ctx),
            None,
            "a write in flight holds the rows still, so no menu opens over them"
        );
    }

    #[test]
    fn a_page_without_access_offers_nothing() {
        let ctx = context();
        let mut library = library_selected(
            "spotify:playlist:a",
            vec![track("a", "t0"), track("b", "t1")],
        );
        let _ = tracklist_frame(&ctx, Table::Full, &mut library);

        assert_eq!(
            crate::view::carry::take_offer(&ctx),
            None,
            "a read that named no state offers no reorder"
        );
    }

    #[test]
    fn a_reverted_move_says_so_above_the_rows() {
        let ctx = context();
        let tracks = vec![track("a", "t0"), track("b", "t1"), track("c", "t2")];
        let mut library = library_with(tracks.clone());
        assert!(
            library
                .move_tracks("spotify:playlist:a", &[0], 2, &[tracks[0].clone()])
                .is_some(),
            "the page takes the move first"
        );
        library.apply(&Update::MoveFailed {
            playlist: "spotify:playlist:a".to_owned(),
            failure: Failure::Save,
        });

        let output = tracklist_frame(&ctx, Table::Full, &mut library);
        let drawn = drawn_galleys(&output);
        let notice = crate::signin::failure_message(Failure::Save);
        assert!(
            galley_top(&drawn, notice) < galley_top(&drawn, "t0"),
            "the failure heads the rows it puts back"
        );
        assert_eq!(painted_rows(&output), vec![0, 1, 2], "the order is back");
    }

    #[test]
    fn the_failed_page_draws_its_failure_and_a_retry_that_reloads() {
        let ctx = context();
        let mut library = library_with(vec![track("a", "t0")]);
        library.apply(&Update::TracksFailed {
            playlist: "spotify:playlist:a".to_owned(),
            failure: Failure::Load,
        });
        let output = tracklist_frame(&ctx, Table::Full, &mut library);
        let painted = drawn_texts(&output);
        assert!(
            painted.contains(&crate::signin::failure_message(Failure::Load)),
            "the failure is drawn: {painted:?}"
        );
        assert!(painted.contains(&"Retry"), "with the retry beside it");

        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let retry = galley_at(&output, "Retry");
        assert_eq!(
            click(
                &ctx,
                retry + Vec2::new(4.0, 4.0),
                &mut library,
                &mut transport,
                &mut queue
            ),
            Some(Command::LoadTracks {
                playlist: "spotify:playlist:a".to_owned()
            }),
            "the retry asks for the page again"
        );
    }

    #[test]
    fn a_double_click_on_a_row_starts_that_track_through_the_queue() {
        let ctx = context();
        let mut library = library_with(vec![track("a", "t0"), track("b", "t1")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        // The click is where the view draws row 0, not a probe's geometry.
        let centre = row_rect_for(&ctx, 2, 0).center();
        let command = double_click(&ctx, centre, &mut library, &mut transport, &mut queue);

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "a".to_owned()
            })
        );
        assert!(
            queue.can_next(),
            "the double-clicked row must start the queue, not only the transport"
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("a")
        );
    }

    #[test]
    fn a_single_track_list_starts_its_only_track() {
        let ctx = context();
        let mut library = library_with(vec![track("a", "t0")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let centre = row_rect_for(&ctx, 1, 0).center();
        let command = double_click(&ctx, centre, &mut library, &mut transport, &mut queue);

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "a".to_owned()
            })
        );
        assert!(
            !queue.can_next(),
            "a one-track queue has nothing after its only entry"
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("a")
        );
    }

    #[test]
    fn a_row_with_no_album_added_or_duration_still_lays_out() {
        let tokens = Tokens::DARK;
        let ctx = context();
        // An album or an artist context carries neither an album name nor a
        // playlist's added date, and a track may have no length. Solving its
        // columns and double-clicking must still lay the row out and activate it.
        let columns = table_layout::columns(900.0, Table::Full.columns());
        let rect = table_layout::row_rect(
            Rect::from_min_size(Pos2::ZERO, vec2(900.0, extent::ROW_H)),
            0,
            extent::ROW_H,
        );
        let row = Row {
            uri: "a",
            number: 1,
            title: "t0",
            artist: "",
            artist_uri: None,
            album: None,
            album_uri: None,
            added: None,
            duration: None,
            state: State::Resting,
            cover: Some(Cover::Missing),
            look: Look::default(),
            menu: false,
            liked: None,
        };

        run(&ctx, input(Vec::new()), |ui| {
            let _ = track_row::show(ui, rect, columns, &row, &tokens);
        });
        let mut clicked = false;
        for _ in 0..2 {
            for event in [
                Event::PointerMoved(rect.center()),
                crate::view::test_support::button(rect.center(), true),
                crate::view::test_support::button(rect.center(), false),
            ] {
                run(&ctx, input(vec![event]), |ui| {
                    clicked |=
                        track_row::show(ui, rect, columns, &row, &tokens) == Some(Click::Play);
                });
            }
        }
        assert!(
            clicked,
            "a row with no album, added or duration must still lay out and activate"
        );
    }

    #[test]
    fn a_row_given_an_album_and_a_duration_draws_them() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let columns = table_layout::columns(900.0, Table::Full.columns());
        let rect = table_layout::row_rect(
            Rect::from_min_size(Pos2::ZERO, vec2(900.0, extent::ROW_H)),
            0,
            extent::ROW_H,
        );
        let row = Row {
            uri: "a",
            number: 1,
            title: "t0",
            artist: "Band",
            artist_uri: None,
            album: Some("Record"),
            album_uri: None,
            added: None,
            duration: Some("3:42"),
            state: State::Resting,
            cover: Some(Cover::Missing),
            look: Look::default(),
            menu: false,
            liked: None,
        };

        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = track_row::show(ui, rect, columns, &row, &tokens);
        });
        output.textures_delta.clear();
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&"Record"),
            "the album column must draw its label: {texts:?}"
        );
        assert!(
            texts.contains(&"3:42"),
            "the duration column must draw its label: {texts:?}"
        );
    }

    #[test]
    fn the_view_hands_a_tracks_album_and_length_to_its_row() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let mut library = library_with(vec![TrackEntry {
            uri: "a".to_owned(),
            title: "t0".to_owned(),
            artists: "Band".to_owned(),
            album: Some("Record".to_owned()),
            album_uri: None,
            artist_uris: Vec::new(),
            artist_names: Vec::new(),
            added_at: None,
            duration_ms: Some(222_000),
            named: true,
            cover: None,
        }]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });
        output.textures_delta.clear();
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&"Record"),
            "the row must be handed the track's album: {texts:?}"
        );
        assert!(
            texts.contains(&"3:42"),
            "the row must be handed the track's length as m:ss: {texts:?}"
        );
    }

    #[test]
    fn a_playlist_row_draws_its_added_label() {
        let ctx = context();
        let mut library = library_with(vec![dated("a", "t0", Some(date(NOW_MS - 4 * DAY)))]);

        let texts = painted_lines(&ctx, Table::Full, &mut library);
        assert!(
            texts.contains(&"ADDED".to_owned()),
            "the header must keep the added column: {texts:?}"
        );
        assert!(
            texts.contains(&"4 days ago".to_owned()),
            "the row must show when the track was added: {texts:?}"
        );
    }

    #[test]
    fn a_playlist_without_dates_keeps_its_rows_and_header() {
        // The column is a playlist's, not a date's: a playlist whose dates are
        // all absent still heads it and still draws every row.
        let ctx = context();
        let mut library = library_with(vec![dated("a", "t0", None), dated("b", "t1", None)]);

        let texts = painted_lines(&ctx, Table::Full, &mut library);
        assert!(
            texts.contains(&"ADDED".to_owned()),
            "the header must stay with no dates loaded: {texts:?}"
        );
        assert!(texts.contains(&"t0".to_owned()), "row 0 must draw");
        assert!(texts.contains(&"t1".to_owned()), "row 1 must draw");
        assert!(
            !texts
                .iter()
                .any(|text| text == "Today" || text.ends_with(" ago")),
            "an absent date must paint no cell: {texts:?}"
        );
    }

    #[test]
    fn a_future_date_draws_no_cell() {
        let ctx = context();
        let mut library = library_with(vec![dated("a", "t0", Some(date(NOW_MS + DAY)))]);

        let texts = painted_lines(&ctx, Table::Full, &mut library);
        assert!(texts.contains(&"t0".to_owned()), "the row must still draw");
        assert!(
            !texts
                .iter()
                .any(|text| text == "Today" || text.ends_with(" ago")),
            "a date after now is not an age: {texts:?}"
        );
    }

    #[test]
    fn a_context_that_is_not_a_playlist_draws_no_added_column() {
        let ctx = context();
        // Long enough to elide at the probe width in both contexts, so the
        // visible run is a measure of the column the title was given.
        let title = "The Extremely Long-Named Orchestra of the Northern Coast and Its Friends \
Plays the Music of the Twentieth Century for an Audience of Patient Listeners";
        let rows = || vec![dated("a", title, Some(date(NOW_MS - 2 * DAY)))];

        let mut playlist = library_selected("spotify:playlist:a", rows());
        let playlist_texts = painted_runs(&ctx, Table::Full, &mut playlist);

        let mut album = library_selected("spotify:album:a", rows());
        let album_texts = painted_runs(&ctx, Table::Full, &mut album);

        assert!(
            playlist_texts.contains(&"ADDED".to_owned()),
            "a playlist heads the date column: {playlist_texts:?}"
        );
        assert!(
            !album_texts.contains(&"ADDED".to_owned()),
            "an album must not head a date column: {album_texts:?}"
        );
        assert!(
            !album_texts
                .iter()
                .any(|text| text == "Today" || text.ends_with(" ago")),
            "an album's rows must show no date even when they carry one: {album_texts:?}"
        );

        // The dropped column's width goes to the title: the same title shows
        // more of itself in the album than in the playlist.
        let painted_title = |texts: &[String]| {
            texts
                .iter()
                .find(|text| text.starts_with("The Extremely"))
                .cloned()
                .expect("the row paints its title")
        };
        let playlist_title = painted_title(&playlist_texts);
        let album_title = painted_title(&album_texts);
        assert!(
            playlist_title.ends_with('…'),
            "the playlist's title must be squeezed: {playlist_title:?}"
        );
        assert!(
            album_title.chars().count() > playlist_title.chars().count(),
            "the album's title must take the freed width: {album_title:?} vs {playlist_title:?}"
        );
    }

    #[test]
    fn the_same_track_twice_shows_each_rows_own_date() {
        let ctx = context();
        let mut library = library_with(vec![
            dated("a", "t0", Some(date(NOW_MS - 4 * DAY))),
            dated("a", "t0", Some(date(NOW_MS - 14 * DAY))),
        ]);

        let texts = painted_lines(&ctx, Table::Full, &mut library);
        assert!(
            texts.contains(&"4 days ago".to_owned()),
            "the first row must show its own date: {texts:?}"
        );
        assert!(
            texts.contains(&"2 weeks ago".to_owned()),
            "the second row must show its own date: {texts:?}"
        );
    }

    #[test]
    fn the_second_of_two_identical_entries_starts_the_second() {
        // Two entries holding one uri: keyed by the uri alone, both rows would
        // share one egui id, so one click would drive both and the higher index
        // would win. The command's uri is `a` whichever row is taken, so the
        // queue's position is the only thing that tells the two apart: starting
        // the second leaves nothing after it, while the first would leave entry
        // two, so `can_next` is the sharp assertion.
        //
        // Index 1 also exercises `row_rect`'s pitch: the other tests only ever
        // click index 0.
        let ctx = context();
        let mut library = library_with(vec![track("a", "t0"), track("a", "t0")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let first = row_rect_for(&ctx, 2, 0);
        let second = row_rect_for(&ctx, 2, 1);
        assert!(
            (second.top() - first.bottom()).abs() < 0.01,
            "the second row must start where the first ends: {first:?} then {second:?}"
        );
        let command = double_click(
            &ctx,
            second.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "a".to_owned()
            })
        );
        assert!(
            !queue.can_next(),
            "the second of two identical entries was started, not the first"
        );
    }

    #[test]
    fn a_double_click_on_the_first_of_two_identical_entries_starts_the_first() {
        // The mirror of the test above. When the two rows shared one id, the
        // higher index overwrote the lower, so clicking the first row started
        // the second; this pins that the first is the one that answers, and it
        // fails against the uri-only key where the other test alone would not.
        let ctx = context();
        let mut library = library_with(vec![track("a", "t0"), track("a", "t0")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let first = row_rect_for(&ctx, 2, 0).center();
        let command = double_click(&ctx, first, &mut library, &mut transport, &mut queue);

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "a".to_owned()
            })
        );
        assert!(
            queue.can_next(),
            "the first of two identical entries was started, not the second"
        );
    }

    #[test]
    fn a_row_outside_the_viewport_takes_no_click() {
        let ctx = context();
        // Sixty rows at `extent::ROW_H` each put row forty far below the 400-point
        // probe window, so `ui.clip_rect()` cannot contain it and the view skips
        // laying it out. A shorter list would leave the clicked row on screen and
        // the test would prove nothing.
        let tracks: Vec<TrackEntry> = (0..60)
            .map(|i| track(&format!("t{i}"), &format!("t{i}")))
            .collect();
        let mut library = library_with(tracks);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let offscreen = row_rect_for(&ctx, 60, 40);
        assert!(
            offscreen.top() > 400.0,
            "the culled row must sit below the probe's viewport: {offscreen:?}"
        );
        let command = double_click(
            &ctx,
            offscreen.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );

        assert_eq!(command, None, "a culled row must not activate");
    }

    #[test]
    fn an_unnamed_row_draws_the_count_inside_the_viewport() {
        let ctx = context();
        let count = 60;
        let mut tracks: Vec<TrackEntry> = (0..count)
            .map(|index| track(&format!("t{index}"), &format!("t{index}")))
            .collect();
        tracks[0].named = false;
        let mut library = library_with(tracks);
        let viewport = Rect::from_min_size(Pos2::ZERO, PROBE);
        let last = row_rect_for(&ctx, count, count - 1);
        assert!(
            !viewport.intersects(last),
            "the last row must sit below the probe's viewport: {last:?}"
        );

        let output = tracklist_frame_in(&ctx, PROBE, &mut library);
        let note = "1 of 60 tracks could not be named";
        let painted = galley_of(&output, note);
        let rect = Rect::from_min_size(galley_at(&output, note), painted.size());
        assert!(
            viewport.contains_rect(rect),
            "the count is drawn inside the viewport, not after the rows: {rect:?}"
        );

        // The strip takes its room from the list: no row draws over the count,
        // and the rows stop at its top edge instead of running under it.
        let rows = painted_rows(&output);
        for index in &rows {
            let (title, _) = clipped_title(&output, &format!("t{index}"))
                .unwrap_or_else(|| panic!("row {index} was painted without its title"));
            assert!(
                !title.intersects(rect),
                "row {index} draws over the count: {title:?} against {rect:?}"
            );
        }
        let highest = *rows.last().expect("the list is drawn");
        let inset = title_inset(&ctx, count, &output);
        assert_eq!(
            rows,
            (0..=highest).collect::<Vec<_>>(),
            "the rows stop where the count starts"
        );
        assert!(
            row_rect_for(&ctx, count, highest).top() + inset < rect.top(),
            "the last row's words start above the count"
        );
        assert!(
            row_rect_for(&ctx, count, highest + 1).top() + inset >= rect.top(),
            "the row under the count would draw its words past the strip"
        );
    }

    #[test]
    fn a_note_costs_the_list_one_row_that_a_named_list_keeps() {
        let ctx = context();
        let count = 60;
        let tracks: Vec<TrackEntry> = (0..count)
            .map(|index| track(&format!("t{index}"), &format!("t{index}")))
            .collect();
        let mut numbered_tracks = tracks.clone();
        numbered_tracks[0].named = false;
        let mut named = library_with(tracks);
        let mut numbered = library_with(numbered_tracks);

        let full = tracklist_frame_in(&ctx, PROBE, &mut named);
        let counted = tracklist_frame_in(&ctx, PROBE, &mut numbered);

        assert!(
            !drawn_texts(&full)
                .iter()
                .any(|text| text.contains("could not be named")),
            "a fully named list draws no count"
        );

        // Without a strip the list uses the whole window: it paints every row
        // that starts above the fold, and nothing below it.
        let all_rows = painted_rows(&full);
        let last_fitting = (0..count)
            .take_while(|index| row_rect_for(&ctx, count, *index).top() < PROBE.y)
            .last()
            .expect("the window holds rows");
        assert_eq!(
            all_rows,
            (0..=last_fitting).collect::<Vec<_>>(),
            "the named list paints every row the window holds"
        );

        let note = "1 of 60 tracks could not be named";
        let strip =
            Rect::from_min_size(galley_at(&counted, note), galley_of(&counted, note).size());
        let fewer = painted_rows(&counted);
        assert!(
            fewer.len() < all_rows.len(),
            "the count costs the list a row: {} rows with it, {} without",
            fewer.len(),
            all_rows.len()
        );
        let highest = *fewer.last().expect("the list is drawn");
        assert_eq!(
            highest + 1,
            *all_rows.last().expect("the list is drawn"),
            "the count costs exactly the one row it covers"
        );
        let inset = title_inset(&ctx, count, &full);
        assert!(
            row_rect_for(&ctx, count, highest).top() + inset < strip.top(),
            "the last row's words start above the count"
        );
        assert!(
            row_rect_for(&ctx, count, highest + 1).top() + inset >= strip.top(),
            "the row the count covers would draw its words past the strip"
        );
    }

    #[test]
    fn the_count_survives_a_filter_that_hides_the_unnamed_row() {
        let ctx = context();
        let mut tracks = vec![track("hidden", "hidden")];
        tracks[0].named = false;
        tracks.extend((1..10).map(|index| track(&format!("t{index}"), "keep")));
        let mut library = library_with(tracks);
        library
            .track_filter
            .edit("spotify:playlist:a")
            .push_str("keep");

        let output = tracklist_frame_in(&ctx, PROBE, &mut library);

        assert!(
            drawn_texts(&output).contains(&"1 of 10 tracks could not be named"),
            "the count covers the whole loaded list, not the visible hits: {:?}",
            drawn_texts(&output)
        );
        assert!(
            !painted_rows(&output).contains(&0),
            "the unnamed row is hidden by the query"
        );
    }

    #[test]
    fn a_row_that_only_touches_the_fold_asks_for_no_cover() {
        // The fold is the viewport's bottom edge, and a row whose top sits on it
        // shares only an edge with the viewport. It is not on screen, so it must
        // not be laid out and must not fetch a cover nothing draws.
        let count = 20;
        let row = 3;
        let ctx = context();
        let tokens = Tokens::DARK;
        let now = Instant::now();
        let tracks: Vec<TrackEntry> = (0..count).map(track_with_cover).collect();
        let mut library = library_with(tracks);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        // Solved the way the view solves it, then read back from the row's own
        // hover wash: the wash is painted at the row's rect.
        let rect = row_rect_for(&ctx, count, row);
        let mut washed = None;
        for _ in 0..12 {
            art.begin_frame();
            let mut output = ctx.run_ui(input(vec![Event::PointerMoved(rect.center())]), |ui| {
                let _ = tracks_view(
                    ui,
                    Table::Full,
                    &mut library,
                    &mut transport,
                    &mut queue,
                    &mut art,
                    now,
                    NOW_MS,
                    &tokens,
                );
            });
            output.textures_delta.clear();
            washed = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::epaint::Shape::Rect(shape) if shape.fill == tokens.palette.row_hover => {
                        Some(shape.rect)
                    }
                    _ => None,
                })
                .or(washed);
        }
        assert_eq!(
            washed,
            Some(rect),
            "a row washes at the rect it is laid out in"
        );

        // The window's height is that row's top, so the fold is its top edge.
        art.begin_frame();
        let mut output = ctx.run_ui(
            sized_input(Vec2::new(PROBE.x, rect.top()), Vec::new()),
            |ui| {
                let _ = tracks_view(
                    ui,
                    Table::Full,
                    &mut library,
                    &mut transport,
                    &mut queue,
                    &mut art,
                    now,
                    NOW_MS,
                    &tokens,
                );
            },
        );
        output.textures_delta.clear();

        let painted: std::collections::HashSet<usize> = painted_rows(&output).into_iter().collect();
        assert_eq!(
            painted,
            (0..row).collect(),
            "only the rows above the fold are laid out"
        );
        let requested: std::collections::HashSet<usize> = art
            .requests(now)
            .into_iter()
            .filter_map(|command| match command {
                Command::Fetch { url } => Some(cover_row(&url)),
                _ => None,
            })
            .collect();
        assert!(
            !requested.contains(&row),
            "the row that only touches the fold asked for a cover: {requested:?}"
        );
        assert_eq!(
            requested, painted,
            "the covers asked for are the rows the window shows"
        );
    }

    #[test]
    fn the_header_and_the_rows_share_a_column_origin() {
        // The header and the rows are solved from one width, so a heading's left
        // edge and its column's left edge are the same screen x. A padding change
        // on one side alone would slide them apart; this reads the header's own
        // painted position rather than re-solving it, so it can tell.
        let ctx = context();
        let mut library = library_with(vec![track("a", "t0")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let row = row_rect_for(&ctx, 1, 0);
        let columns = table_layout::columns(row.width(), Table::Full.columns()).at(row.min);
        for (label, column) in [
            ("Title", columns.title),
            ("Artist", columns.artist),
            ("Album", columns.album),
            ("Added", columns.added),
        ] {
            let painted = heading_left(&ctx, label, &mut library, &mut transport, &mut queue);
            assert!(
                (painted - column.left()).abs() < 0.01,
                "the {label} heading is painted at {painted}, its column starts at {}",
                column.left()
            );
        }
    }

    /// The table sits one gutter inside the pane, so its first heading lines up
    /// under the band's own padding rather than rubbing the panel's edge.
    #[test]
    fn the_heading_sits_one_gutter_inside_the_pane() {
        let ctx = context();
        let mut library = library_with(vec![track("a", "t0")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let painted = heading_left(&ctx, "Title", &mut library, &mut transport, &mut queue);
        let columns = table_layout::columns(PROBE.x - 2.0 * space::GUTTER, Table::Full.columns());
        assert!(
            (painted - (space::GUTTER + columns.title.left())).abs() < 0.01,
            "the title heading is painted at {painted}, one gutter plus its column is {}",
            space::GUTTER + columns.title.left()
        );
    }

    #[test]
    fn below_1000_the_playlist_heads_and_draws_the_artist_but_not_the_album() {
        // The reference drops the album at the same width it reduces the
        // playbar, from the header and from every row, and keeps the artist.
        let ctx = context();
        let mut library = library_with(vec![TrackEntry {
            uri: "a".to_owned(),
            title: "t0".to_owned(),
            artists: "Band".to_owned(),
            album: Some("Record".to_owned()),
            album_uri: None,
            artist_uris: Vec::new(),
            artist_names: Vec::new(),
            added_at: None,
            duration_ms: Some(222_000),
            named: true,
            cover: None,
        }]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Reduced,
                &mut library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &Tokens::DARK,
            );
        });
        output.textures_delta.clear();
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&"ARTIST"),
            "the header must keep the artist heading: {texts:?}"
        );
        assert!(
            texts.contains(&"Band"),
            "the row must keep the artist: {texts:?}"
        );
        assert!(
            !texts.contains(&"ALBUM"),
            "the header must drop the album heading: {texts:?}"
        );
        assert!(
            !texts.contains(&"Record"),
            "the row must drop the album: {texts:?}"
        );
    }

    #[test]
    fn a_long_artist_elides_inside_its_column() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let width = 700.0;
        let columns = table_layout::columns(width, Table::Reduced.columns());
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(width, extent::ROW_H));
        let artist = "The Extremely Long-Named Orchestra of the Northern Coast and Its Friends";
        let row = Row {
            uri: "a",
            number: 1,
            title: "t0",
            artist,
            artist_uri: None,
            album: Some("Record"),
            album_uri: None,
            added: None,
            duration: Some("3:42"),
            state: State::Resting,
            cover: Some(Cover::Missing),
            look: Look::default(),
            menu: false,
            liked: None,
        };

        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = track_row::show(ui, rect, columns, &row, &tokens);
        });
        output.textures_delta.clear();
        let galley = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == artist => {
                    Some(text.galley.clone())
                }
                _ => None,
            })
            .expect("the artist column paints its label");

        assert_eq!(galley.rows.len(), 1, "the artist must not wrap");
        assert!(galley.elided, "the artist must elide");
        assert!(
            galley.rows[0].row.text().ends_with('…'),
            "the elided artist must end in an ellipsis: {:?}",
            galley.rows[0].row.text()
        );
        assert!(
            galley.size().x <= columns.artist.width() + 0.01,
            "the artist is {} wide in a {} column",
            galley.size().x,
            columns.artist.width()
        );
    }

    #[test]
    fn an_empty_track_list_draws_without_panicking() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let mut library = library_with(Vec::new());
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let mut command = None;
        run(&ctx, input(Vec::new()), |ui| {
            command = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });

        assert_eq!(command, None, "an empty list has nothing to activate");
    }

    #[test]
    fn the_hero_heads_a_named_playlist_and_not_an_unnamed_one() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        // A selected playlist whose name the library knows: the band is drawn.
        let mut named = library_named(vec![track("a", "t0")], "Evening");
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut named,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });
        output.textures_delta.clear();
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&"PLAYLIST"),
            "the band's eyebrow must be drawn over a named playlist: {texts:?}"
        );
        assert!(
            texts.contains(&"Evening"),
            "the band must name the playlist: {texts:?}"
        );

        // The same selection with nothing in the library to name it: no band,
        // only the rows.
        let mut unnamed = library_with(vec![track("a", "t0")]);
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut unnamed,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });
        output.textures_delta.clear();
        let texts = drawn_texts(&output);
        assert!(
            !texts.contains(&"PLAYLIST"),
            "no name means no band, only the rows: {texts:?}"
        );
    }

    #[test]
    fn the_hero_play_control_starts_the_first_track_through_the_queue() {
        let ctx = context();
        let mut library = library_named(vec![track("a", "t0"), track("b", "t1")], "Evening");
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        // The click is where the view draws the control, not a probe's geometry.
        let play = play_tile_rect_for(&ctx, "Evening").center();
        let command = click(&ctx, play, &mut library, &mut transport, &mut queue);

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "a".to_owned()
            })
        );
        assert!(
            queue.can_next(),
            "the hero's play must start the queue, not only the transport"
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("a")
        );
    }

    #[test]
    fn a_double_click_on_the_second_row_plays_it_while_the_band_is_drawn() {
        let ctx = context();
        let mut library = library_named(vec![track("a", "t0"), track("b", "t1")], "Evening");
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        // The band pushes the rows down, so the only solve that sees its offset
        // is the one below it. Clicking the second row must start the second
        // track: if the offset were dropped, the same point would land on the
        // first.
        let second = row_rect_below_hero(&ctx, TALL, 2, 1, "Evening").center();
        let command = clicks(
            TALL,
            &ctx,
            second,
            2,
            &mut library,
            &mut transport,
            &mut queue,
        );

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "b".to_owned()
            })
        );
        assert!(
            !queue.can_next(),
            "the second of two rows was started, not the first"
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("b")
        );
    }

    #[test]
    fn idle_view_reads_the_playlists_load() {
        // A list nobody has asked for yet and a list in flight are one state:
        // neither has anything to draw.
        assert_eq!(idle_view(&Library::default()), IdleView::Loading);

        let mut loading = Library::default();
        loading.apply(&Update::SignedIn);
        assert_eq!(idle_view(&loading), IdleView::Loading);
    }

    #[test]
    fn idle_view_failed_playlists_win_for_every_chip() {
        for kind in library_filter::ROW {
            let mut library = library_idle(Vec::new());
            library.root_filter.choose(kind);
            library.apply(&Update::PlaylistFailed(Failure::Load));
            assert_eq!(
                idle_view(&library),
                IdleView::Failed(Failure::Load),
                "{kind:?} must still report the page's failure"
            );
        }
    }

    #[test]
    fn idle_view_all_shows_playlists_and_shows() {
        // The four kinds in one column are pinned by `library_filter`; this is
        // the root reading them together, and settling.
        let mut library = library_idle(vec![playlist("a", "A")]);
        library.apply(&Update::FollowedShows(vec![show("s", "S")]));
        library.apply(&Update::FollowedArtists(Vec::new()));
        library.apply(&Update::SavedAlbums(Vec::new()));
        let IdleView::Grid { items, note } = idle_view(&library) else {
            panic!("a ready root is a grid: {:?}", idle_view(&library));
        };
        assert_eq!(items.len(), 2, "playlists first, then shows");
        assert!(matches!(items[0], Item::Playlist(entry) if entry.uri == "a"));
        assert!(matches!(&items[1], Item::Show(entry) if entry.uri == "s"));
        assert_eq!(note, LoadNote::None);
    }

    /// All draws four reads, so its note names the first still owed, in the
    /// order the cards are drawn.
    #[test]
    fn idle_view_all_names_the_first_read_it_is_still_owed() {
        let mut library = library_idle(vec![playlist("a", "A")]);
        library.apply(&Update::FollowedShows(Vec::new()));
        let IdleView::Grid { note, .. } = idle_view(&library) else {
            panic!("a ready root is a grid: {:?}", idle_view(&library));
        };
        assert_eq!(
            note,
            LoadNote::Loading(FollowedRead::Artists),
            "the shows are in, so the artists are the first owed"
        );

        library.apply(&Update::FollowedArtists(Vec::new()));
        let IdleView::Grid { note, .. } = idle_view(&library) else {
            panic!("a ready root is a grid: {:?}", idle_view(&library));
        };
        assert_eq!(note, LoadNote::Loading(FollowedRead::Albums));

        library.apply(&Update::SavedAlbums(Vec::new()));
        let IdleView::Grid { note, .. } = idle_view(&library) else {
            panic!("a ready root is a grid: {:?}", idle_view(&library));
        };
        assert_eq!(note, LoadNote::None);
    }

    #[test]
    fn idle_view_all_with_no_playlists_waits_on_the_followed_list() {
        let mut library = library_idle(Vec::new());
        let _ = library.followed_request();
        let IdleView::Grid { items, note } = idle_view(&library) else {
            panic!("the shows are still coming: {:?}", idle_view(&library));
        };
        assert!(items.is_empty());
        assert_eq!(note, LoadNote::Loading(FollowedRead::Shows));

        library.apply(&Update::FollowedShowsFailed(Failure::Followed));
        let IdleView::Grid { note, .. } = idle_view(&library) else {
            panic!("a failed list is its own note: {:?}", idle_view(&library));
        };
        assert_eq!(
            note,
            LoadNote::Failed(FollowedRead::Shows, Failure::Followed)
        );

        let mut settled = library_idle(Vec::new());
        settled.apply(&Update::FollowedShows(Vec::new()));
        settled.apply(&Update::FollowedArtists(Vec::new()));
        settled.apply(&Update::SavedAlbums(Vec::new()));
        assert_eq!(idle_view(&settled), IdleView::Empty(RootKind::All));
    }

    #[test]
    fn idle_view_playlists_hides_shows_and_their_loading() {
        let mut library = library_idle(vec![playlist("a", "A")]);
        library.root_filter.choose(RootKind::Playlists);
        // The followed list is in flight; the playlists chip does not wait on it.
        let _ = library.followed_request();
        let IdleView::Grid { items, note } = idle_view(&library) else {
            panic!("the playlists chip is a grid: {:?}", idle_view(&library));
        };
        assert_eq!(items.len(), 1);
        assert_eq!(note, LoadNote::None);
    }

    #[test]
    fn idle_view_podcasts_loading_failed_and_empty() {
        let mut library = library_idle(vec![playlist("a", "A")]);
        library.root_filter.choose(RootKind::Podcasts);

        let _ = library.followed_request();
        let IdleView::Grid { items, note } = idle_view(&library) else {
            panic!(
                "a loading followed list is a grid: {:?}",
                idle_view(&library)
            );
        };
        assert!(
            items.is_empty(),
            "the playlists chip is not the podcasts one"
        );
        assert_eq!(note, LoadNote::Loading(FollowedRead::Shows));

        library.apply(&Update::FollowedShowsFailed(Failure::Load));
        let IdleView::Grid { items, note } = idle_view(&library) else {
            panic!(
                "a failed followed list is a grid: {:?}",
                idle_view(&library)
            );
        };
        assert!(items.is_empty());
        assert_eq!(note, LoadNote::Failed(FollowedRead::Shows, Failure::Load));

        let mut empty = library_idle(vec![playlist("a", "A")]);
        empty.root_filter.choose(RootKind::Podcasts);
        empty.apply(&Update::FollowedShows(Vec::new()));
        assert_eq!(
            idle_view(&empty),
            IdleView::Empty(RootKind::Podcasts),
            "nothing to show is the chip's own empty line"
        );
    }

    #[test]
    fn a_click_on_a_card_loads_its_playlist() {
        let ctx = context();
        let mut library = library_idle(vec![playlist("a", "A"), playlist("b", "B")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        // The click is where the view draws the first card, not a probe's
        // geometry.
        let centre = card_rect_for(&ctx, 0).center();
        let command = click(&ctx, centre, &mut library, &mut transport, &mut queue);

        assert_eq!(
            command,
            Some(Command::LoadTracks {
                playlist: "a".to_owned()
            })
        );
        assert_eq!(library.selected(), Some("a"));
        assert_eq!(library.tracks, Load::Loading);
    }

    #[test]
    fn a_card_click_asks_for_tracks_once() {
        // The whole click sequence twice: the first click selects the playlist
        // and starts its load, and the second lands on the progress the view is
        // showing instead of a card. Counting the commands the view reports is
        // the assertion the single command in `a_click_on_a_card_loads_its_
        // playlist` cannot make.
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut library = library_idle(vec![playlist("a", "A")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let centre = card_rect_for(&ctx, 0).center();

        run(&ctx, input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });

        let mut loads = 0;
        for _ in 0..2 {
            for event in crate::view::test_support::click_events(centre) {
                run(&ctx, input(vec![event]), |ui| {
                    if let Some(Command::LoadTracks { .. }) = tracks_view(
                        ui,
                        Table::Full,
                        &mut library,
                        &mut transport,
                        &mut queue,
                        &mut Art::default(),
                        Instant::now(),
                        NOW_MS,
                        &tokens,
                    ) {
                        loads += 1;
                    }
                });
            }
        }

        assert_eq!(loads, 1, "two clicks on the same card must load it once");
        assert_eq!(library.selected(), Some("a"));
    }

    #[test]
    fn the_tile_height_the_grid_solves_contains_the_last_line_the_tile_paints() {
        let ctx = context();
        let tokens = Tokens::DARK;
        // A realistic tile: the grid's own solve at the probe's width, so the
        // rect's height is `tile_w + media::extra_height`, the number the grid is
        // given.
        let rect = card_rect_for(&ctx, 0);
        let media = crate::view::media::Media {
            uri: "a",
            name: "Evening",
            kind: "Playlist",
            sub: "Playlist · Vela Moreno",
            size: "62 songs",
            round: false,
            shelf: SidebarKind::Playlists,
            playable: true,
            cover: Cover::Missing,
            picture: hero::Picture::Cover,
        };
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = crate::view::media::tile(ui, rect, &media, &tokens);
        });
        output.textures_delta.clear();

        let last = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) => Some(text.pos.y + text.galley.size().y),
                _ => None,
            })
            .max_by(f32::total_cmp)
            .expect("the card paints a name line and a kind line");

        // The grid's card must contain the painter's lowest line, and it must
        // leave no more than the layout's own `S2` below it. The first half
        // catches an `extra_height` short enough to spill the line out of the
        // card; the second catches the 16 points of slack left when the padding
        // was counted against the art as well as against the card.
        assert!(
            last <= rect.bottom(),
            "the last painted line ends at {last}, past the card's {}",
            rect.bottom()
        );
        assert!(
            rect.bottom() - last <= space::S2 + 0.5,
            "the card leaves {} below its last line, more than the {} it lays out",
            rect.bottom() - last,
            space::S2
        );
    }

    #[test]
    fn to_playlists_returns_to_the_grid() {
        let mut library = library_idle(vec![playlist("a", "A")]);
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t", "t")],
            cover: None,
            access: None,
        });
        library.to_playlists();

        let IdleView::Grid { items, .. } = idle_view(&library) else {
            panic!(
                "to_playlists must return to the grid: {:?}",
                idle_view(&library)
            );
        };
        assert_eq!(items.len(), 1);
        assert!(matches!(&items[0], Item::Playlist(entry) if entry.uri == "a"));
        assert!(
            matches!(library.tracks, Load::Ready(_)),
            "the grid draws while the tracks stay ready"
        );
    }

    #[test]
    fn the_root_draws_the_grid_while_the_tracks_stay_ready() {
        let ctx = context();
        let mut library = library_idle(vec![playlist("a", "A")]);
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t0", "t0")],
            cover: None,
            access: None,
        });
        library.to_playlists();

        assert_eq!(library.page(), None);
        assert!(
            matches!(library.tracks, Load::Ready(_)),
            "the tracks stay ready behind the root"
        );

        let texts = painted_lines(&ctx, Table::Full, &mut library);

        assert!(
            texts.contains(&"A".to_owned()),
            "the grid must draw its card: {texts:?}"
        );
        assert!(
            !texts.contains(&"t0".to_owned()),
            "the ready tracklist must not draw at the root: {texts:?}"
        );
    }

    #[test]
    fn a_click_on_the_album_cell_opens_the_album() {
        let ctx = context();
        let mut library = library_with(vec![linkable("a")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let words = words_of(&ctx, &mut library, "Record");
        let command = click(
            &ctx,
            words.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );

        assert_eq!(
            library.page(),
            Some(&Page::Album("spotify:album:rec".to_owned())),
            "a single click on the album cell's words must open the album"
        );
        assert_eq!(
            command,
            Some(Command::LoadAlbum {
                album: "spotify:album:rec".to_owned()
            }),
            "opening an album asks the worker for it"
        );
        assert!(!queue.can_next(), "opening a page must not start the queue");
        assert_eq!(transport.now_playing, None, "opening a page must not play");
    }

    #[test]
    fn a_click_in_the_album_cells_padding_selects_the_row() {
        // The link is the painted words, not the cell: a click past the end of
        // "Record" reaches the row, which selects it.
        let ctx = context();
        let mut library = library_with(vec![linkable("a")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.album);
        let words = words_of(&ctx, &mut library, "Record");
        let padding = Pos2::new(cell.right() - space::S3, cell.center().y);
        assert!(
            words.right() < padding.x && padding.x < cell.right(),
            "the probe at x={} must be past \"Record\", which ends at x={}",
            padding.x,
            words.right()
        );

        let command = click(&ctx, padding, &mut library, &mut transport, &mut queue);
        assert_eq!(command, None, "a click past the words is the row's own");
        assert_eq!(
            library.page(),
            Some(&Page::Playlist("spotify:playlist:a".to_owned())),
            "a click past the album's words must not navigate"
        );
        assert!(!queue.can_next(), "a click past the words must not play");

        // The cell still belongs to the row, so a double click there plays.
        let command = double_click(&ctx, padding, &mut library, &mut transport, &mut queue);
        assert_eq!(
            command,
            Some(Command::Play {
                uri: "a".to_owned()
            }),
            "a double click past the words must play the row"
        );
    }

    #[test]
    fn a_click_on_the_artist_cell_opens_the_first_artist() {
        let ctx = context();
        let mut library = library_with(vec![linkable("a")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let words = words_of(&ctx, &mut library, "Band");
        let command = click(
            &ctx,
            words.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );

        assert_eq!(
            library.page(),
            Some(&Page::Artist("spotify:artist:band".to_owned())),
            "a single click on the artist cell's words must open the first artist"
        );
        assert_eq!(
            command,
            Some(Command::LoadArtist {
                artist: "spotify:artist:band".to_owned()
            }),
            "opening an artist asks the worker for it"
        );
        assert!(!queue.can_next());
        assert_eq!(transport.now_playing, None);
    }

    #[test]
    fn a_click_in_the_artist_cells_padding_selects_the_row() {
        let ctx = context();
        let mut library = library_with(vec![linkable("a")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.artist);
        let words = words_of(&ctx, &mut library, "Band");
        let padding = Pos2::new(cell.right() - space::S3, cell.center().y);
        assert!(
            words.right() < padding.x && padding.x < cell.right(),
            "the probe at x={} must be past \"Band\", which ends at x={}",
            padding.x,
            words.right()
        );

        let command = click(&ctx, padding, &mut library, &mut transport, &mut queue);
        assert_eq!(command, None, "a click past the words is the row's own");
        assert_eq!(
            library.page(),
            Some(&Page::Playlist("spotify:playlist:a".to_owned())),
            "a click past the artist's words must not navigate"
        );
        assert!(!queue.can_next(), "a click past the words must not play");

        let command = double_click(&ctx, padding, &mut library, &mut transport, &mut queue);
        assert_eq!(
            command,
            Some(Command::Play {
                uri: "a".to_owned()
            }),
            "a double click past the words must play the row"
        );
    }

    #[test]
    fn a_link_cell_points_the_hand_and_brightens_its_label() {
        // A screenshot shows the colour but not the cursor the window reports, so
        // both of the hover's effects are pinned here.
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut library = library_with(vec![linkable("a")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.album);
        let words = words_of(&ctx, &mut library, "Record");

        let mut frame = |events: Vec<Event>| {
            let mut output = run(&ctx, input(events), |ui| {
                let _ = tracks_view(
                    ui,
                    Table::Full,
                    &mut library,
                    &mut transport,
                    &mut queue,
                    &mut Art::default(),
                    Instant::now(),
                    NOW_MS,
                    &tokens,
                );
            });
            output.textures_delta.clear();
            let ink = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::epaint::Shape::Text(text) if text.galley.text() == "Record" => {
                        Some(text.fallback_color)
                    }
                    _ => None,
                })
                .expect("the album label is painted");
            (output.platform_output.cursor_icon, ink)
        };

        // At rest the label is muted and the cursor is the window's own.
        let (rest_cursor, rest_ink) = frame(Vec::new());
        assert_eq!(rest_ink, tokens.palette.text_muted);
        assert_ne!(rest_cursor, egui::CursorIcon::PointingHand);

        // Resting on the words points the hand and brightens them to the body
        // colour, over the reveal's own time.
        for _ in 0..12 {
            frame(vec![Event::PointerMoved(words.center())]);
        }
        let (hover_cursor, hover_ink) = frame(vec![Event::PointerMoved(words.center())]);
        assert_eq!(hover_cursor, egui::CursorIcon::PointingHand);
        assert_eq!(hover_ink, tokens.palette.text);

        // Resting past them is the row's: no hand, and the label goes muted.
        let padding = Pos2::new(cell.right() - space::S3, cell.center().y);
        assert!(
            words.right() < padding.x && padding.x < cell.right(),
            "the probe at x={} must be past \"Record\", which ends at x={}",
            padding.x,
            words.right()
        );
        for _ in 0..12 {
            frame(vec![Event::PointerMoved(padding)]);
        }
        let (padding_cursor, padding_ink) = frame(vec![Event::PointerMoved(padding)]);
        assert_ne!(
            padding_cursor,
            egui::CursorIcon::PointingHand,
            "the empty part of the cell must not point the hand"
        );
        assert_eq!(padding_ink, tokens.palette.text_muted);
    }

    #[test]
    fn the_row_keeps_its_hover_over_a_link_cell() {
        // A link cell's interact is created after the row's, and egui marks only
        // the top hit widget as hovered, so the row's own `hovered()` goes false
        // over the link's words. The row must read where the pointer is instead,
        // or its highlight, its index play glyph and its like all vanish there.
        // The fill is the visible half of that; the glyph and the like follow the
        // same `revealed` value. The label's ink says which widget the hit test
        // gave the pointer to.
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut library = library_with(vec![linkable("a")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let row = row_rect_for(&ctx, 1, 0);
        let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.album);
        let words = words_of(&ctx, &mut library, "Record");

        // The frame's fill behind this row and the album label's ink, read back
        // from the shapes painted.
        let mut frame = |events: Vec<Event>| {
            let output = run(&ctx, input(events), |ui| {
                let _ = tracks_view(
                    ui,
                    Table::Full,
                    &mut library,
                    &mut transport,
                    &mut queue,
                    &mut Art::default(),
                    Instant::now(),
                    NOW_MS,
                    &tokens,
                );
            });
            let fill = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::epaint::Shape::Rect(shape) if shape.rect == row => Some(shape.fill),
                    _ => None,
                });
            let ink = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::epaint::Shape::Text(text) if text.galley.text() == "Record" => {
                        Some(text.fallback_color)
                    }
                    _ => None,
                });
            (fill, ink)
        };

        // With the pointer away the row paints no fill at all.
        assert_eq!(
            frame(Vec::new()),
            (None, Some(tokens.palette.text_muted)),
            "a resting row paints no fill"
        );

        // Resting on the link's words lights the row to the same fill the title
        // would, while the link itself takes the pointer's hover: both are true
        // at once, which is the tie this pins.
        for _ in 0..12 {
            frame(vec![Event::PointerMoved(words.center())]);
        }
        assert_eq!(
            frame(vec![Event::PointerMoved(words.center())]),
            (Some(tokens.palette.row_hover), Some(tokens.palette.text)),
            "the row must keep its hover fill over the album link's words"
        );

        // Past the words the row owns the point, so the fill stays and the link
        // is not hovered.
        let padding = Pos2::new(cell.right() - space::S3, cell.center().y);
        assert!(
            words.right() < padding.x && padding.x < cell.right(),
            "the probe at x={} must be past \"Record\", which ends at x={}",
            padding.x,
            words.right()
        );
        for _ in 0..12 {
            frame(vec![Event::PointerMoved(padding)]);
        }
        assert_eq!(
            frame(vec![Event::PointerMoved(padding)]),
            (
                Some(tokens.palette.row_hover),
                Some(tokens.palette.text_muted)
            ),
            "the row must keep its hover fill past the album link's words"
        );
    }

    #[test]
    fn a_double_click_in_the_album_cell_opens_it_without_playing() {
        let ctx = context();
        let mut library = library_with(vec![linkable("a")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let words = words_of(&ctx, &mut library, "Record");
        let command = double_click(
            &ctx,
            words.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );

        assert_eq!(
            library.page(),
            Some(&Page::Album("spotify:album:rec".to_owned())),
            "the second click lands on the album page, not on the row"
        );
        assert_eq!(
            command,
            Some(Command::LoadAlbum {
                album: "spotify:album:rec".to_owned()
            }),
            "the album page's load is the command, not a play"
        );
        assert!(!queue.can_next(), "a link cell must never start the queue");
        assert_eq!(transport.now_playing, None);
    }

    #[test]
    fn a_double_click_on_the_title_still_plays() {
        let ctx = context();
        let mut library = library_with(vec![linkable("a"), linkable("b")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let cell = cell_rect_for(&ctx, 2, 0, |columns| columns.title);
        let command = double_click(
            &ctx,
            cell.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "a".to_owned()
            })
        );
        assert_eq!(
            library.page(),
            Some(&Page::Playlist("spotify:playlist:a".to_owned()))
        );
        assert!(
            queue.can_next(),
            "the double-clicked row must start the queue"
        );
    }

    #[test]
    fn a_row_without_an_album_uri_keeps_a_plain_cell() {
        let ctx = context();
        let mut library = library_with(vec![TrackEntry {
            album_uri: None,
            ..linkable("a")
        }]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.album);
        let command = click(
            &ctx,
            cell.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );
        assert_eq!(
            command, None,
            "a cell with no URI must take no click of its own"
        );
        assert_eq!(
            library.page(),
            Some(&Page::Playlist("spotify:playlist:a".to_owned())),
            "a plain cell must not navigate"
        );

        // The cell still belongs to the row, so a double click there plays.
        let command = double_click(
            &ctx,
            cell.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );
        assert_eq!(
            command,
            Some(Command::Play {
                uri: "a".to_owned()
            })
        );
    }

    #[test]
    fn an_album_with_no_displayed_name_is_not_a_link() {
        // The labelling can resolve a gid for an album the row never names. An
        // empty cell painted nothing, so it must offer no hit area either.
        let ctx = context();
        let mut library = library_with(vec![TrackEntry {
            album: Some(String::new()),
            ..linkable("a")
        }]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.album);
        let command = click(
            &ctx,
            cell.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );

        assert_eq!(command, None);
        assert_eq!(
            library.page(),
            Some(&Page::Playlist("spotify:playlist:a".to_owned())),
            "an unnamed album must not navigate"
        );
    }

    #[test]
    fn the_enter_key_plays_with_the_pointer_on_a_link_cell() {
        // The pointer's hover gates only the pointer's double click. A keyboard
        // activation of the focused row is never taken by a link. Enter is the
        // row's key: a space belongs to the transport, so it reaches no row.
        for pressed in [egui::Key::Enter, egui::Key::Space] {
            let ctx = context();
            let tokens = Tokens::DARK;
            let mut library = library_with(vec![linkable("a")]);
            let mut transport = Transport::default();
            let mut queue = Queue::default();
            let words = words_of(&ctx, &mut library, "Record");

            // Warm up so focus traversal and the hit test read a previous pass.
            // The action row's widgets come before the list, so the keyboard is
            // walked on until it lands on a row rather than counted out.
            let pass = |library: &mut Library,
                        transport: &mut Transport,
                        queue: &mut Queue,
                        events: Vec<Event>| {
                run(&ctx, input(events), |ui| {
                    let _ = tracks_view(
                        ui,
                        Table::Full,
                        library,
                        transport,
                        queue,
                        &mut Art::default(),
                        Instant::now(),
                        NOW_MS,
                        &tokens,
                    );
                })
            };
            let warm = pass(&mut library, &mut transport, &mut queue, Vec::new());
            // The action row's box at rest: the field sits on its centre line.
            let bar_bottom = field_at(&warm).center().y + crate::theme::actionbar::REST_H / 2.0;
            let mut focused = None;
            for _ in 0..8 {
                pass(
                    &mut library,
                    &mut transport,
                    &mut queue,
                    vec![key(egui::Key::Tab)],
                );
                let landed = ctx.memory(|memory| memory.focused());
                if landed.is_some_and(|id| {
                    // A row of the list: a row's height, below the action row.
                    ctx.read_response(id).is_some_and(|row| {
                        (row.rect.height() - extent::ROW_H).abs() < 0.01
                            && row.rect.top() >= bar_bottom - 0.01
                    })
                }) {
                    focused = landed;
                    break;
                }
            }
            assert!(focused.is_some(), "the keyboard never reached a row");

            // Rest the pointer on the album cell's words, then press the key.
            let mut command = None;
            for events in [
                vec![Event::PointerMoved(words.center())],
                vec![key(pressed)],
            ] {
                let command = &mut command;
                let library = &mut library;
                let transport = &mut transport;
                let queue = &mut queue;
                run(&ctx, input(events), |ui| {
                    if let Some(next) = tracks_view(
                        ui,
                        Table::Full,
                        library,
                        transport,
                        queue,
                        &mut Art::default(),
                        Instant::now(),
                        NOW_MS,
                        &tokens,
                    ) {
                        *command = Some(next);
                    }
                });
            }

            let expected = (pressed == egui::Key::Enter).then(|| Command::Play {
                uri: "a".to_owned(),
            });
            assert_eq!(
                command, expected,
                "{pressed:?} on the focused row, with the pointer on a link"
            );
            assert_eq!(
                library.page(),
                Some(&Page::Playlist("spotify:playlist:a".to_owned())),
                "{pressed:?} must not navigate"
            );
        }
    }

    /// The album's own page data for the view tests: one track, so a page can be
    /// told apart from the playlist behind it by its rows alone.
    fn album_page(uri: &str, name: &str) -> AlbumDetail {
        AlbumDetail {
            uri: uri.to_owned(),
            name: name.to_owned(),
            artists: "Band".to_owned(),
            artist_uri: None,
            year: Some(1997),
            cover: None,
            tracks: vec![TrackEntry {
                artists: "Band".to_owned(),
                ..track("spotify:track:t1", "Album Song")
            }],
        }
    }

    #[test]
    fn back_from_an_album_page_redraws_the_playlist() {
        let ctx = context();
        let mut library = library_with(vec![linkable("a")]);
        assert_eq!(
            library.open(Page::Album("spotify:album:rec".to_owned())),
            Some(Command::LoadAlbum {
                album: "spotify:album:rec".to_owned()
            })
        );
        library.apply(&Update::Album(album_page("spotify:album:rec", "Record")));
        let texts = painted_lines(&ctx, Table::Full, &mut library);
        assert!(
            texts.contains(&"Record".to_owned()),
            "the album page draws its own band: {texts:?}"
        );
        assert!(
            !texts.contains(&"t0".to_owned()),
            "the playlist behind it is not what is drawn: {texts:?}"
        );

        assert_eq!(
            library.back(),
            None,
            "a ready playlist with a matching wanted load needs no command"
        );

        let texts = painted_lines(&ctx, Table::Full, &mut library);
        assert!(
            texts.contains(&"t0".to_owned()),
            "Back must redraw the ready tracklist: {texts:?}"
        );
        assert!(
            !texts.contains(&"Album Song".to_owned()),
            "the album's rows are gone: {texts:?}"
        );
    }

    #[test]
    fn an_album_page_draws_the_album_view() {
        let ctx = context();
        let mut library = library_idle(vec![playlist("a", "A")]);
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t0", "t0")],
            cover: None,
            access: None,
        });
        assert_eq!(
            library.open(Page::Album("spotify:album:x".to_owned())),
            Some(Command::LoadAlbum {
                album: "spotify:album:x".to_owned()
            })
        );
        library.apply(&Update::Album(album_page(
            "spotify:album:x",
            "Signal Decay",
        )));

        let texts = painted_lines(&ctx, Table::Full, &mut library);

        assert!(
            texts.contains(&"ALBUM".to_owned()),
            "the band's eyebrow: {texts:?}"
        );
        assert!(
            texts.contains(&"Signal Decay".to_owned()),
            "the band names the album: {texts:?}"
        );
        assert!(
            texts.contains(&"Album Song".to_owned()),
            "the album's table lists its own track: {texts:?}"
        );
        assert!(
            !texts.contains(&"t0".to_owned()),
            "the playlist behind the page is not what is drawn: {texts:?}"
        );
    }

    #[test]
    fn the_frame_loop_requests_only_visible_covers() {
        let ctx = context();
        let tokens = Tokens::DARK;
        // A thousand distinct covers in a short window: only the dozen rows the
        // viewport can show are ever laid out, even as the list is scrolled.
        let tracks: Vec<TrackEntry> = (0..1_000).map(track_with_cover).collect();
        let mut library = library_with(tracks);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let now = Instant::now();
        let mut requested: std::collections::HashMap<CoverUrl, usize> =
            std::collections::HashMap::new();
        let mut painted: std::collections::HashSet<usize> = std::collections::HashSet::new();
        let centre = Pos2::new(PROBE.x / 2.0, PROBE.y / 2.0);

        // Scroll from the top down. Every requested cover must belong to a row
        // the window painted in some frame, and the outstanding set must never
        // exceed the cap.
        for step in 0..240 {
            art.begin_frame();
            let mut events = vec![Event::PointerMoved(centre)];
            if step > 0 {
                events.push(Event::MouseWheel {
                    unit: MouseWheelUnit::Point,
                    delta: vec2(0.0, -extent::ROW_H),
                    phase: TouchPhase::Move,
                    source: MouseWheelSource::Unknown,
                    modifiers: Modifiers::NONE,
                });
            }
            let mut output = ctx.run_ui(input(events), |ui| {
                let _ = tracks_view(
                    ui,
                    Table::Full,
                    &mut library,
                    &mut transport,
                    &mut queue,
                    &mut art,
                    now,
                    NOW_MS,
                    &tokens,
                );
            });
            output.textures_delta.clear();
            painted.extend(painted_rows(&output));

            let commands = art.requests(now);
            assert!(
                art.outstanding() <= crate::cover::MAX_IN_FLIGHT,
                "{} fetches outstanding",
                art.outstanding()
            );
            for command in commands {
                if let Command::Fetch { url } = command {
                    requested.insert(url.clone(), cover_row(&url));
                    // Answer it, so the next frame can ask for the next row.
                    art.apply(
                        &Update::Cover {
                            url,
                            image: image(),
                        },
                        now,
                    );
                }
            }
        }

        assert!(!requested.is_empty(), "the visible rows must be fetched");
        assert!(
            painted.iter().copied().max().unwrap_or(0) > 50,
            "the frame loop must scroll through the list to exercise the cap"
        );
        // The loop stops one row short of painting the row that enters at the
        // fold, so a request may name it; nothing further off may be asked for.
        let furthest = painted.iter().copied().max().unwrap_or(0);
        for (url, row) in &requested {
            assert!(
                painted.contains(row) || *row <= furthest + 1,
                "{url:?} was requested for row {row}, which no frame ever painted"
            );
        }
    }

    #[test]
    fn a_grid_card_switches_to_the_context_image_when_it_arrives() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let first = cover_url("first");
        let context_cover = cover_url("context");
        let mut library = library_idle(vec![playlist_with_cover("a", "A", first.clone())]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let now = Instant::now();

        art.begin_frame();
        run(&ctx, input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
                now,
                NOW_MS,
                &tokens,
            );
        });
        assert_eq!(fetch_urls(art.requests(now)), vec![first.clone()]);

        // A context image for an unselected playlist still upgrades its entry,
        // because the upgrade is keyed by the update's own playlist.
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: Vec::new(),
            cover: Some(context_cover.clone()),
            access: None,
        });

        art.begin_frame();
        run(&ctx, input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
                now,
                NOW_MS,
                &tokens,
            );
        });
        assert_eq!(fetch_urls(art.requests(now)), vec![context_cover.clone()]);
    }

    #[test]
    fn the_band_waits_for_the_context_image_and_then_shows_it() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let first = cover_url("first");
        let context_cover = cover_url("context");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist_with_cover(
            "a",
            "Evening",
            first.clone(),
        )]));
        library.select("a");
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let now = Instant::now();

        // While the tracks load, the band is drawn waiting and asks for nothing:
        // it must not fetch the first-track cover it is about to replace.
        art.begin_frame();
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
                now,
                NOW_MS,
                &tokens,
            );
        });
        output.textures_delta.clear();
        assert!(
            drawn_texts(&output).contains(&"PLAYLIST"),
            "the band must head a loading playlist"
        );
        assert!(art.requests(now).is_empty(), "no cover yet");

        // The first page lands with the context image: the entry is upgraded and
        // the band wants it on the same frame.
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t", "t")],
            cover: Some(context_cover.clone()),
            access: None,
        });
        art.begin_frame();
        run(&ctx, input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
                now,
                NOW_MS,
                &tokens,
            );
        });
        assert_eq!(fetch_urls(art.requests(now)), vec![context_cover.clone()]);
    }

    #[test]
    fn the_band_keeps_an_already_held_cover_while_the_tracks_reload() {
        let ctx = context();
        let cover = cover_url("held");
        let entry = playlist_with_cover("a", "Evening", cover.clone());
        let now = Instant::now();
        let mut art = Art::default();

        // An earlier visit to the playlist fetched and held its cover.
        art.begin_frame();
        assert_eq!(art.look(Some(&cover), now), Cover::Loading);
        assert_eq!(fetch_urls(art.requests(now)), vec![cover.clone()]);
        art.apply(
            &Update::Cover {
                url: cover.clone(),
                image: image(),
            },
            now,
        );
        art.upload(&ctx);
        assert!(matches!(art.look(Some(&cover), now), Cover::Ready(_)));

        // Re-selecting the playlist puts the tracks back into `Loading`. The
        // band still draws the picture it already holds rather than flashing the
        // placeholder, and does not fetch it again.
        art.begin_frame();
        assert!(
            matches!(
                band_cover_for(&entry, false, &mut art, now),
                Cover::Ready(_)
            ),
            "the band keeps the cover it already holds"
        );
        assert!(
            fetch_urls(art.requests(now)).is_empty(),
            "a held cover is not fetched again"
        );

        // A playlist whose cover is not held still waits without fetching it.
        let other = playlist_with_cover("b", "Other", cover_url("unheld"));
        art.begin_frame();
        assert_eq!(band_cover_for(&other, false, &mut art, now), Cover::Loading);
        assert!(fetch_urls(art.requests(now)).is_empty());
    }

    /// The band's worded parts below: 62 tracks of 223_548 ms sum to 13_859_976
    /// ms, which the byline rounds to `3 hr 51 min`.
    const DESCRIPTION: &str = "Windows down, city behind you.";
    const META: &str = "Made by alice \u{b7} 62 songs \u{b7} 3 hr 51 min";
    /// Long enough that two rows of the band's column cannot hold it.
    const LONG_DESCRIPTION: &str = "Windows down, city behind you. Updated every \
        Sunday, and again whenever the mood takes me, with no plan and no promise, \
        only the long way home through the sodium light of the ring road, past the \
        depot and the all-night garage, until the city runs out.";

    /// A band whose description, owner and tracks are all known: the widest case
    /// the tests below read.
    fn library_full() -> Library {
        library_detailed(
            timed_tracks(62, 223_548),
            "Evening",
            Some(DESCRIPTION),
            Some("alice"),
        )
    }

    #[test]
    fn the_band_draws_the_description_and_the_owner() {
        let ctx = context();
        let mut library = library_full();
        let output = tracklist_frame(&ctx, Table::Full, &mut library);
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&DESCRIPTION),
            "the band must draw the description: {texts:?}"
        );
        assert!(
            texts.contains(&META),
            "the band must draw the owner ahead of the count and the total: {texts:?}"
        );
    }

    #[test]
    fn the_band_stacks_the_eyebrow_the_title_the_description_and_the_meta() {
        let ctx = context();
        let mut library = library_full();
        let output = tracklist_frame(&ctx, Table::Full, &mut library);
        let drawn = drawn_galleys(&output);
        let eyebrow = galley_top(&drawn, "PLAYLIST");
        let title = galley_top(&drawn, "Evening");
        let description = galley_top(&drawn, DESCRIPTION);
        let meta = galley_top(&drawn, META);
        assert!(
            eyebrow < title && title < description && description < meta,
            "the stack reads eyebrow, title, description, meta downwards: \
             {eyebrow} {title} {description} {meta}"
        );
    }

    #[test]
    fn a_playlist_without_a_description_or_owner_draws_neither_and_leaves_no_gap() {
        let ctx = context();
        let mut library = library_detailed(timed_tracks(2, 1_000), "Evening", None, None);
        let output = tracklist_frame(&ctx, Table::Full, &mut library);
        let texts = drawn_texts(&output);
        assert!(
            !texts.iter().any(|text| text.starts_with("Made by")),
            "no owner means no byline: {texts:?}"
        );
        assert!(
            !texts.contains(&DESCRIPTION),
            "no description means no description line: {texts:?}"
        );

        // With no description the title and the meta are one stack gap apart,
        // not two: an absent description must not leave its own space behind.
        let drawn = drawn_galleys(&output);
        let title_bottom = galley_top(&drawn, "Evening") + galley_height(&drawn, "Evening");
        let meta = galley_top(&drawn, "2 songs \u{b7} 0 min");
        assert!(
            (meta - title_bottom - space::S2).abs() < 0.01,
            "the gap between the title and the meta is {} with no description, \
             not the stack's own {}",
            meta - title_bottom,
            space::S2
        );
    }

    #[test]
    fn a_description_without_an_owner_or_tracks_draws_only_the_description() {
        let ctx = context();
        // The listing names a description but no owner, and the tracks are not
        // asked for: the band has one byline and no meta line at all.
        let mut library = library_listed("Evening", Some(DESCRIPTION), None);
        let output = tracklist_frame(&ctx, Table::Full, &mut library);
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&DESCRIPTION),
            "the description draws with no owner and no tracks: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text.starts_with("Made by")),
            "no owner means no byline: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text.contains("songs")),
            "no ready tracks means no count: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text.contains('\u{b7}')),
            "the meta line is absent, not drawn empty: {texts:?}"
        );
        assert_eq!(
            texts.len(),
            3,
            "the band paints the eyebrow, the title and the description, and
             nothing where the meta would be: {texts:?}"
        );
    }

    #[test]
    fn the_band_names_the_owner_before_the_tracks_are_ready() {
        let ctx = context();
        let mut library = library_listed("Evening", None, Some("alice"));
        let output = tracklist_frame(&ctx, Table::Full, &mut library);
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&"Made by alice"),
            "the owner must show while the tracks load: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text.contains("songs")),
            "the count waits for the tracks: {texts:?}"
        );
    }

    #[test]
    fn the_band_says_made_by_you_for_our_own_playlist() {
        let ctx = context();
        let mut library = library_ours(timed_tracks(62, 223_548), "Evening");
        let output = tracklist_frame(&ctx, Table::Full, &mut library);
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&"Made by you \u{b7} 62 songs \u{b7} 3 hr 51 min"),
            "our own list says Made by you: {texts:?}"
        );
    }

    #[test]
    fn a_numeric_owner_draws_no_made_by_line() {
        let ctx = context();
        let mut library = library_with(timed_tracks(2, 1_000));
        let mut entry = listed("Evening", None, None);
        entry.owner = OwnerName::from_wire("31415926535").map(Owner::Named);
        assert_eq!(entry.owner, None, "a numeric id is not an owner");
        library.apply(&Update::Playlists(vec![entry]));
        let output = tracklist_frame(&ctx, Table::Full, &mut library);
        let texts = drawn_texts(&output);
        assert!(
            !texts.iter().any(|text| text.starts_with("Made by")),
            "a numeric owner names nobody: {texts:?}"
        );
    }

    #[test]
    fn a_card_for_our_own_playlist_draws_the_kind_word_alone() {
        let mut art = Art::default();
        let tokens = Tokens::DARK;
        let mut select = None;
        let mut open = None;
        let mut play = None;
        let library = Library::default();
        let mut draw = MediaDraw {
            art: &mut art,
            now: Instant::now(),
            tokens: &tokens,
            library: &library,
            select: &mut select,
            open: &mut open,
            play: &mut play,
        };

        let mut ours = playlist("spotify:playlist:a", "A");
        ours.owner = Some(Owner::You);
        assert_eq!(
            draw.media_of(&Item::Playlist(&ours), MediaLayout::Grid).sub,
            "Playlist"
        );

        let mut named = playlist("spotify:playlist:b", "B");
        named.owner = Some(Owner::Named(
            OwnerName::from_wire("Vela Moreno").expect("a name"),
        ));
        assert_eq!(
            draw.media_of(&Item::Playlist(&named), MediaLayout::Grid)
                .sub,
            "Playlist \u{b7} Vela Moreno"
        );
    }

    #[test]
    fn a_playlist_with_four_held_covers_draws_the_mosaic() {
        let ctx = context();
        let now = Instant::now();
        let mut art = Art::default();
        let mut library = mosaic_library(&["m0", "m1", "m2", "m3"]);
        let urls: Vec<CoverUrl> = (0..4)
            .map(|index| cover_url(&format!("m{index}")))
            .collect();
        hold(&ctx, &mut art, &urls, now);

        let output = band_frame_with(&ctx, &mut library, &mut art, now);
        let area = band_art(&output);
        assert_eq!(
            textured(&output, area),
            crate::layout::hero::quadrants(area).to_vec(),
            "four covers quarter the band's art box"
        );
    }

    #[test]
    fn the_band_draws_the_single_cover_until_every_mosaic_cover_is_held() {
        let ctx = context();
        let now = Instant::now();
        let mut art = Art::default();
        let mut library = mosaic_library_with_picture(&["m0", "m1", "m2", "m3"]);
        let urls: Vec<CoverUrl> = (0..4)
            .map(|index| cover_url(&format!("m{index}")))
            .collect();
        hold(&ctx, &mut art, &urls[..3], now);

        let output = band_frame_with(&ctx, &mut library, &mut art, now);
        let area = band_art(&output);
        assert_eq!(
            textured(&output, area),
            vec![area],
            "one picture fills the box while a quadrant is missing"
        );

        hold(&ctx, &mut art, &urls[3..], now);
        let output = band_frame_with(&ctx, &mut library, &mut art, now);
        let area = band_art(&output);
        assert_eq!(
            textured(&output, area).len(),
            4,
            "the mosaic swaps in whole once the fourth cover lands"
        );
    }

    #[test]
    fn a_mosaic_cover_not_yet_held_is_fetched_but_not_drawn() {
        let ctx = context();
        let now = Instant::now();
        let mut art = Art::default();
        let mut library = mosaic_library_with_picture(&["m0", "m1", "m2", "m3"]);
        let entry = library
            .selected_entry()
            .cloned()
            .expect("the page names its playlist");

        let (_, picture) = band_picture(
            &entry,
            &library.tracks,
            &mut library.mosaic_kept,
            &mut art,
            now,
        );
        assert_eq!(
            picture,
            hero::Picture::Cover,
            "nothing is drawn until every cover lands"
        );
        let fetched = fetch_urls(art.requests(now));
        for tail in ["m0", "m1", "m2", "m3"] {
            assert!(
                fetched.contains(&cover_url(tail)),
                "{tail} is fetched for the mosaic: {fetched:?}"
            );
        }

        let output = band_frame_with(&ctx, &mut library, &mut art, now);
        let area = band_art(&output);
        assert!(
            textured(&output, area).is_empty(),
            "a cover that has not landed draws nothing"
        );
    }

    #[test]
    fn two_distinct_covers_draw_two_quadrants_and_two_holes() {
        let ctx = context();
        let now = Instant::now();
        let mut art = Art::default();
        let mut library = mosaic_library(&["m0", "m1"]);
        hold(&ctx, &mut art, &[cover_url("m0"), cover_url("m1")], now);

        let output = band_frame_with(&ctx, &mut library, &mut art, now);
        let area = band_art(&output);
        let quadrants = crate::layout::hero::quadrants(area);
        assert_eq!(
            textured(&output, area),
            vec![quadrants[0], quadrants[1]],
            "the two covers take the first quadrants in reading order"
        );
        assert_eq!(
            holes(&output, area),
            vec![quadrants[2], quadrants[3]],
            "the missing corners are holes"
        );
    }

    #[test]
    fn a_mosaic_quadrant_that_failed_for_good_keeps_the_single_picture() {
        let ctx = context();
        let now = Instant::now();
        let mut art = Art::default();
        let mut library = mosaic_library_with_picture(&["m0", "m1", "m2", "m3"]);
        let urls: Vec<CoverUrl> = (0..4)
            .map(|index| cover_url(&format!("m{index}")))
            .collect();
        hold(&ctx, &mut art, &urls[..3], now);
        let at = fail_permanently(&mut art, &urls[3], now);
        let entry = library
            .selected_entry()
            .cloned()
            .expect("the page names its playlist");

        let (_, picture) = band_picture(
            &entry,
            &library.tracks,
            &mut library.mosaic_kept,
            &mut art,
            at,
        );
        assert_eq!(
            picture,
            hero::Picture::Cover,
            "a quadrant that will never arrive holds the mosaic back"
        );

        let output = band_frame_with(&ctx, &mut library, &mut art, at);
        let area = band_art(&output);
        assert_eq!(
            textured(&output, area),
            vec![area],
            "the band stays on its single picture"
        );
    }

    #[test]
    fn a_reload_keeps_the_mosaic_that_was_held() {
        let ctx = context();
        let now = Instant::now();
        let mut art = Art::default();
        let mut library = mosaic_library(&["m0", "m1", "m2", "m3"]);
        let urls: Vec<CoverUrl> = (0..4)
            .map(|index| cover_url(&format!("m{index}")))
            .collect();
        hold(&ctx, &mut art, &urls, now);

        let output = band_frame_with(&ctx, &mut library, &mut art, now);
        assert_eq!(textured(&output, band_art(&output)).len(), 4);

        // Re-selecting the same playlist puts its tracks back into `Loading`.
        library.select("spotify:playlist:a");
        let output = band_frame_with(&ctx, &mut library, &mut art, now);
        let area = band_art(&output);
        assert_eq!(
            textured(&output, area).len(),
            4,
            "a held mosaic survives the reload"
        );
        assert!(
            fetch_urls(art.requests(now)).is_empty(),
            "a held mosaic is not fetched again"
        );
    }

    #[test]
    fn a_ready_list_of_another_playlist_draws_no_mosaic() {
        let ctx = context();
        let now = Instant::now();
        let mut art = Art::default();
        let mut library = mosaic_library_with_picture(&["m0", "m1", "m2", "m3"]);
        let urls: Vec<CoverUrl> = (0..4)
            .map(|index| cover_url(&format!("m{index}")))
            .collect();
        hold(&ctx, &mut art, &urls, now);
        let tracks = match &library.tracks {
            Load::Ready(ready) => ready.tracks.clone(),
            Load::Idle | Load::Loading | Load::Failed(_) => panic!("the page is ready"),
        };
        library.tracks = Load::Ready(TracksFor {
            playlist: "spotify:playlist:other".to_owned(),
            tracks,
            access: None,
            notice: None,
        });

        let entry = library
            .selected_entry()
            .cloned()
            .expect("the page names its playlist");
        let (_, picture) = band_picture(
            &entry,
            &library.tracks,
            &mut library.mosaic_kept,
            &mut art,
            now,
        );
        assert_eq!(
            picture,
            hero::Picture::Cover,
            "another list's read never quarter this band"
        );

        let output = band_frame_with(&ctx, &mut library, &mut art, now);
        let area = band_art(&output);
        assert_eq!(
            textured(&output, area),
            vec![area],
            "the band keeps its own picture"
        );
    }

    #[test]
    fn a_drag_in_flight_keeps_the_mosaic() {
        let ctx = context();
        let now = Instant::now();
        let mut art = Art::default();
        let mut library = mosaic_library(&["m0", "m1", "m2", "m3"]);
        let urls: Vec<CoverUrl> = (0..4)
            .map(|index| cover_url(&format!("m{index}")))
            .collect();
        hold(&ctx, &mut art, &urls, now);

        let before = {
            let output = band_frame_with(&ctx, &mut library, &mut art, now);
            textured(&output, band_art(&output))
        };
        egui::DragAndDrop::set_payload(&ctx, ());
        let after = {
            let output = band_frame_with(&ctx, &mut library, &mut art, now);
            textured(&output, band_art(&output))
        };
        egui::DragAndDrop::clear_payload(&ctx);

        assert_eq!(after, before, "a drag in flight does not disturb the band");
        assert_eq!(after.len(), 4);
    }

    #[test]
    fn a_reorder_that_changes_the_first_four_covers_changes_the_mosaic_once_ready() {
        let ctx = context();
        let now = Instant::now();
        let mut art = Art::default();
        let mut library = mosaic_library(&["m0", "m1", "m2", "m3"]);
        let all: Vec<CoverUrl> = (0..8)
            .map(|index| cover_url(&format!("m{index}")))
            .collect();
        hold(&ctx, &mut art, &all, now);
        let entry = library
            .selected_entry()
            .cloned()
            .expect("the page names its playlist");

        let first = match band_picture(
            &entry,
            &library.tracks,
            &mut library.mosaic_kept,
            &mut art,
            now,
        )
        .1
        {
            hero::Picture::Mosaic(quadrants) => quadrants,
            hero::Picture::Cover => panic!("four held covers quarter the band"),
        };
        assert_eq!(
            first,
            [
                drawn_cover(&mut art, "m0", now),
                drawn_cover(&mut art, "m1", now),
                drawn_cover(&mut art, "m2", now),
                drawn_cover(&mut art, "m3", now),
            ]
        );

        // The reload between orders keeps the quadrants that were held.
        library.select("spotify:playlist:a");
        let reloading = match band_picture(
            &entry,
            &library.tracks,
            &mut library.mosaic_kept,
            &mut art,
            now,
        )
        .1
        {
            hero::Picture::Mosaic(quadrants) => quadrants,
            hero::Picture::Cover => panic!("the kept mosaic stays during the reload"),
        };
        assert_eq!(
            reloading, first,
            "the held quadrants stay while the list reloads"
        );

        // The reordered list lands ready, and the quadrants follow it.
        let mut tracks: Vec<TrackEntry> = (0..4)
            .map(|index| track(&format!("t{index}"), &format!("t{index}")))
            .collect();
        for (index, track) in tracks.iter_mut().enumerate() {
            track.cover = Some(cover_url(&format!("m{}", index + crate::mosaic::QUADRANTS)));
        }
        library.apply(&Update::Tracks {
            playlist: "spotify:playlist:a".to_owned(),
            tracks,
            cover: None,
            access: None,
        });
        let after = match band_picture(
            &entry,
            &library.tracks,
            &mut library.mosaic_kept,
            &mut art,
            now,
        )
        .1
        {
            hero::Picture::Mosaic(quadrants) => quadrants,
            hero::Picture::Cover => panic!("the reordered covers quarter the band"),
        };
        assert_eq!(
            after,
            [
                drawn_cover(&mut art, "m4", now),
                drawn_cover(&mut art, "m5", now),
                drawn_cover(&mut art, "m6", now),
                drawn_cover(&mut art, "m7", now),
            ],
            "the quadrants follow the reordered list"
        );
    }

    #[test]
    fn a_chosen_cover_keeps_its_single_picture() {
        let ctx = context();
        let now = Instant::now();
        let mut art = Art::default();
        let mut library = mosaic_library(&["m0", "m1", "m2", "m3"]);
        library.apply(&Update::Playlists(vec![PlaylistEntry {
            composed: Vec::new(),
            cover: Some(PlaylistCover::chosen(cover_url("own"))),
            ..listed("Evening", Some(DESCRIPTION), None)
        }]));
        let urls: Vec<CoverUrl> = (0..4)
            .map(|index| cover_url(&format!("m{index}")))
            .collect();
        hold(&ctx, &mut art, &urls, now);
        hold(&ctx, &mut art, &[cover_url("own")], now);

        let output = band_frame_with(&ctx, &mut library, &mut art, now);
        let area = band_art(&output);
        assert_eq!(
            textured(&output, area),
            vec![area],
            "a picture its users chose is never replaced by a mosaic"
        );
    }

    /// The words and picture the root's grid builds for one playlist card.
    fn card_media(
        library: &Library,
        art: &mut Art,
        entry: &PlaylistEntry,
        now: Instant,
    ) -> MediaWords {
        let tokens = Tokens::DARK;
        let mut select = None;
        let mut open = None;
        let mut play = None;
        let mut draw = MediaDraw {
            art,
            now,
            tokens: &tokens,
            library,
            select: &mut select,
            open: &mut open,
            play: &mut play,
        };
        draw.media_of(&Item::Playlist(entry), MediaLayout::Grid)
    }

    /// The single cover the card draws, and its art box.
    fn card_picture(
        ctx: &Context,
        art: &mut Art,
        entry: &PlaylistEntry,
        now: Instant,
    ) -> (Vec<Rect>, hero::Picture) {
        let library = Library::default();
        let words = card_media(&library, art, entry, now);
        let tokens = Tokens::DARK;
        let mut area = Rect::ZERO;
        let output = run(ctx, input(Vec::new()), |ui| {
            let rect = Rect::from_min_size(
                Pos2::ZERO,
                Vec2::new(176.0, 176.0 + media::extra_height(ui)),
            );
            let _ = media::tile(ui, rect, &words.as_media(), &tokens);
            area = media::art_rect(rect);
        });
        (textured(&output, area), words.picture)
    }

    #[test]
    fn a_card_waits_for_every_quadrant_before_swapping() {
        let ctx = context();
        let now = Instant::now();
        let mut art = Art::default();
        let entry = PlaylistEntry {
            cover: Some(PlaylistCover::derived(cover_url("m0"))),
            composed: (0..4)
                .map(|index| cover_url(&format!("m{index}")))
                .collect(),
            ..listed("Evening", None, None)
        };
        // Three of the four are held; the fourth is still arriving.
        let held: Vec<CoverUrl> = (0..3)
            .map(|index| cover_url(&format!("m{index}")))
            .collect();
        hold(&ctx, &mut art, &held, now);

        let (drawn, picture) = card_picture(&ctx, &mut art, &entry, now);
        assert_eq!(
            picture,
            hero::Picture::Cover,
            "one quadrant still loading keeps the single cover"
        );
        assert_eq!(drawn.len(), 1, "no partial mosaic is drawn");

        hold(&ctx, &mut art, &[cover_url("m3")], now);
        let (drawn, picture) = card_picture(&ctx, &mut art, &entry, now);
        assert!(
            matches!(picture, hero::Picture::Mosaic(_)),
            "the card swaps whole once every quadrant is held"
        );
        assert_eq!(drawn.len(), 4);
    }

    #[test]
    fn a_list_row_asks_only_for_the_cover_it_draws() {
        let now = Instant::now();
        let mut art = Art::default();
        let library = Library::default();
        let entry = PlaylistEntry {
            cover: Some(PlaylistCover::derived(cover_url("m0"))),
            composed: (0..4)
                .map(|index| cover_url(&format!("m{index}")))
                .collect(),
            ..listed("Evening", None, None)
        };
        let tokens = Tokens::DARK;
        let mut select = None;
        let mut open = None;
        let mut play = None;
        art.begin_frame();
        let mut draw = MediaDraw {
            art: &mut art,
            now,
            tokens: &tokens,
            library: &library,
            select: &mut select,
            open: &mut open,
            play: &mut play,
        };
        let _ = draw.media_of(&Item::Playlist(&entry), MediaLayout::List);

        assert_eq!(
            fetch_urls(draw.art.requests(now)),
            vec![cover_url("m0")],
            "a row asks only for the cover it draws"
        );
    }

    #[test]
    fn a_long_single_album_head_keeps_the_single_cover_on_the_card() {
        let ctx = context();
        let now = Instant::now();
        let mut art = Art::default();
        // A head of twelve rows, all from one album, is one cover: no mosaic.
        let head: Vec<CoverUrl> = (0..12).map(|_| cover_url("one")).collect();
        let composed = takofy_model::composed_from(None, head.iter().map(Some));
        assert!(
            composed.is_empty(),
            "one album repeated has nothing to tile"
        );
        let entry = PlaylistEntry {
            cover: Some(PlaylistCover::derived(cover_url("one"))),
            composed,
            ..listed("Evening", None, None)
        };
        hold(&ctx, &mut art, &[cover_url("one")], now);

        let (drawn, picture) = card_picture(&ctx, &mut art, &entry, now);
        assert_eq!(picture, hero::Picture::Cover);
        assert_eq!(drawn.len(), 1, "the accepted gap: the card keeps one cover");
    }

    #[test]
    fn one_distinct_cover_keeps_the_single_look() {
        let ctx = context();
        let now = Instant::now();
        let mut art = Art::default();
        let tracks = (0..4)
            .map(|index| TrackEntry {
                cover: Some(cover_url("one")),
                ..track(&format!("t{index}"), &format!("t{index}"))
            })
            .collect();
        let mut library = library_detailed(tracks, "Evening", None, None);
        library.apply(&Update::Playlists(vec![PlaylistEntry {
            composed: Vec::new(),
            cover: Some(PlaylistCover::derived(cover_url("one"))),
            ..listed("Evening", None, None)
        }]));
        hold(&ctx, &mut art, &[cover_url("one")], now);

        let output = band_frame_with(&ctx, &mut library, &mut art, now);
        let area = band_art(&output);
        assert_eq!(
            textured(&output, area),
            vec![area],
            "one album is one picture, not four"
        );
    }

    #[test]
    fn album_and_show_bands_still_draw_one_cover() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let now = Instant::now();
        let mut art = Art::default();
        hold(&ctx, &mut art, &[cover_url("one")], now);
        let cover = art.look(Some(&cover_url("one")), now);
        assert!(matches!(cover, Cover::Ready(_)));

        for eyebrow in ["Album", "Show"] {
            let mut output = run(&ctx, input(Vec::new()), |ui| {
                let band = hero::Band {
                    eyebrow,
                    title: "Signal Decay",
                    meta: Vec::new(),
                    credit: None,
                    description: "",
                    cover,
                };
                let _ = hero::show_with(ui, &band, &tokens, hero::Title::Plain);
            });
            let area = band_art(&output);
            assert_eq!(
                textured(&output, area),
                vec![area],
                "the {eyebrow} band still draws its one cover"
            );
            output.textures_delta.clear();
        }
    }

    #[test]
    fn the_band_text_does_not_move_with_the_covers_held() {
        let ctx = context();
        let now = Instant::now();
        let mut art = Art::default();
        let mut library = mosaic_full();
        let urls: Vec<CoverUrl> = (0..4)
            .map(|index| cover_url(&format!("m{index}")))
            .collect();

        let mut held = 0;
        let mut positions = Vec::new();
        let mut pictures = Vec::new();
        for want in [0, 2, 4] {
            if want > held {
                hold(&ctx, &mut art, &urls[held..want], now);
                held = want;
            }
            let output = band_frame_with(&ctx, &mut library, &mut art, now);
            let area = band_art(&output);
            pictures.push(textured(&output, area).len());
            positions.push((
                galley_at(&output, "Evening"),
                galley_at(&output, META),
                galley_at(&output, DESCRIPTION),
            ));
        }

        assert_eq!(positions[0], positions[1], "two covers moved the words");
        assert_eq!(positions[1], positions[2], "four covers moved the words");
        assert_eq!(
            pictures,
            vec![0, 0, 4],
            "the picture itself changed, so the texts were compared across frames"
        );
    }

    #[test]
    fn a_long_description_is_clamped_to_two_rows_and_elided() {
        let ctx = context();
        let mut library = library_detailed(
            timed_tracks(2, 1_000),
            "Evening",
            Some(LONG_DESCRIPTION),
            None,
        );
        let output = tracklist_frame(&ctx, Table::Full, &mut library);
        let galley = galley_of(&output, LONG_DESCRIPTION);
        assert_eq!(galley.rows.len(), 2, "two rows, no more");
        assert!(galley.elided, "a description that does not fit elides");
        let last = galley.rows[1].row.text();
        assert!(
            last.ends_with('\u{2026}'),
            "the elided last row must end in an ellipsis: {last:?}"
        );
    }

    #[test]
    fn a_narrow_band_elides_the_byline() {
        let ctx = context();
        let mut library = library_detailed(
            timed_tracks(62, 223_548),
            "Evening",
            None,
            Some("alice-the-very-long-username"),
        );
        let output = tracklist_frame_in(&ctx, Vec2::new(560.0, 800.0), &mut library);
        let galley = galley_of(
            &output,
            "Made by alice-the-very-long-username \u{b7} 62 songs \u{b7} 3 hr 51 min",
        );
        assert!(
            galley.elided,
            "a byline wider than the band elides: {:?}",
            galley.text()
        );
    }

    #[test]
    fn the_band_does_not_clip_the_eyebrow_at_its_tallest_content() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut band_top = None;
        let output = run(
            &ctx,
            sized_input(Vec2::new(1600.0, 900.0), Vec::new()),
            |ui| {
                band_top = Some(ui.cursor().min.y);
                let band = hero::Band {
                    eyebrow: "Playlist",
                    title: "Late Night Drive",
                    meta: vec![
                        "Made by alice".to_owned(),
                        "62 songs".to_owned(),
                        "3 hr 51 min".to_owned(),
                    ],
                    credit: None,
                    description: LONG_DESCRIPTION,
                    cover: Cover::Missing,
                };
                let _ = hero::show_with(ui, &band, &tokens, hero::Title::Plain);
            },
        );
        let band_top = band_top.expect("the band starts at the cursor");
        let drawn = drawn_galleys(&output);
        let eyebrow = galley_top(&drawn, "PLAYLIST");
        assert!(
            eyebrow >= band_top,
            "the eyebrow's top {eyebrow} is above the band's top {band_top}: the \
             tallest content does not fit the band's height"
        );
    }
    /// A playlist page whose sidebar entry says whether the user may write it.
    fn band_library(name: &str, editable: bool, titles: &[&str]) -> Library {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![PlaylistEntry {
            composed: Vec::new(),
            uri: "spotify:playlist:a".to_owned(),
            name: name.to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: Some(titles.len()),
            access: access(editable),
        }]));
        library.apply(&Update::RootlistAccess(PlaylistAccess {
            revision: Revision::from_wire(b"root-1").expect("a revision"),
            editable: true,
        }));
        library.select("spotify:playlist:a");
        library.apply(&Update::Tracks {
            playlist: "spotify:playlist:a".to_owned(),
            tracks: titles.iter().map(|title| track(title, title)).collect(),
            cover: None,
            access: access(editable),
        });
        library
    }

    /// One frame of the tracklist with `events`, reporting its command.
    fn band_frame(
        ctx: &Context,
        library: &mut Library,
        events: Vec<Event>,
    ) -> (Option<Command>, egui::FullOutput) {
        let tokens = Tokens::DARK;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut command = None;
        let output = run(ctx, input(events), |ui| {
            command = tracks_view(
                ui,
                Table::Full,
                library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });
        (command, output)
    }

    /// Clicks the band's title, returning the command the view reported.
    fn click_band_title(ctx: &Context, library: &mut Library, name: &str) -> Option<Command> {
        let output = tracklist_frame(ctx, Table::Full, library);
        let at = galley_at(&output, name) + Vec2::new(20.0, 20.0);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        click(ctx, at, library, &mut transport, &mut queue)
    }

    /// The "…" in the band's action row, from the icons it paints beside play.
    fn band_more_point(ctx: &Context, library: &mut Library, name: &str) -> Pos2 {
        let output = tracklist_frame(ctx, Table::Full, library);
        let play = play_tile_rect_for(ctx, name);
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Path(_) => Some(clipped.shape.visual_bounding_rect()),
                _ => None,
            })
            .filter(|rect| rect.center().y < play.center().y + 20.0)
            .filter(|rect| rect.center().x > play.right())
            .max_by(|left, right| left.center().x.total_cmp(&right.center().x))
            .map(|rect| rect.center())
            .expect("the band paints its `⋯`")
    }

    #[test]
    fn a_title_click_opens_the_band_field_only_on_a_list_of_ours() {
        let ctx = context();
        let mut editable = band_library("Night Drive", true, &["t0"]);
        assert_eq!(click_band_title(&ctx, &mut editable, "Night Drive"), None);
        assert!(
            editable
                .renaming
                .is_open_on("spotify:playlist:a", Surface::Band)
        );
        assert_eq!(editable.renaming.text(), "Night Drive");

        let mut read_only = band_library("Night Drive", false, &["t0"]);
        assert_eq!(click_band_title(&ctx, &mut read_only, "Night Drive"), None);
        assert!(!read_only.renaming.is_open());
    }

    #[test]
    fn a_title_click_waits_while_a_block_is_carried_or_a_write_is_pending() {
        let ctx = context();
        let mut carried = band_library("Night Drive", true, &["t0"]);
        egui::DragAndDrop::set_payload(&ctx, ());
        assert_eq!(click_band_title(&ctx, &mut carried, "Night Drive"), None);
        assert!(!carried.renaming.is_open(), "a carry opened the field");
        egui::DragAndDrop::clear_payload(&ctx);

        let mut writing = band_library("Night Drive", true, &["t0", "t1"]);
        let first = match &writing.tracks {
            Load::Ready(tracks) => tracks.tracks[0].clone(),
            Load::Idle | Load::Loading | Load::Failed(_) => panic!("the page is ready"),
        };
        assert!(
            writing
                .remove_tracks("spotify:playlist:a", &[0], &[first])
                .is_some()
        );
        assert_eq!(click_band_title(&ctx, &mut writing, "Night Drive"), None);
        assert!(!writing.renaming.is_open(), "a write opened the field");
    }

    #[test]
    fn the_band_menu_offers_rename_first_and_never_on_a_read_only_list() {
        let ctx = context();
        let mut editable = band_library("Night Drive", true, &["t0"]);
        let at = band_more_point(&ctx, &mut editable, "Night Drive");
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        assert_eq!(
            click(&ctx, at, &mut editable, &mut transport, &mut queue),
            None
        );
        let request =
            crate::view::context_menu::take_request(&ctx).expect("the band posted a menu");
        assert_eq!(
            request.listing,
            crate::menu::Listing::Band(LibraryItem {
                kind: SidebarKind::Playlists,
                uri: "spotify:playlist:a".to_owned(),
                name: "Night Drive".to_owned(),
            })
        );
        assert_eq!(
            request
                .items
                .iter()
                .map(|item| item.verb)
                .collect::<Vec<_>>(),
            [
                crate::menu::Verb::Rename,
                crate::menu::Verb::CopyLink,
                crate::menu::Verb::RemoveFromLibrary,
            ]
        );

        let mut read_only = band_library("Night Drive", false, &["t0"]);
        let at = band_more_point(&ctx, &mut read_only, "Night Drive");
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        assert_eq!(
            click(&ctx, at, &mut read_only, &mut transport, &mut queue),
            None
        );
        let request =
            crate::view::context_menu::take_request(&ctx).expect("the band posted a menu");
        assert!(
            request
                .items
                .iter()
                .all(|item| item.verb != crate::menu::Verb::Rename),
            "a list we cannot write offered Rename"
        );
    }

    #[test]
    fn the_band_starts_flush_at_the_top_of_the_page() {
        let ctx = context();
        let mut library = band_library("Night Drive", true, &["t0"]);
        let output = tracklist_frame_in(&ctx, TALL, &mut library);
        let band = band_rect(&output);
        assert_eq!(
            band.top(),
            0.0,
            "the page starts {}pt below the topbar",
            band.top()
        );
        assert_eq!(band.width(), TALL.x, "the band is no longer full-bleed");
    }

    #[test]
    fn the_band_field_keeps_the_floor_still() {
        let ctx = context();
        let mut closed = band_library("Night Drive", true, &["t0"]);
        let closed_output = tracklist_frame_in(&ctx, TALL, &mut closed);
        let closed_top = drawn_galleys(&closed_output)
            .into_iter()
            .find(|(text, _, _)| text == "t0")
            .map(|(_, top, _)| top)
            .expect("the row is painted");

        let mut open = band_library("Night Drive", true, &["t0"]);
        assert!(open.begin_rename("spotify:playlist:a", Surface::Band));
        let open_output = tracklist_frame_in(&ctx, TALL, &mut open);
        let open_top = drawn_galleys(&open_output)
            .into_iter()
            .find(|(text, _, _)| text == "t0")
            .map(|(_, top, _)| top)
            .expect("the row is painted");

        assert_eq!(closed_top, open_top, "the field moved the tracks");
        // The floor holding still is not the whole claim: the band itself keeps
        // the box it had, and the title keeps the baseline it stood on, so the
        // field is drawn in the title's own place rather than beside it.
        let closed_band = band_rect(&closed_output);
        let open_band = band_rect(&open_output);
        assert_eq!(
            closed_band, open_band,
            "the field resized or moved the band"
        );

        let closed_title = text_rect(&closed_output, "Night Drive").expect("the title is painted");
        let open_title = text_rect(&open_output, "Night Drive").expect("the field paints the name");
        // The field's own text layout rounds its baseline to a fraction of a
        // point, so the claim is that the title keeps its row, not its subpixel.
        assert!(
            (closed_title.top() - open_title.top()).abs() < 0.1,
            "the field moved the title: {} then {}",
            closed_title.top(),
            open_title.top()
        );
        assert_eq!(
            closed_title.height(),
            open_title.height(),
            "the field resized the title"
        );
    }

    /// The band's own rect, read from the wash it paints behind itself.
    fn band_rect(output: &egui::FullOutput) -> Rect {
        let tokens = Tokens::DARK;
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape) if shape.fill == tokens.palette.hero_wash => {
                    Some(shape.rect)
                }
                _ => None,
            })
            .expect("the band paints its wash")
    }

    /// The box a text shape with `label` was painted in.
    fn text_rect(output: &egui::FullOutput, label: &str) -> Option<Rect> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                    Some(Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
    }

    /// The title's widget id, taken from the press that opens its field: a press
    /// on the title takes the keyboard before the field draws.
    fn band_title_focus_id(ctx: &Context, library: &mut Library, name: &str) -> egui::Id {
        assert_eq!(click_band_title(ctx, library, name), None);
        ctx.memory(|memory| memory.focused())
            .expect("the title did not take the keyboard")
    }

    #[test]
    fn a_closed_band_field_hands_the_keyboard_back_to_the_title() {
        let ctx = context();
        let mut escaping = band_library("Night Drive", true, &["t0"]);
        let title = band_title_focus_id(&ctx, &mut escaping, "Night Drive");
        let _ = band_frame(&ctx, &mut escaping, Vec::new());
        let _ = band_frame(&ctx, &mut escaping, Vec::new());
        assert!(ctx.text_edit_focused(), "the field has no keyboard");

        let _ = band_frame(&ctx, &mut escaping, vec![key(egui::Key::Escape)]);

        assert!(!escaping.renaming.is_open());
        assert!(
            ctx.memory(|memory| memory.has_focus(title)),
            "Escape left the keyboard nowhere"
        );

        let mut confirming = band_library("Night Drive", true, &["t0"]);
        let title = band_title_focus_id(&ctx, &mut confirming, "Night Drive");
        let _ = band_frame(&ctx, &mut confirming, Vec::new());
        let _ = band_frame(&ctx, &mut confirming, Vec::new());
        confirming.renaming.edit().clear();
        confirming.renaming.edit().push_str("New");
        let _ = band_frame(&ctx, &mut confirming, vec![key(egui::Key::Enter)]);
        assert!(crate::view::name_field::take_submit(&ctx));
        assert!(confirming.rename_playlist().is_some());
        confirming.apply(&Update::PlaylistRenamed {
            playlist: "spotify:playlist:a".to_owned(),
            name: "New".to_owned(),
            revision: Revision::from_wire(b"rev-2").expect("a revision"),
        });
        let _ = band_frame(&ctx, &mut confirming, Vec::new());

        assert!(
            ctx.memory(|memory| memory.has_focus(title)),
            "a confirmed rename left the keyboard nowhere"
        );
    }

    #[test]
    fn escape_while_the_band_field_writes_closes_nothing() {
        let ctx = context();
        let mut library = band_library("Night Drive", true, &["t0"]);
        click_band_title(&ctx, &mut library, "Night Drive");
        let _ = band_frame(&ctx, &mut library, Vec::new());
        let _ = band_frame(&ctx, &mut library, Vec::new());
        library.renaming.edit().clear();
        library.renaming.edit().push_str("New");
        let _ = band_frame(&ctx, &mut library, vec![key(egui::Key::Enter)]);
        assert!(crate::view::name_field::take_submit(&ctx));
        assert!(library.rename_playlist().is_some());

        let _ = band_frame(&ctx, &mut library, vec![key(egui::Key::Escape)]);

        assert!(
            library.renaming.is_open(),
            "Escape closed a field that was writing"
        );
        assert!(library.renaming.writing());
        assert!(!library.renaming.wants_return());
    }

    #[test]
    fn a_row_fields_return_belongs_to_the_sidebar_row() {
        let ctx = context();
        let mut library = band_library("Night Drive", true, &["t0"]);
        assert!(library.begin_rename("spotify:playlist:a", Surface::Row));
        library.renaming.cancel();
        assert!(library.renaming.wants_return());

        let _ = band_frame(&ctx, &mut library, Vec::new());

        assert!(
            library.renaming.wants_return(),
            "the band took a row field's hand-back"
        );
        assert_eq!(
            library.renaming.uri(),
            "spotify:playlist:a",
            "the band cleared the row's target"
        );
    }

    #[test]
    fn a_band_fields_return_waits_for_the_page_that_opened_it() {
        let ctx = context();
        let mut library = band_library("Night Drive", true, &["t0"]);
        if let Load::Ready(playlists) = &mut library.playlists {
            playlists.push(PlaylistEntry {
                composed: Vec::new(),
                uri: "spotify:playlist:b".to_owned(),
                name: "Other".to_owned(),
                named: true,
                cover: None,
                description: None,
                owner: None,
                tracks: Some(1),
                access: access(true),
            });
        }
        assert!(library.begin_rename("spotify:playlist:a", Surface::Band));
        *library.renaming.edit() = "New".to_owned();
        assert!(library.rename_playlist().is_some());

        // The reply lands after the user moved to another playlist, where this
        // field's title is not drawn at all.
        library.select("spotify:playlist:b");
        library.apply(&Update::Tracks {
            playlist: "spotify:playlist:b".to_owned(),
            tracks: vec![track("t9", "t9")],
            cover: None,
            access: access(true),
        });
        library.apply(&Update::PlaylistRenamed {
            playlist: "spotify:playlist:a".to_owned(),
            name: "New".to_owned(),
            revision: Revision::from_wire(b"rev-2").expect("a revision"),
        });
        assert!(library.renaming.wants_return());

        let _ = band_frame(&ctx, &mut library, Vec::new());

        assert!(
            library.renaming.wants_return(),
            "another page spent the field's hand-back"
        );
        assert_eq!(
            library.renaming.uri(),
            "spotify:playlist:a",
            "another page took the field's target"
        );
        assert!(!ctx.text_edit_focused());
    }

    #[test]
    fn the_band_field_takes_enter_and_escape() {
        let ctx = context();
        let mut library = band_library("Night Drive", true, &["t0"]);
        click_band_title(&ctx, &mut library, "Night Drive");
        let _ = band_frame(&ctx, &mut library, Vec::new());
        let _ = band_frame(&ctx, &mut library, Vec::new());
        assert!(ctx.text_edit_focused(), "the band field has no keyboard");

        library.renaming.edit().clear();
        let _ = band_frame(&ctx, &mut library, vec![Event::Text("New".to_owned())]);
        let (command, _) = band_frame(&ctx, &mut library, vec![key(egui::Key::Enter)]);
        assert_eq!(command, None, "the view never sends a command itself");
        assert!(
            crate::view::name_field::take_submit(&ctx),
            "Enter posted no submit for the shell"
        );
        assert_eq!(
            library.rename_playlist(),
            Some(Command::RenamePlaylist {
                playlist: "spotify:playlist:a".to_owned(),
                name: "New".to_owned(),
            })
        );
        assert!(library.renaming.writing());

        let mut escaping = band_library("Night Drive", true, &["t0"]);
        click_band_title(&ctx, &mut escaping, "Night Drive");
        let _ = band_frame(&ctx, &mut escaping, Vec::new());
        let _ = band_frame(&ctx, &mut escaping, Vec::new());
        let (command, _) = band_frame(&ctx, &mut escaping, vec![key(egui::Key::Escape)]);
        assert_eq!(command, None);
        assert!(!escaping.renaming.is_open());
    }

    #[test]
    fn a_held_enter_after_a_band_rename_does_not_reopen_the_field() {
        let ctx = context();
        let mut library = band_library("Night Drive", true, &["t0"]);
        click_band_title(&ctx, &mut library, "Night Drive");
        let _ = band_frame(&ctx, &mut library, Vec::new());
        let _ = band_frame(&ctx, &mut library, Vec::new());
        library.renaming.edit().clear();
        let _ = band_frame(&ctx, &mut library, vec![Event::Text("New".to_owned())]);
        let _ = band_frame(&ctx, &mut library, vec![key(egui::Key::Enter)]);
        assert!(crate::view::name_field::take_submit(&ctx));
        assert!(library.rename_playlist().is_some());
        library.apply(&Update::PlaylistRenamed {
            playlist: "spotify:playlist:a".to_owned(),
            name: "New".to_owned(),
            revision: Revision::from_wire(b"rev-2").expect("a revision"),
        });
        let _ = band_frame(&ctx, &mut library, Vec::new());

        let repeat = Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: true,
            modifiers: Modifiers::NONE,
        };
        let _ = band_frame(&ctx, &mut library, vec![repeat]);

        assert!(
            !library.renaming.is_open(),
            "a held Enter reopened the field it just closed"
        );
    }

    #[test]
    fn a_band_rename_failure_on_an_empty_playlist_says_why() {
        let ctx = context();
        let mut library = band_library("Night Drive", true, &[]);
        assert!(library.begin_rename("spotify:playlist:a", Surface::Band));
        library.renaming.edit().clear();
        library.renaming.edit().push_str("New");
        assert!(library.rename_playlist().is_some());
        library.apply(&Update::PlaylistRenameFailed {
            playlist: "spotify:playlist:a".to_owned(),
        });

        let painted = painted_lines(&ctx, Table::Full, &mut library);

        assert!(
            painted.contains(&crate::signin::failure_message(Failure::Rename).to_owned()),
            "an empty playlist hid the failure: {painted:?}"
        );
        assert!(painted.contains(&"No tracks in this playlist.".to_owned()));
    }

    // -----------------------------------------------------------------------
    // The filter field: the rows it leaves, the count it paints, and Escape.
    // -----------------------------------------------------------------------

    const PLAYLIST: &str = "spotify:playlist:a";
    /// A window tall enough to hold the band-less page's action row, head and all
    /// sixty-two rows, so a restore can be read row by row. Wider than the rail's
    /// 900, because the count is not drawn in a rail.
    const ALL: Vec2 = Vec2::new(1000.0, 3200.0);
    /// The twelve true indices a `midnight` query leaves.
    const MIDNIGHT: [usize; 12] = [1, 4, 9, 12, 20, 23, 31, 38, 39, 51, 57, 60];

    /// Sixty-two rows whose artist says `Midnight Artist` at twelve true
    /// positions, so the title can stay `t<index>` for `painted_rows`.
    fn midnight_tracks() -> Vec<TrackEntry> {
        (0..62)
            .map(|index| TrackEntry {
                artists: if MIDNIGHT.contains(&index) {
                    "Midnight Artist".to_owned()
                } else {
                    "Vela Moreno".to_owned()
                },
                ..track(&format!("t{index}"), &format!("t{index}"))
            })
            .collect()
    }

    /// One frame of the tracklist in a window of `size`, carrying `events`.
    fn frame_in(
        ctx: &Context,
        size: Vec2,
        library: &mut Library,
        events: Vec<Event>,
    ) -> egui::FullOutput {
        table_frame_in(ctx, size, Table::Full, library, events)
    }

    /// One frame of the tracklist handed `table`, so a test can put the page in
    /// the form its own table names at a width the page is not classified for.
    fn table_frame_in(
        ctx: &Context,
        size: Vec2,
        table: Table,
        library: &mut Library,
        events: Vec<Event>,
    ) -> egui::FullOutput {
        let tokens = Tokens::DARK;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        run(ctx, sized_input(size, events), |ui| {
            let _ = tracks_view(
                ui,
                table,
                library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        })
    }

    /// One frame of the tracklist with a wheel jump applied in the frame it
    /// arrives, as the window's own draw pass does: `view::show` calls
    /// `scroll::instant` before any scroll area.
    fn instant_frame_in(
        ctx: &Context,
        size: Vec2,
        library: &mut Library,
        events: Vec<Event>,
    ) -> egui::FullOutput {
        let tokens = Tokens::DARK;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        run(ctx, sized_input(size, events), |ui| {
            crate::scroll::instant(ui.ctx());
            let _ = tracks_view(
                ui,
                Table::Full,
                library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        })
    }

    /// The field's bottom hairline: the box it spans and the ink it carries.
    fn filter_hairline(output: &egui::FullOutput) -> (Rect, egui::Color32) {
        let tokens = Tokens::DARK;
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape)
                    if (shape.rect.height() - crate::theme::stroke::HAIRLINE).abs() < 0.01
                        && shape.rect.width() <= 240.5
                        && shape.rect.width() > 80.0
                        && (shape.fill == tokens.palette.border
                            || shape.fill == tokens.palette.text) =>
                {
                    Some((shape.rect, shape.fill))
                }
                _ => None,
            })
            .min_by(|left, right| left.0.top().total_cmp(&right.0.top()))
            .expect("the filter field paints its bottom hairline")
    }

    /// The filter field's box, read from the hairline it paints under itself:
    /// the line is the field's bottom, and the field is one large button tall.
    fn field_at(output: &egui::FullOutput) -> Rect {
        let line = filter_hairline(output).0;
        Rect::from_min_max(
            Pos2::new(line.left(), line.bottom() - extent::BUTTON_LG),
            Pos2::new(line.right(), line.bottom()),
        )
    }

    /// The filter field's drawn box at any width, read from the line under its
    /// query.
    ///
    /// A condensed page's field is wider than `filter_hairline`'s upper bound, so
    /// the tests that check it pick the hairline below the field's own text
    /// rather than the first one on the page.
    fn field_box_under(output: &egui::FullOutput, label: &str) -> Rect {
        let tokens = Tokens::DARK;
        let text = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                    Some(Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
            .expect("the field paints its query");
        let line = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape)
                    if (shape.rect.height() - crate::theme::stroke::HAIRLINE).abs() < 0.01
                        && shape.rect.top() >= text.bottom() - 0.01
                        && (shape.fill == tokens.palette.border
                            || shape.fill == tokens.palette.text) =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
            .min_by(|left, right| left.top().total_cmp(&right.top()))
            .expect("the field paints its bottom hairline under its query");
        Rect::from_min_max(
            Pos2::new(line.left(), line.bottom() - extent::BUTTON_LG),
            Pos2::new(line.right(), line.bottom()),
        )
    }

    /// The clear control's centre, solved the way the view places it: the field's
    /// right padding, then the glyph's side.
    fn clear_point(field: Rect) -> Pos2 {
        Pos2::new(
            field.right() - space::S1 - (space::S3 + crate::theme::icon::Size::SM.side) / 2.0,
            field.center().y,
        )
    }

    /// The clear control's box, solved the way the view places it.
    fn clear_box(field: Rect) -> Rect {
        Rect::from_min_max(
            Pos2::new(
                field.right() - space::S3 - crate::theme::icon::Size::SM.side - space::S1,
                field.top(),
            ),
            Pos2::new(field.right() - space::S1, field.bottom()),
        )
    }

    /// The boxes of the geometry a frame painted: a glyph is paths and discs,
    /// so this finds an icon without knowing which one it is.
    fn painted_boxes(output: &egui::FullOutput) -> Vec<Rect> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Path(path) if !path.points.is_empty() => {
                    let mut boxed = Rect::NOTHING;
                    for point in &path.points {
                        boxed.extend_with(*point);
                    }
                    Some(boxed)
                }
                egui::epaint::Shape::Circle(circle) => Some(Rect::from_center_size(
                    circle.center,
                    Vec2::splat(circle.radius),
                )),
                _ => None,
            })
            .collect()
    }

    /// Every text shape a frame painted, as a box.
    fn painted_text_boxes(output: &egui::FullOutput) -> Vec<Rect> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) => {
                    Some(Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
            .collect()
    }

    /// One click at `at` with `modifiers` held, one pass per pointer event so
    /// egui's hit test has the previous frame to read.
    fn click_holding(
        ctx: &Context,
        at: Pos2,
        modifiers: Modifiers,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
    ) {
        let tokens = Tokens::DARK;
        run(ctx, input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                library,
                transport,
                queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });
        for event in crate::view::test_support::click_events(at) {
            run(
                ctx,
                input(vec![Event::ModifiersChanged(modifiers), event]),
                |ui| {
                    let _ = tracks_view(
                        ui,
                        Table::Full,
                        library,
                        transport,
                        queue,
                        &mut Art::default(),
                        Instant::now(),
                        NOW_MS,
                        &tokens,
                    );
                },
            );
        }
    }

    /// The action row's shuffle tile, solved as `play_tile_rect_for` solves play.
    fn shuffle_tile_rect_for(ctx: &Context, title: &str) -> Rect {
        let tokens = Tokens::DARK;
        let mut found = None;
        run(ctx, input(Vec::new()), |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let band = hero::Band {
                    eyebrow: "Playlist",
                    title,
                    meta: Vec::new(),
                    credit: None,
                    description: "",
                    cover: Cover::Missing,
                };
                let _ = hero::show_with(ui, &band, &tokens, hero::Title::Plain);
                let row = actionbar::row_rect(ui);
                found = tiles_of(row).shuffle;
            });
        });
        found.expect("the band and its action row lay out")
    }

    fn queued(queue: &Queue) -> usize {
        usize::from(queue.current().is_some()) + queue.upcoming().len()
    }

    /// Every uri the queue holds, the current entry first.
    fn queue_uris(queue: &Queue) -> Vec<String> {
        let mut uris: Vec<String> = queue
            .current()
            .map(|entry| entry.uri.clone())
            .into_iter()
            .collect();
        uris.extend(queue.upcoming().map(|(_, entry)| entry.uri.clone()));
        uris
    }

    /// Every uri the whole sixty-two-row list holds, in its own order.
    fn whole_list() -> Vec<String> {
        (0..62).map(|index| format!("t{index}")).collect()
    }

    #[test]
    fn typing_in_the_filter_keeps_the_matching_rows_with_their_true_numbers() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let layout = tracklist_frame_in(&ctx, ALL, &mut library);
        let field = field_at(&layout);
        let _ = click_in(
            ALL,
            &ctx,
            field.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );
        let output = frame_in(
            &ctx,
            ALL,
            &mut library,
            vec![Event::Text("midnight".to_owned())],
        );

        assert_eq!(
            painted_rows(&output),
            MIDNIGHT.to_vec(),
            "the twelve matching rows are drawn, by their true indices"
        );
        assert!(
            drawn_texts(&output).contains(&"40"),
            "row forty still shows forty: {:?}",
            drawn_texts(&output)
        );
        let tops: Vec<f32> = MIDNIGHT
            .iter()
            .map(|index| {
                text_rect(&output, &format!("t{index}"))
                    .unwrap_or_else(|| panic!("t{index} was not painted"))
                    .top()
            })
            .collect();
        assert!(
            tops.windows(2)
                .all(|pair| (pair[1] - pair[0] - extent::ROW_H).abs() < 0.01),
            "the kept rows sit in consecutive slots: {tops:?}"
        );
    }

    #[test]
    fn the_count_line_reads_n_of_total_tracks() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());
        library.track_filter.edit(PLAYLIST).push_str("midnight");

        let output = tracklist_frame_in(&ctx, ALL, &mut library);
        let field = field_at(&output);
        let count = text_rect(&output, "12 OF 62 TRACKS").expect("the count is painted");
        assert!(
            count.right() <= field.left() - crate::theme::actionbar::TOOLS_GAP + 0.01,
            "the count sits one tools gap left of the field: {count:?} against {field:?}"
        );
        assert!(
            !drawn_texts(&output).contains(&"FILTER"),
            "text replaces the placeholder"
        );

        library.track_filter.edit(PLAYLIST).clear();
        library.track_filter.edit(PLAYLIST).push_str("   ");
        let output = tracklist_frame_in(&ctx, ALL, &mut library);
        assert!(
            !drawn_texts(&output)
                .iter()
                .any(|text| text.contains(" OF ")),
            "a whitespace query is no filter and paints no count"
        );
    }

    #[test]
    fn at_900_the_count_gives_its_room_to_the_rail() {
        let ctx = context();
        // The rail starts at 900, inclusive, and the count is what steps aside
        // for it: the field keeps its 176 and the list still narrows.
        let rail = Vec2::new(900.0, 3200.0);
        let mut library = library_with(midnight_tracks());
        library.track_filter.edit(PLAYLIST).push_str("midnight");

        let output = tracklist_frame_in(&ctx, rail, &mut library);

        assert_eq!(
            painted_rows(&output),
            MIDNIGHT.to_vec(),
            "the query still narrows the list in a rail"
        );
        assert!(
            field_at(&output).width() == crate::theme::actionbar::FIELD_W,
            "the field keeps its width: {:?}",
            field_at(&output)
        );
        assert!(
            !drawn_texts(&output)
                .iter()
                .any(|text| text.contains(" OF ")),
            "the count is hidden at 900: {:?}",
            drawn_texts(&output)
        );
    }

    #[test]
    fn the_first_keystroke_replaces_the_placeholder_in_its_own_frame() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let layout = tracklist_frame_in(&ctx, ALL, &mut library);
        assert!(
            drawn_texts(&layout).contains(&"FILTER"),
            "the empty field shows its placeholder"
        );
        let field = field_at(&layout);
        let _ = click_in(
            ALL,
            &ctx,
            field.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );

        // `i` is in every `Midnight Artist` row and in no title, so the first
        // character is enough to tell the narrowed list from the whole one.
        let typed = frame_in(&ctx, ALL, &mut library, vec![Event::Text("i".to_owned())]);
        let texts = drawn_texts(&typed);

        assert_eq!(
            painted_rows(&typed),
            MIDNIGHT.to_vec(),
            "the first character narrows the rows on its own frame"
        );
        assert!(
            !texts.contains(&"FILTER"),
            "the placeholder leaves with the first character: {texts:?}"
        );
        assert!(
            texts.contains(&"i"),
            "the field paints the character on its own frame: {texts:?}"
        );
        assert!(
            texts.contains(&"12 OF 62 TRACKS"),
            "the count is on the keystroke's own frame: {texts:?}"
        );

        let clear = clear_box(field_at(&typed));
        assert!(
            painted_boxes(&typed)
                .iter()
                .any(|glyph| clear.contains_rect(*glyph)),
            "the clear control appears with the text: {clear:?}"
        );
        for text in painted_text_boxes(&typed) {
            assert!(
                !text.intersects(clear),
                "no text sits under the clear control: {text:?} against {clear:?}"
            );
        }
    }

    #[test]
    fn clearing_the_filter_restores_every_row_in_order() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());
        library.track_filter.edit(PLAYLIST).push_str("midnight");
        let narrowed = tracklist_frame_in(&ctx, ALL, &mut library);
        assert_eq!(painted_rows(&narrowed), MIDNIGHT.to_vec());

        library.track_filter.clear();
        let output = tracklist_frame_in(&ctx, ALL, &mut library);

        assert_eq!(
            painted_rows(&output),
            (0..62).collect::<Vec<_>>(),
            "clearing restores every row, in the list's order"
        );
        assert!(
            !drawn_texts(&output)
                .iter()
                .any(|text| text.contains(" OF "))
        );
    }

    #[test]
    fn the_clear_button_clears_in_the_frame_it_is_clicked() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let layout = tracklist_frame_in(&ctx, ALL, &mut library);
        let field = field_at(&layout);
        let _ = click_in(
            ALL,
            &ctx,
            field.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );
        let typed = frame_in(
            &ctx,
            ALL,
            &mut library,
            vec![Event::Text("midnight".to_owned())],
        );
        assert_eq!(painted_rows(&typed), MIDNIGHT.to_vec());

        let clear = clear_point(field_at(&typed));
        let mut output = typed;
        for event in crate::view::test_support::click_events(clear) {
            output = frame_in(&ctx, ALL, &mut library, vec![event]);
        }

        assert_eq!(
            painted_rows(&output),
            (0..62).collect::<Vec<_>>(),
            "the click frame itself shows every row"
        );
        assert!(
            !drawn_texts(&output)
                .iter()
                .any(|text| text.contains(" OF ")),
            "the count is gone with the query"
        );
        assert!(library.track_filter.edit(PLAYLIST).is_empty());

        // The chrome follows the click on its own frame: the clear control goes
        // with the text it cleared, and the hairline already says the field kept
        // the keyboard the click gave it.
        assert!(
            drawn_texts(&output).contains(&"FILTER"),
            "the placeholder is back on the click frame"
        );
        let clear = clear_box(field_at(&output));
        assert!(
            !painted_boxes(&output)
                .iter()
                .any(|glyph| clear.contains_rect(*glyph)),
            "the clear control leaves with the text: {clear:?}"
        );
        assert_eq!(
            filter_hairline(&output).1,
            Tokens::DARK.palette.text,
            "the hairline is focused on the click frame"
        );

        let _ = tracklist_frame_in(&ctx, ALL, &mut library);
        assert!(ctx.text_edit_focused(), "the field keeps the keyboard");
    }

    #[test]
    fn escape_in_the_focused_field_clears_and_returns_the_keyboard() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let layout = tracklist_frame_in(&ctx, ALL, &mut library);
        let field = field_at(&layout);
        let _ = click_in(
            ALL,
            &ctx,
            field.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );
        let typed = frame_in(
            &ctx,
            ALL,
            &mut library,
            vec![Event::Text("midnight".to_owned())],
        );
        assert_eq!(painted_rows(&typed), MIDNIGHT.to_vec());

        let output = frame_in(&ctx, ALL, &mut library, vec![key(egui::Key::Escape)]);

        assert!(library.track_filter.edit(PLAYLIST).is_empty());
        assert_eq!(
            painted_rows(&output),
            (0..62).collect::<Vec<_>>(),
            "the full list is back in the Escape frame"
        );
        assert!(!ctx.text_edit_focused(), "Escape returns the keyboard");

        // The chrome is the Escape frame's too: the placeholder is back, the
        // clear control is not, and the hairline has already let focus go.
        assert!(
            drawn_texts(&output).contains(&"FILTER"),
            "the placeholder returns on the Escape frame"
        );
        let clear = clear_box(field_at(&output));
        assert!(
            !painted_boxes(&output)
                .iter()
                .any(|glyph| clear.contains_rect(*glyph)),
            "the clear control leaves with the text: {clear:?}"
        );
        assert_eq!(
            filter_hairline(&output).1,
            Tokens::DARK.palette.border,
            "the hairline gives up the keyboard on the Escape frame"
        );
    }

    #[test]
    fn escape_the_frame_after_a_click_still_clears() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());
        library.track_filter.edit(PLAYLIST).push_str("midnight");

        let layout = tracklist_frame_in(&ctx, ALL, &mut library);
        let centre = field_at(&layout).center();
        let _ = frame_in(&ctx, ALL, &mut library, vec![Event::PointerMoved(centre)]);
        let _ = frame_in(
            &ctx,
            ALL,
            &mut library,
            vec![crate::view::test_support::button(centre, true)],
        );
        let output = frame_in(&ctx, ALL, &mut library, vec![key(egui::Key::Escape)]);

        assert!(
            library.track_filter.edit(PLAYLIST).is_empty(),
            "the Escape immediately after a click still clears"
        );
        assert_eq!(painted_rows(&output), (0..62).collect::<Vec<_>>());
        assert!(!ctx.text_edit_focused());
    }

    #[test]
    fn escape_in_an_empty_focused_field_surrenders_focus() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let layout = tracklist_frame_in(&ctx, ALL, &mut library);
        let field = field_at(&layout);
        let _ = click_in(
            ALL,
            &ctx,
            field.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );
        assert!(ctx.text_edit_focused(), "the click focused the field");

        let _ = frame_in(&ctx, ALL, &mut library, vec![key(egui::Key::Escape)]);

        assert!(
            !ctx.text_edit_focused(),
            "Escape with nothing to clear still returns the keyboard"
        );
    }

    #[test]
    fn escape_with_focus_on_a_row_leaves_the_query() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());
        library.track_filter.edit(PLAYLIST).push_str("midnight");
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let _ = tracklist_frame_in(&ctx, PROBE, &mut library);
        let row = row_rect_for(&ctx, 12, 0).center();
        let _ = click(&ctx, row, &mut library, &mut transport, &mut queue);

        let output = frame_in(&ctx, PROBE, &mut library, vec![key(egui::Key::Escape)]);

        assert_eq!(
            library.track_filter.needle(PLAYLIST).as_deref(),
            Some("midnight"),
            "a page-wide Escape would fight the drawer's"
        );
        let painted = painted_rows(&output);
        assert!(!painted.is_empty(), "the filtered rows are still drawn");
        assert_eq!(
            painted,
            MIDNIGHT[..painted.len()].to_vec(),
            "and they are still the matching rows"
        );
    }

    #[test]
    fn a_query_with_no_match_hides_the_head_and_says_so() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());
        library.track_filter.edit(PLAYLIST).push_str("zzz");

        let output = tracklist_frame_in(&ctx, ALL, &mut library);
        let texts = drawn_texts(&output);

        assert!(
            texts.contains(&"Nothing in this list matches \u{201c}zzz\u{201d}"),
            "the note names the query: {texts:?}"
        );
        assert!(!texts.contains(&"TITLE"), "the head hides with the rows");
        assert!(
            texts.contains(&"0 OF 62 TRACKS"),
            "the count still says zero"
        );
        assert!(field_at(&output).width() > 0.0, "the field stays drawn");
    }

    #[test]
    fn a_shift_click_on_a_filtered_list_picks_only_visible_rows() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());
        library.track_filter.edit(PLAYLIST).push_str("midnight");
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let _ = tracklist_frame_in(&ctx, PROBE, &mut library);
        let first = row_rect_for(&ctx, 12, 0).center();
        let third = row_rect_for(&ctx, 12, 2).center();
        let _ = click(&ctx, first, &mut library, &mut transport, &mut queue);
        click_holding(
            &ctx,
            third,
            Modifiers::SHIFT,
            &mut library,
            &mut transport,
            &mut queue,
        );

        for row in [1, 4, 9] {
            assert!(
                picked(&ctx, PLAYLIST, 62, row),
                "visible row {row} is in the span"
            );
        }
        for row in [0, 2, 3, 12] {
            assert!(
                !picked(&ctx, PLAYLIST, 62, row),
                "hidden row {row} never enters the span"
            );
        }
    }

    #[test]
    fn a_pick_made_before_filtering_loses_its_hidden_rows() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());
        let list = egui::Id::unique(("playlist-tracks", PLAYLIST));
        crate::view::carry::select_rows(&ctx, list, 0..5);
        assert!(picked(&ctx, PLAYLIST, 62, 0));

        library.track_filter.edit(PLAYLIST).push_str("midnight");
        let output = tracklist_frame_in(&ctx, ALL, &mut library);

        assert!(!picked(&ctx, PLAYLIST, 62, 0), "a hidden pick is dropped");
        assert!(!picked(&ctx, PLAYLIST, 62, 2));
        assert!(picked(&ctx, PLAYLIST, 62, 1), "a visible pick survives");
        assert!(picked(&ctx, PLAYLIST, 62, 4));
        assert_eq!(painted_rows(&output), MIDNIGHT.to_vec());
    }

    #[test]
    fn reloading_the_same_playlist_keeps_the_query_and_the_narrowed_rows() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());
        library.track_filter.edit(PLAYLIST).push_str("midnight");
        let narrowed = tracklist_frame_in(&ctx, ALL, &mut library);
        assert_eq!(painted_rows(&narrowed), MIDNIGHT.to_vec());

        library.select(PLAYLIST);
        assert!(matches!(library.tracks, Load::Loading));
        let _ = tracklist_frame_in(&ctx, ALL, &mut library);
        library.apply(&Update::Tracks {
            playlist: PLAYLIST.to_owned(),
            tracks: midnight_tracks(),
            cover: None,
            access: access(true),
        });
        let output = tracklist_frame_in(&ctx, ALL, &mut library);

        assert_eq!(
            painted_rows(&output),
            MIDNIGHT.to_vec(),
            "the reload is not a page change"
        );
        assert!(
            drawn_texts(&output).contains(&"midnight"),
            "the field comes back populated: {:?}",
            drawn_texts(&output)
        );
    }

    #[test]
    fn a_failed_load_clears_the_query() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());
        library.track_filter.edit(PLAYLIST).push_str("midnight");
        let _ = tracklist_frame_in(&ctx, ALL, &mut library);

        library.apply(&Update::TracksFailed {
            playlist: PLAYLIST.to_owned(),
            failure: Failure::Load,
        });
        let output = tracklist_frame_in(&ctx, ALL, &mut library);

        assert_eq!(library.track_filter.needle(PLAYLIST), None);
        assert!(
            !drawn_texts(&output).contains(&"FILTER"),
            "no field to see it in"
        );
        assert!(drawn_texts(&output).contains(&crate::signin::failure_message(Failure::Load)));
    }

    #[test]
    fn an_empty_list_clears_the_query() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());
        library.track_filter.edit(PLAYLIST).push_str("midnight");
        let _ = tracklist_frame_in(&ctx, ALL, &mut library);

        library.apply(&Update::Tracks {
            playlist: PLAYLIST.to_owned(),
            tracks: Vec::new(),
            cover: None,
            access: access(true),
        });
        let output = tracklist_frame_in(&ctx, ALL, &mut library);

        assert_eq!(library.track_filter.needle(PLAYLIST), None);
        assert!(
            drawn_texts(&output).contains(&"No tracks in this playlist."),
            "the empty page is unfiltered"
        );
    }

    #[test]
    fn a_query_survives_loading_and_its_field_is_absent_meanwhile() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());
        library.track_filter.edit(PLAYLIST).push_str("midnight");
        let _ = tracklist_frame_in(&ctx, ALL, &mut library);

        library.select(PLAYLIST);
        let output = tracklist_frame_in(&ctx, ALL, &mut library);

        assert_eq!(
            library.track_filter.needle(PLAYLIST).as_deref(),
            Some("midnight"),
            "a reload of the same URI keeps the query"
        );
        assert!(
            !drawn_texts(&output)
                .iter()
                .any(|text| text.contains(" OF ")),
            "the skeleton has no count"
        );
        assert!(!drawn_texts(&output).contains(&"midnight"));
    }

    #[test]
    fn deleting_every_visible_row_keeps_the_field_and_the_no_match_note() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());
        library.track_filter.edit(PLAYLIST).push_str("midnight");
        let _ = tracklist_frame_in(&ctx, ALL, &mut library);

        let remainder: Vec<TrackEntry> = midnight_tracks()
            .into_iter()
            .filter(|entry| !entry.artists.contains("Midnight"))
            .collect();
        library.apply(&Update::Tracks {
            playlist: PLAYLIST.to_owned(),
            tracks: remainder,
            cover: None,
            access: access(true),
        });
        let output = tracklist_frame_in(&ctx, ALL, &mut library);
        let texts = drawn_texts(&output);

        assert!(
            texts.contains(&"Nothing in this list matches \u{201c}midnight\u{201d}"),
            "the user can still see why, and clear it: {texts:?}"
        );
        assert!(texts.contains(&"0 OF 50 TRACKS"));
        assert!(field_at(&output).width() > 0.0, "the field is still drawn");
    }

    #[test]
    fn another_page_shows_an_unfiltered_list() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());
        library.track_filter.edit(PLAYLIST).push_str("midnight");
        let _ = tracklist_frame_in(&ctx, ALL, &mut library);

        library.select("spotify:playlist:b");
        library.apply(&Update::Tracks {
            playlist: "spotify:playlist:b".to_owned(),
            tracks: vec![track("u0", "t0"), track("u1", "t1")],
            cover: None,
            access: access(true),
        });
        let output = tracklist_frame_in(&ctx, ALL, &mut library);

        assert_eq!(library.track_filter.needle("spotify:playlist:b"), None);
        assert_eq!(painted_rows(&output), vec![0, 1], "the new page is whole");
        assert!(
            !drawn_texts(&output)
                .iter()
                .any(|text| text.contains(" OF "))
        );
    }

    #[test]
    fn a_filtered_editable_page_offers_no_drop_zone_and_an_unfiltered_one_does() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());

        let _ = tracklist_frame_in(&ctx, ALL, &mut library);
        assert!(
            crate::view::carry::take_offer(&ctx).is_some(),
            "an unfiltered editable page offers its rows"
        );

        library.track_filter.edit(PLAYLIST).push_str("midnight");
        let output = tracklist_frame_in(&ctx, ALL, &mut library);
        assert_eq!(painted_rows(&output), MIDNIGHT.to_vec());
        assert!(
            crate::view::carry::take_offer(&ctx).is_none(),
            "a filtered page offers no reorder"
        );

        library.track_filter.clear();
        let _ = tracklist_frame_in(&ctx, ALL, &mut library);
        assert!(
            crate::view::carry::take_offer(&ctx).is_some(),
            "clearing the query restores the drop zone"
        );
    }

    #[test]
    fn a_filtered_page_still_lifts_a_pick_to_the_rail_but_offers_nothing() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());
        library.track_filter.edit(PLAYLIST).push_str("midnight");
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let _ = tracklist_frame_in(&ctx, PROBE, &mut library);
        let first = row_rect_for(&ctx, 12, 0).center();
        lift_row(&ctx, first, &mut library, &mut transport, &mut queue);

        let carried = crate::view::carry::carried(&ctx).expect("a filtered row still lifts");
        assert_eq!(carried.rows, vec![1], "the lift carries the true index");
        assert!(
            crate::view::carry::take_offer(&ctx).is_none(),
            "the page still withholds the reorder"
        );
    }

    #[test]
    fn a_filtered_double_click_plays_the_true_index() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());
        library.track_filter.edit(PLAYLIST).push_str("midnight");
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let _ = tracklist_frame_in(&ctx, PROBE, &mut library);
        let first = row_rect_for(&ctx, 12, 0).center();
        let command = double_click(&ctx, first, &mut library, &mut transport, &mut queue);

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "t1".to_owned()
            }),
            "slot zero is the second true row"
        );
    }

    #[test]
    fn the_band_play_and_shuffle_ignore_the_filter() {
        let ctx = context();

        // The band is the whole list's, hidden rows included: both tiles start
        // the queue on the true first row (the first row the query leaves is
        // `t1`), and both hand the queue all sixty-two entries.
        for (shuffled, tile) in [
            (false, play_tile_rect_for as fn(&Context, &str) -> Rect),
            (true, shuffle_tile_rect_for),
        ] {
            let mut library = library_named(midnight_tracks(), "Evening");
            library.track_filter.edit(PLAYLIST).push_str("midnight");
            let mut transport = Transport::default();
            let mut queue = Queue::default();
            let at = tile(&ctx, "Evening").center();
            let command = click(&ctx, at, &mut library, &mut transport, &mut queue);

            assert_eq!(
                command,
                Some(Command::Play {
                    uri: "t0".to_owned()
                }),
                "the band starts the true first row (shuffled: {shuffled})"
            );
            assert_eq!(
                queued(&queue),
                62,
                "the queue holds the whole playlist (shuffled: {shuffled})"
            );
            assert_eq!(
                queue.current().map(|entry| entry.uri.as_str()),
                Some("t0"),
                "the true first row is the current one (shuffled: {shuffled})"
            );
            let mut uris = queue_uris(&queue);
            let mut expected = whole_list();
            if shuffled {
                // Shuffle draws the rest in its own order, so the whole list is
                // read as the same set of entries, not the same sequence.
                uris.sort();
                expected.sort();
            }
            assert_eq!(
                uris, expected,
                "every true row is in the queue (shuffled: {shuffled})"
            );
        }
    }

    #[test]
    fn a_play_from_the_page_remembers_the_playlist() {
        let ctx = context();
        let mut library = library_named(midnight_tracks(), "Evening");
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let at = play_tile_rect_for(&ctx, "Evening").center();

        let command = click(&ctx, at, &mut library, &mut transport, &mut queue);

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "t0".to_owned()
            })
        );
        assert_eq!(
            queue.origin(),
            Some(PLAYLIST),
            "a move from here has a page to carry"
        );
    }

    #[test]
    fn the_filter_field_takes_enter_and_leaves_it_alone() {
        let ctx = context();
        let mut library = library_with(midnight_tracks());
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let tokens = Tokens::DARK;

        let layout = tracklist_frame_in(&ctx, ALL, &mut library);
        let field = field_at(&layout);
        let _ = click_in(
            ALL,
            &ctx,
            field.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );
        let typed = frame_in(
            &ctx,
            ALL,
            &mut library,
            vec![Event::Text("midnight".to_owned())],
        );
        assert_eq!(painted_rows(&typed), MIDNIGHT.to_vec());

        let mut command = None;
        let output = run(&ctx, sized_input(ALL, vec![key(egui::Key::Enter)]), |ui| {
            command = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });

        assert_eq!(command, None, "Enter in the field plays nothing");
        assert_eq!(painted_rows(&output), MIDNIGHT.to_vec());
        assert_eq!(library.track_filter.edit(PLAYLIST).as_str(), "midnight");
        // Enter is consumed and nothing else moves: the field keeps its text and
        // the keyboard it was given, so the hairline still shows the focus.
        assert!(
            drawn_texts(&output).contains(&"midnight"),
            "the field keeps its text: {:?}",
            drawn_texts(&output)
        );
        assert!(
            ctx.text_edit_focused(),
            "Enter leaves the keyboard in the field"
        );
        assert_eq!(
            filter_hairline(&output).1,
            tokens.palette.text,
            "the hairline still shows the field's focus"
        );
    }

    /// The top of the list's own viewport: `tracks_view` starts flush with the
    /// window's top edge, and the scroll area fills what is left of it.
    const VIEWPORT_TOP: f32 = 0.0;

    /// Where a row that pins under the action row comes to rest: the viewport's
    /// top edge plus the height the action row condenses to.
    const PIN_TOP: f32 = VIEWPORT_TOP + crate::theme::actionbar::STUCK_H;

    /// Every box a text shape with `label` was painted in, in paint order.
    fn text_rects(output: &egui::FullOutput, label: &str) -> Vec<Rect> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                    Some(Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
            .collect()
    }

    /// The AccessKit node a frame announced for `label` inside `rect`.
    fn announced_in(
        output: &egui::FullOutput,
        label: &str,
        rect: Rect,
    ) -> Option<egui::accesskit::NodeId> {
        let update = output.platform_output.accesskit_update.as_ref()?;
        update.nodes.iter().find_map(|(id, node)| {
            let bounds = node.bounds()?;
            let bounds = Rect::from_min_max(
                Pos2::new(bounds.x0 as f32, bounds.y0 as f32),
                Pos2::new(bounds.x1 as f32, bounds.y1 as f32),
            );
            (node.label() == Some(label) && bounds.intersects(rect)).then_some(*id)
        })
    }

    /// Whether a frame's AccessKit update still contains `id`.
    fn announced(output: &egui::FullOutput, id: egui::accesskit::NodeId) -> bool {
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .is_some_and(|update| update.nodes.iter().any(|(node, _)| *node == id))
    }

    /// The action row's box, read from the surface it fills once it pins.
    ///
    /// The row's condensed height is the only fill of this tone on the page that
    /// tall, so a pinned frame has exactly one.
    fn pinned_bar(output: &egui::FullOutput) -> Option<Rect> {
        let tokens = Tokens::DARK;
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape)
                    if shape.fill == tokens.palette.surface
                        && (shape.rect.height() - crate::theme::actionbar::STUCK_H).abs()
                            < 0.01 =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
    }

    /// The field's box on a stuck frame, solved from the row's own drawn box: a
    /// stuck field paints no hairline of its own to read it from.
    fn stuck_field(output: &egui::FullOutput, size: Vec2) -> Rect {
        let bar = pinned_bar(output).expect("the row is pinned");
        crate::layout::actionbar::solve(
            bar,
            crate::layout::actionbar::Kind::Playlist(crate::layout::actionbar::Form::at(
                size.x,
                Table::Full,
            )),
            crate::layout::actionbar::Parts {
                shuffle: true,
                field: true,
                follow: None,
            },
        )
        .field
        .expect("the pinned row has a field")
    }

    /// One frame with the wheel moved by `points` at `at`, which is how the
    /// list scrolls.
    fn wheel(
        ctx: &Context,
        size: Vec2,
        at: Pos2,
        points: f32,
        library: &mut Library,
    ) -> egui::FullOutput {
        wheel_in(ctx, size, Table::Full, at, points, library)
    }

    /// The same, on a page handed `table`.
    fn wheel_in(
        ctx: &Context,
        size: Vec2,
        table: Table,
        at: Pos2,
        points: f32,
        library: &mut Library,
    ) -> egui::FullOutput {
        table_frame_in(
            ctx,
            size,
            table,
            library,
            vec![
                Event::PointerMoved(at),
                Event::MouseWheel {
                    unit: MouseWheelUnit::Point,
                    delta: vec2(0.0, points),
                    phase: TouchPhase::Move,
                    source: MouseWheelSource::Unknown,
                    modifiers: Modifiers::NONE,
                },
            ],
        )
    }

    /// Wheels down until the action row pins, answering the frame it pinned on.
    fn wheel_until_pinned(
        ctx: &Context,
        size: Vec2,
        table: Table,
        library: &mut Library,
    ) -> Option<egui::FullOutput> {
        let at = Pos2::new(size.x / 2.0, size.y - extent::ROW_H);
        for _ in 0..240 {
            let output = wheel_in(ctx, size, table, at, -extent::ROW_H, library);
            if pinned_bar(&output).is_some() {
                return Some(output);
            }
        }
        None
    }

    /// The first frame whose action row is pinned to the top of the list.
    fn pinned_frame(ctx: &Context, size: Vec2, library: &mut Library) -> egui::FullOutput {
        pinned_frame_in(ctx, size, Table::Full, library)
    }

    /// The same, on a page handed `table`.
    fn pinned_frame_in(
        ctx: &Context,
        size: Vec2,
        table: Table,
        library: &mut Library,
    ) -> egui::FullOutput {
        wheel_until_pinned(ctx, size, table, library).unwrap_or_else(|| {
            panic!(
                "the action row never pinned in a {}x{} window",
                size.x, size.y
            )
        })
    }

    #[test]
    fn the_bar_pins_to_the_top_of_the_list() {
        let ctx = context();
        let mut library = library_named(midnight_tracks(), "Late Night Drive");
        // Short enough that sixty-two rows scroll well past the band.
        let size = Vec2::new(900.0, 400.0);

        let layout = tracklist_frame_in(&ctx, size, &mut library);
        assert!(
            pinned_bar(&layout).is_none(),
            "the row starts in its place in the flow"
        );
        let flow = field_at(&layout);
        assert!(
            flow.top() > VIEWPORT_TOP,
            "the field starts below the viewport top: {flow:?}"
        );

        let pinned = pinned_frame(&ctx, size, &mut library);
        let bar = pinned_bar(&pinned).expect("the action row is pinned");
        assert!(
            (bar.top() - VIEWPORT_TOP).abs() < 0.01,
            "the row sits at the viewport's top edge: {bar:?}"
        );
        assert!(
            (bar.height() - crate::theme::actionbar::STUCK_H).abs() < 0.01,
            "the row condenses to the height the rows clear: {bar:?}"
        );
        let field = stuck_field(&pinned, size);
        assert!(
            (field.center().y - bar.center().y).abs() < 0.01,
            "the field sits on the pinned row's centre line: {field:?} against {bar:?}"
        );

        // One field, one placeholder: a second copy left at the row's place in
        // the flow would be a second `FILTER`, above the pinned one.
        let placeholders: Vec<Rect> = text_rects(&pinned, "FILTER");
        assert_eq!(placeholders.len(), 1, "the row is painted once, at the top");
        assert!(
            placeholders[0].top() >= bar.top() - 0.01,
            "no `FILTER` paints above the pinned row: {:?} against {bar:?}",
            placeholders[0]
        );

        // Still there while the list keeps scrolling under it: a dozen more rows
        // must not drift the bar or paint a second copy of the field.
        let at = Pos2::new(size.x / 2.0, size.y - extent::ROW_H);
        let mut settled = pinned;
        for _ in 0..12 {
            settled = wheel(&ctx, size, at, -extent::ROW_H, &mut library);
        }
        let bar = pinned_bar(&settled).expect("the row stays pinned");
        assert!(
            (bar.top() - VIEWPORT_TOP).abs() < 0.01,
            "the pinned row stays at the viewport top: {bar:?}"
        );
        assert!(
            (bar.height() - crate::theme::actionbar::STUCK_H).abs() < 0.01,
            "the pinned row keeps its height: {bar:?}"
        );
        let placeholders: Vec<Rect> = text_rects(&settled, "FILTER");
        assert_eq!(
            placeholders.len(),
            1,
            "one field is painted, not a second copy left in the flow"
        );
        assert!(
            placeholders[0].top() >= bar.top() - 0.01,
            "no `FILTER` paints above the pinned row: {:?} against {bar:?}",
            placeholders[0]
        );

        // The wheel back hands the row its place in the flow again.
        let mut restored = None;
        for _ in 0..240 {
            let output = wheel(&ctx, size, at, extent::ROW_H, &mut library);
            if pinned_bar(&output).is_none() && (field_at(&output).top() - flow.top()).abs() < 0.01
            {
                restored = Some(output);
                break;
            }
        }
        let restored = restored.expect("the row returns to its place in the flow");
        assert!(
            field_at(&restored).top() > VIEWPORT_TOP + 0.01,
            "back at the top of the scroll, the row is in its flow again"
        );
        assert_eq!(
            text_rects(&restored, "FILTER").len(),
            1,
            "the row is still painted once"
        );
    }

    #[test]
    fn a_pinned_field_still_edits_in_the_same_frame() {
        let ctx = context();
        let mut library = library_named(midnight_tracks(), "Late Night Drive");
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let size = Vec2::new(1000.0, 400.0);

        let _ = tracklist_frame_in(&ctx, size, &mut library);
        let pinned = pinned_frame(&ctx, size, &mut library);
        let bar = pinned_bar(&pinned).expect("the action row is pinned");
        let field = stuck_field(&pinned, size);
        assert!(
            (field.center().y - bar.center().y).abs() < 0.01,
            "the field is vertically centred on the pinned row: {field:?} against {bar:?}"
        );
        let before = painted_rows(&pinned);
        assert!(!before.is_empty(), "the list is drawn under the pin");

        let _ = clicks(
            size,
            &ctx,
            field.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
        );
        let typed = frame_in(
            &ctx,
            size,
            &mut library,
            vec![Event::Text("midnight".to_owned())],
        );

        let rows = painted_rows(&typed);
        assert!(!rows.is_empty(), "the narrowed list is still drawn");
        // The scroll offset is untouched, so the run of kept rows starts below
        // the top; what matters is that every painted row is a kept row, in the
        // list's order, where the whole list was painted before.
        let start = MIDNIGHT
            .iter()
            .position(|row| *row == rows[0])
            .unwrap_or_else(|| panic!("a kept row is painted first: {rows:?}"));
        assert_eq!(
            rows,
            MIDNIGHT[start..start + rows.len()].to_vec(),
            "the pinned field narrows the list on the keystroke's own frame: {before:?} was the whole list"
        );
        let typed_field = stuck_field(&typed, size);
        let count = text_rect(&typed, "12 OF 62 TRACKS").expect("the count is painted");
        assert!(
            count.right() <= typed_field.left() - crate::theme::actionbar::TOOLS_GAP + 0.01,
            "the count sits one tools gap left of the pinned field: {count:?} against {typed_field:?}"
        );
        assert!(
            drawn_texts(&typed).contains(&"midnight"),
            "the pinned field paints the query: {:?}",
            drawn_texts(&typed)
        );
    }

    #[test]
    fn no_row_paints_over_the_pinned_bar() {
        let ctx = context();
        let mut library = library_named(midnight_tracks(), "Late Night Drive");
        let size = Vec2::new(900.0, 400.0);

        let _ = tracklist_frame_in(&ctx, size, &mut library);
        let pinned = pinned_frame(&ctx, size, &mut library);
        let bar = pinned_bar(&pinned).expect("the action row is pinned");

        let mut painted = 0;
        for clipped in &pinned.shapes {
            let egui::epaint::Shape::Text(text) = &clipped.shape else {
                continue;
            };
            let Some(index) = text.galley.text().strip_prefix('t') else {
                continue;
            };
            if index.parse::<usize>().is_err() {
                continue;
            }
            painted += 1;
            assert!(
                clipped.clip_rect.top() >= bar.bottom() - 0.01,
                "row `{}` may paint from {:?}, above the pinned row's bottom {:?}",
                text.galley.text(),
                clipped.clip_rect,
                bar.bottom()
            );
        }
        assert!(painted > 0, "the list is still drawn under the pinned row");
    }

    #[test]
    fn the_pinned_field_keeps_its_keyboard() {
        let ctx = context();
        let mut library = library_named(midnight_tracks(), "Late Night Drive");
        library.track_filter.edit(PLAYLIST).push_str("midnight");
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let size = Vec2::new(900.0, 400.0);

        let layout = tracklist_frame_in(&ctx, size, &mut library);
        let field = field_at(&layout);
        let _ = clicks(
            size,
            &ctx,
            field.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
        );
        assert!(
            ctx.text_edit_focused(),
            "the click gave the field the keyboard"
        );

        let pinned = pinned_frame(&ctx, size, &mut library);
        assert!(
            pinned_bar(&pinned).is_some(),
            "the field is pinned with the row: {:?}",
            pinned_bar(&pinned)
        );
        assert!(
            ctx.text_edit_focused(),
            "a scroll frame leaves the pinned field its keyboard"
        );

        let cleared = frame_in(&ctx, size, &mut library, vec![key(egui::Key::Escape)]);

        let rows = painted_rows(&cleared);
        assert!(!rows.is_empty(), "the list is drawn after the Escape");
        assert_eq!(
            rows,
            (0..rows.len()).collect::<Vec<_>>(),
            "Escape clears the query from the pinned position, restoring the whole list in order"
        );
        assert!(library.track_filter.edit(PLAYLIST).is_empty());
        assert!(
            drawn_texts(&cleared).contains(&"FILTER"),
            "the pinned field shows its placeholder again: {:?}",
            drawn_texts(&cleared)
        );
        assert!(
            !ctx.text_edit_focused(),
            "Escape gives the keyboard back from the pinned position"
        );
    }

    /// A title's box and the rect the frame clipped it to.
    fn clipped_title(output: &egui::FullOutput, label: &str) -> Option<(Rect, Rect)> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => Some((
                    Rect::from_min_size(text.pos, text.galley.size()),
                    clipped.clip_rect,
                )),
                _ => None,
            })
    }

    /// How far a row's words sit below the row's own box, read from the first
    /// row the frame painted: the fold clips at the words, not at the box.
    fn title_inset(ctx: &Context, count: usize, output: &egui::FullOutput) -> f32 {
        let first = painted_rows(output)[0];
        let (title, _) =
            clipped_title(output, &format!("t{first}")).expect("the first row's title");
        title.top() - row_rect_for(ctx, count, first).top()
    }

    /// The row index, box and clip of the first title the frame painted.
    fn first_drawn_row(output: &egui::FullOutput) -> Option<(usize, Rect, Rect)> {
        let first = *painted_rows(output).first()?;
        let (title, clip) = clipped_title(output, &format!("t{first}"))?;
        Some((first, title, clip))
    }

    /// The title the frame painted inside `rect`, with the row it names.
    fn title_in(output: &egui::FullOutput, rect: Rect) -> Option<(usize, Rect, Rect)> {
        output.shapes.iter().find_map(|clipped| {
            let egui::epaint::Shape::Text(text) = &clipped.shape else {
                return None;
            };
            let index = text
                .galley
                .text()
                .strip_prefix('t')?
                .parse::<usize>()
                .ok()?;
            let title = Rect::from_min_size(text.pos, text.galley.size());
            rect.contains(title.center())
                .then_some((index, title, clipped.clip_rect))
        })
    }

    /// Idle frames until the rows stop moving, answering the frame they rest on.
    fn settled_frame(ctx: &Context, size: Vec2, library: &mut Library) -> egui::FullOutput {
        let mut output = frame_in(ctx, size, library, Vec::new());
        let mut last = f32::NAN;
        for _ in 0..120 {
            let top = first_drawn_row(&output).map_or(f32::NAN, |(_, title, _)| title.top());
            if (top - last).abs() < 0.01 {
                break;
            }
            last = top;
            output = frame_in(ctx, size, library, Vec::new());
        }
        output
    }

    /// The resting pinned frame whose first drawn row has its title cut off by
    /// the pin, and that row's true index.
    ///
    /// The finding needs a row the pin partly covers: an offset that stops a row
    /// exactly at the pin's bottom would test nothing, so the wheel is nudged in
    /// six-point steps and the list let rest until one does.
    fn pinned_over_a_row(
        ctx: &Context,
        size: Vec2,
        library: &mut Library,
    ) -> (egui::FullOutput, usize) {
        let at = Pos2::new(size.x / 2.0, size.y - extent::ROW_H);
        let mut output = settled_frame(ctx, size, library);
        for _ in 0..80 {
            if let Some(&hidden) = painted_rows(&output).first()
                && let Some((title, clip)) = clipped_title(&output, &format!("t{hidden}"))
                && title.top() < clip.top() - 0.5
                && title.bottom() < clip.bottom()
            {
                return (output, hidden);
            }
            let _ = wheel(ctx, size, at, -6.0, library);
            output = settled_frame(ctx, size, library);
        }
        panic!("the pin never covered a row's title");
    }

    /// Tabs until the keyboard lands on a row, answering the row's id.
    ///
    /// The bar's field, then its tools, come before the rows, so the first
    /// `ROW_H`-tall widget after the field is a row of the list.
    fn tab_to_a_row(ctx: &Context, size: Vec2, library: &mut Library) -> egui::Id {
        let [press, release] = crate::view::test_support::tab();
        for _ in 0..8 {
            let _ = frame_in(ctx, size, library, vec![press.clone()]);
            let focused = ctx.memory(|memory| memory.focused());
            let _ = frame_in(ctx, size, library, vec![release.clone()]);
            if let Some(id) = focused
                && ctx
                    .read_response(id)
                    .is_some_and(|row| (row.rect.height() - extent::ROW_H).abs() < 0.01)
            {
                return id;
            }
        }
        panic!("the keyboard never reached a row");
    }

    #[test]
    fn a_focused_row_is_not_left_under_the_pinned_bar() {
        let ctx = context();
        let mut library = library_named(midnight_tracks(), "Late Night Drive");
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let size = Vec2::new(900.0, 400.0);

        let _ = tracklist_frame_in(&ctx, size, &mut library);
        let (pinned, hidden) = pinned_over_a_row(&ctx, size, &mut library);
        let bar = pinned_bar(&pinned).expect("the action row is pinned");

        // The field takes the keyboard, and Tab walks past the row's tools and
        // into the list: the first row is the one the pin covers.
        let field = stuck_field(&pinned, size);
        let _ = clicks(
            size,
            &ctx,
            field.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
        );
        assert!(ctx.text_edit_focused(), "the field has no keyboard");
        let focused = tab_to_a_row(&ctx, size, &mut library);
        let row = ctx
            .read_response(focused)
            .expect("the focused row is drawn");
        assert!(
            row.rect.top() < bar.bottom(),
            "the pin covers the row the keyboard reached: {:?} against {bar:?}",
            row.rect
        );

        // The row asks to be brought into view; the pin stays at the top. The
        // scroll is animated, so the row is read until it stops moving, and
        // what it settles on is what the pin allows.
        let mut output = frame_in(&ctx, size, &mut library, Vec::new());
        let mut row = ctx
            .read_response(focused)
            .expect("the focused row is drawn");
        let mut last = f32::NAN;
        for _ in 0..120 {
            let bar = pinned_bar(&output).expect("the row stays pinned");
            if (row.rect.top() - last).abs() < 0.01 && row.rect.top() >= bar.bottom() - 0.01 {
                break;
            }
            last = row.rect.top();
            output = frame_in(&ctx, size, &mut library, Vec::new());
            row = ctx
                .read_response(focused)
                .expect("the focused row keeps being drawn");
        }
        let bar = pinned_bar(&output).expect("the row stays pinned");
        assert!(
            (bar.top() - VIEWPORT_TOP).abs() < 0.01,
            "the row stays pinned: {bar:?}"
        );
        assert_eq!(
            ctx.memory(|memory| memory.focused()),
            Some(focused),
            "the row keeps the keyboard"
        );
        assert!(
            row.rect.top() >= bar.bottom() - 0.01,
            "the focused row rests below the pin: {:?} against {bar:?}",
            row.rect
        );
        let (index, title, clip) =
            title_in(&output, row.rect).expect("the focused row paints its title");
        assert_eq!(index, hidden, "the focused row is the one the pin covered");
        assert!(
            title.top() >= bar.bottom() - 0.01,
            "the focused row's title is below the pin: {title:?} against {bar:?}"
        );
        assert!(
            clip.top() <= title.top() + 0.01 && clip.bottom() >= title.bottom() - 0.01,
            "the pin cuts none of the title off: {clip:?} against {title:?}"
        );
        assert!(
            row.interact_rect.height() > 0.0
                && row.interact_rect.top() <= title.top() + 0.01
                && row.interact_rect.bottom() >= title.bottom() - 0.01,
            "the focused row's body is still reachable: {:?}",
            row.interact_rect
        );
    }

    #[test]
    fn the_pinned_bar_owns_the_space_it_covers() {
        let ctx = context();
        let mut library = library_named(midnight_tracks(), "Late Night Drive");
        library.track_filter.edit(PLAYLIST).push_str("midnight");
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let size = Vec2::new(1000.0, 400.0);

        let _ = tracklist_frame_in(&ctx, size, &mut library);
        let (pinned, _hidden) = pinned_over_a_row(&ctx, size, &mut library);
        let bar = pinned_bar(&pinned).expect("the action row is pinned");
        let drawn = painted_rows(&pinned);
        assert!(!drawn.is_empty(), "rows are drawn under the pin");

        // A pick on a kept row that is off screen, so a press on any row under
        // the pin would replace it rather than look like it.
        let list = egui::Id::unique(("playlist-tracks", PLAYLIST));
        let kept = *MIDNIGHT
            .iter()
            .find(|row| !drawn.contains(row))
            .expect("a kept row is off screen");
        crate::view::carry::select_rows(&ctx, list, kept..kept + 1);

        // The count sits left of the field and on the pinned row's own line,
        // over the first row drawn under it: a press there is the bar's.
        let field = stuck_field(&pinned, size);
        let count = text_rect(&pinned, "12 OF 62 TRACKS").expect("the count is painted");
        assert!(
            count.top() >= bar.top() - 0.01 && count.bottom() <= bar.bottom() + 0.01,
            "the count is on the pinned row's line: {count:?} against {bar:?}"
        );
        assert!(
            count.right() <= field.left(),
            "the count sits left of the field: {count:?} against {field:?}"
        );

        let mut pressed = Vec::new();
        for at in [count.center(), bar.center()] {
            pressed.push(clicks(
                size,
                &ctx,
                at,
                1,
                &mut library,
                &mut transport,
                &mut queue,
            ));
        }

        assert!(
            pressed.iter().all(Option::is_none),
            "a press in the pinned row played a track: {pressed:?}"
        );
        assert!(
            queue.current().is_none(),
            "a press in the pinned row queued a track"
        );
        assert!(
            picked(&ctx, PLAYLIST, 62, kept),
            "a press in the pinned row changed the selection"
        );
        for row in 0..62 {
            if row != kept {
                assert!(
                    !picked(&ctx, PLAYLIST, 62, row),
                    "row {row} under the pinned row was pressed"
                );
            }
        }
    }

    #[test]
    fn the_playlist_bar_pins_and_rows_clear_it() {
        let ctx = context();
        let mut library = library_named(midnight_tracks(), "Late Night Drive");
        let size = Vec2::new(900.0, 400.0);

        let _ = tracklist_frame_in(&ctx, size, &mut library);
        let pinned = pinned_frame(&ctx, size, &mut library);
        let bar = pinned_bar(&pinned).expect("the action row is pinned");
        assert!(
            (bar.top() - VIEWPORT_TOP).abs() < 0.01,
            "the row sits at the viewport's top edge: {bar:?}"
        );
        assert!(
            (bar.bottom() - PIN_TOP).abs() < 0.01,
            "the row condenses to the height the rows clear: {bar:?}"
        );

        let mut painted = 0;
        for index in painted_rows(&pinned) {
            let (_, clip) = clipped_title(&pinned, &format!("t{index}"))
                .unwrap_or_else(|| panic!("row {index} paints a title"));
            painted += 1;
            assert!(
                clip.top() >= bar.bottom() - 0.01,
                "row {index} paints from {clip:?}, inside the pinned row {bar:?}"
            );
        }
        assert!(painted > 0, "the list is drawn under the pinned row");
    }

    #[test]
    fn the_playlist_bar_title_is_the_pages_name() {
        let ctx = context();
        let mut library = library_named(midnight_tracks(), "Late Night Drive");
        let size = Vec2::new(900.0, 400.0);

        let _ = tracklist_frame_in(&ctx, size, &mut library);
        let pinned = pinned_frame(&ctx, size, &mut library);
        let bar = pinned_bar(&pinned).expect("the action row is pinned");
        let labels: Vec<Rect> = text_rects(&pinned, "Late Night Drive");
        let label = labels
            .iter()
            .find(|rect| rect.top() >= bar.top() - 0.01 && rect.bottom() <= bar.bottom() + 0.01)
            .unwrap_or_else(|| {
                panic!("the pinned row paints the playlist's name: {labels:?} in {bar:?}")
            });
        assert!(
            label.left() > bar.left() + crate::theme::space::GUTTER,
            "the name stands beside the play tile, not in the gutter: {label:?}"
        );
        assert!(
            labels.len() > 1,
            "the band keeps its own copy of the name above the row: {labels:?}"
        );
    }

    #[test]
    fn a_focused_row_partly_under_the_bar_is_scrolled_clear_of_it() {
        let ctx = context();
        let mut library = library_named(midnight_tracks(), "Late Night Drive");
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let size = Vec2::new(900.0, 500.0);

        let _ = tracklist_frame_in(&ctx, size, &mut library);
        let pinned = pinned_frame(&ctx, size, &mut library);
        assert!(
            pinned_bar(&pinned).is_some(),
            "the action row is pinned before the press"
        );

        // The first row the list draws is wholly below the pinned row, so a
        // press on it lands on the row and not in the strip the row owns.
        let first = *painted_rows(&pinned)
            .first()
            .expect("the list is drawn under the pinned row");
        let title = clipped_title(&pinned, &format!("t{first}"))
            .expect("the row paints its title")
            .0;
        let _ = clicks(
            size,
            &ctx,
            Pos2::new(title.left() + 2.0, title.center().y),
            1,
            &mut library,
            &mut transport,
            &mut queue,
        );
        let focused = ctx
            .memory(|memory| memory.focused())
            .expect("the row took the keyboard");
        // Walk the list up until the row's top is inside the strip everything
        // pinned above the list covers, and let what the reveal asks for settle.
        let pinned_under = pinned_bar(&pinned).expect("the row is pinned").bottom();
        let at = Pos2::new(size.x / 2.0, size.y - extent::ROW_H);
        let mut covered = false;
        for _ in 0..80 {
            let row = ctx.read_response(focused).expect("the row is drawn").rect;
            if row.top() < pinned_under - 0.5 {
                covered = true;
                break;
            }
            let _ = wheel(&ctx, size, at, -6.0, &mut library);
        }
        assert!(covered, "the row never slid under the pinned row");

        let mut output = frame_in(&ctx, size, &mut library, Vec::new());
        let mut settled = false;
        for _ in 0..240 {
            if let Some(row) = ctx.read_response(focused)
                && row.rect.top() >= pinned_under - 0.01
            {
                settled = true;
                break;
            }
            output = frame_in(&ctx, size, &mut library, Vec::new());
        }
        assert_eq!(
            ctx.memory(|memory| memory.focused()),
            Some(focused),
            "the row keeps the keyboard"
        );
        assert!(
            settled,
            "the focused row never came out from under the pinned row"
        );

        let bar = pinned_bar(&output).expect("the row is still pinned");
        let row = ctx
            .read_response(focused)
            .expect("the row is drawn again")
            .rect;
        assert!(
            row.top() >= bar.bottom() - 0.01,
            "the focused row rests below the pinned row: {row:?} against {bar:?}"
        );
    }

    #[test]
    fn a_focused_row_wholly_under_the_bar_is_still_brought_out() {
        let ctx = context();
        let mut library = library_named(midnight_tracks(), "Late Night Drive");
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let size = Vec2::new(900.0, 500.0);

        let _ = tracklist_frame_in(&ctx, size, &mut library);
        let pinned = pinned_frame(&ctx, size, &mut library);
        let bar = pinned_bar(&pinned).expect("the action row is pinned");
        assert!(
            (bar.top() - VIEWPORT_TOP).abs() < 0.01,
            "the action row is pinned before the press"
        );

        // The first row the list draws is wholly below the pinned row, so a
        // press on its title lands on the row and not in the strip.
        let first = *painted_rows(&pinned)
            .first()
            .expect("the list is drawn under the pinned row");
        let title = clipped_title(&pinned, &format!("t{first}"))
            .expect("the row paints its title")
            .0;
        let _ = clicks(
            size,
            &ctx,
            Pos2::new(title.left() + 2.0, title.center().y),
            1,
            &mut library,
            &mut transport,
            &mut queue,
        );
        let focused = ctx
            .memory(|memory| memory.focused())
            .expect("the row took the keyboard");
        // The press lands on whichever row was under the pointer when the click
        // resolved, so the focused row is read from the frame it was drawn in
        // rather than assumed to be the one the pinned frame drew first.
        let after_click = frame_in(&ctx, size, &mut library, Vec::new());
        let focused_rect = ctx.read_response(focused).expect("the row is drawn").rect;
        let focused_row = title_in(&after_click, focused_rect)
            .expect("the focused row paints its title")
            .0;

        // One wheel jump of at least the pinned row and a whole row, and enough
        // to leave this row wholly above the strip, so the frame after it draws
        // nothing at the focused row: the cull skips it and `rows::body`, which
        // is a drawn row's own reveal, never runs.
        let at = Pos2::new(size.x / 2.0, size.y - extent::ROW_H);
        let strip = pinned_bar(&after_click)
            .expect("the row is pinned")
            .bottom();
        let jump =
            (focused_rect.bottom() - strip).max(crate::theme::actionbar::STUCK_H) + extent::ROW_H;
        let mut output = instant_frame_in(
            &ctx,
            size,
            &mut library,
            vec![
                Event::PointerMoved(at),
                Event::MouseWheel {
                    unit: MouseWheelUnit::Point,
                    delta: vec2(0.0, -jump),
                    phase: TouchPhase::Move,
                    source: MouseWheelSource::Unknown,
                    modifiers: Modifiers::NONE,
                },
            ],
        );

        let mut culled = false;
        let mut settled = false;
        for _ in 0..240 {
            let strip = pinned_bar(&output).expect("the row stays pinned").bottom();
            if !painted_rows(&output).contains(&focused_row) {
                culled = true;
            } else if culled
                && ctx
                    .read_response(focused)
                    .is_some_and(|row| row.rect.top() >= strip - 0.01)
            {
                settled = true;
                break;
            }
            // Every frame runs the wheel the way the window's own pass does, so
            // the notch's tail cannot keep moving the list under the reveal.
            output = instant_frame_in(&ctx, size, &mut library, Vec::new());
        }
        assert!(
            culled,
            "the jump never put the focused row wholly under the pinned row"
        );
        assert!(
            settled,
            "the focused row never came out from under the pinned row"
        );
        let strip = pinned_bar(&output).expect("the row stays pinned").bottom();
        let row = ctx
            .read_response(focused)
            .expect("the row is drawn again")
            .rect;
        assert!(
            row.top() >= strip - 0.01,
            "the focused row rests below the pinned row: {row:?} against {strip}"
        );
    }

    #[test]
    fn a_playlist_the_library_cannot_name_still_filters() {
        let ctx = context();
        // No entry: the library has no name for this playlist, so there is no
        // band and no title, but the tracks are there to filter and to play.
        let mut library = library_with(midnight_tracks());
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let layout = tracklist_frame_in(&ctx, ALL, &mut library);
        let field = field_at(&layout);
        assert!(
            field.width() > 0.0,
            "the row is drawn for a playlist the library cannot name"
        );
        assert!(
            drawn_texts(&layout).contains(&"FILTER"),
            "the field shows its placeholder: {:?}",
            drawn_texts(&layout)
        );
        let _ = click_in(
            ALL,
            &ctx,
            field.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );
        let typed = frame_in(
            &ctx,
            ALL,
            &mut library,
            vec![Event::Text("midnight".to_owned())],
        );
        assert_eq!(
            painted_rows(&typed),
            MIDNIGHT.to_vec(),
            "typing narrows the list on a page with no name"
        );
        assert!(
            drawn_texts(&typed).contains(&"12 OF 62 TRACKS"),
            "and the count is painted: {:?}",
            drawn_texts(&typed)
        );

        // The play tile works on the tracks the page has, even with no name.
        let row = crate::layout::actionbar::solve(
            Rect::from_min_max(
                Pos2::new(0.0, field.top() - crate::theme::actionbar::PAD_REST),
                Pos2::new(ALL.x, field.bottom() + crate::theme::actionbar::PAD_REST),
            ),
            crate::layout::actionbar::Kind::Playlist(crate::layout::actionbar::Form::at(
                ALL.x,
                Table::Full,
            )),
            crate::layout::actionbar::Parts {
                shuffle: true,
                field: true,
                follow: None,
            },
        );
        let command = click(
            &ctx,
            row.play.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );
        assert_eq!(
            command,
            Some(Command::Play {
                uri: "t0".to_owned()
            }),
            "a page with no name still plays its first track"
        );
    }

    #[test]
    fn at_760_the_stuck_title_and_heart_are_gone_and_the_field_fills() {
        let ctx = context();
        ctx.enable_accesskit();
        let mut library = library_named(midnight_tracks(), "Late Night Drive");
        library.track_filter.edit(PLAYLIST).push_str("midnight");
        let size = Vec2::new(760.0, 400.0);

        // The field the page actually draws is wider than the wide form's fixed
        // width: it has taken the room the count and the heart left.
        let layout = table_frame_in(&ctx, size, Table::Narrow, &mut library, Vec::new());
        let field = field_box_under(&layout, "midnight");
        assert!(
            field.width() > crate::theme::actionbar::FIELD_W,
            "the drawn field fills to the icons: {field:?}"
        );

        let pinned = pinned_frame_in(&ctx, size, Table::Narrow, &mut library);
        let bar = pinned_bar(&pinned).expect("the action row is pinned");
        assert!(
            !text_rects(&pinned, "Late Night Drive")
                .iter()
                .any(|label| label.top() >= bar.top() - 0.01
                    && label.bottom() <= bar.bottom() + 0.01),
            "a stuck narrow row paints no name: {:?}",
            text_rects(&pinned, "Late Night Drive")
        );
        for label in ["Save to your library", "Remove from your library"] {
            assert!(
                announced_in(&pinned, label, bar).is_none(),
                "a narrow row creates no heart control: {label} is announced in {bar:?}"
            );
        }

        // The page's own table is what condenses it: handed the full table, the
        // same width keeps the heart and the name.
        let mut wide = library_named(midnight_tracks(), "Late Night Drive");
        wide.track_filter.edit(PLAYLIST).push_str("midnight");
        let _ = table_frame_in(&ctx, size, Table::Full, &mut wide, Vec::new());
        let pinned_wide = pinned_frame_in(&ctx, size, Table::Full, &mut wide);
        let wide_bar = pinned_bar(&pinned_wide).expect("the action row is pinned");
        let heart = ["Save to your library", "Remove from your library"]
            .iter()
            .find_map(|label| announced_in(&pinned_wide, label, wide_bar))
            .expect("a wide row creates its heart control");
        assert!(
            !announced(&pinned, heart),
            "the narrow row never creates that control: {heart:?}"
        );
        assert!(
            text_rects(&pinned_wide, "Late Night Drive")
                .iter()
                .any(|label| label.top() >= wide_bar.top() - 0.01
                    && label.bottom() <= wide_bar.bottom() + 0.01),
            "a wide row keeps the name on the pin"
        );
    }
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn main_page_maps_show_to_the_show_view() {
        let mut library = Library::default();
        assert_eq!(main_page(&library), MainPage::Root);
        let _ = library.open(Page::Playlist("spotify:playlist:a".to_owned()));
        assert_eq!(main_page(&library), MainPage::Playlist);
        let _ = library.open(Page::Album("spotify:album:a".to_owned()));
        assert_eq!(main_page(&library), MainPage::Album);
        let _ = library.open(Page::Artist("spotify:artist:a".to_owned()));
        assert_eq!(main_page(&library), MainPage::Artist);
        let _ = library.open(Page::Show("spotify:show:a".to_owned()));
        assert_eq!(
            main_page(&library),
            MainPage::Show,
            "a show page has its own body now"
        );
        let _ = library.open(Page::Search("never gonna".to_owned()));
        assert_eq!(main_page(&library), MainPage::Search);
    }

    /// A query can spell a playlist's URI. The pane must still draw the results
    /// page and not that playlist's tracklist: the page variant decides.
    #[test]
    fn a_search_query_that_spells_a_playlist_uri_draws_the_results_page() {
        let mut library = Library::default();
        let _ = library.open(Page::Search("spotify:playlist:a".to_owned()));

        assert_eq!(main_page(&library), MainPage::Search);
    }
}

#[cfg(test)]
mod root_tests {
    use std::time::Instant;

    use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};
    use takofy_model::{CoverUrl, Episode, FollowedShow, PlaylistAccess, PlaylistEntry, Revision};

    use super::*;
    use crate::menu::{Anchor, Device, Kind, Listing, Request};
    use crate::message::{Failure, Update};
    use crate::theme::{extent, fonts, style};
    use crate::view::test_support::{click_events, secondary_click_events};

    const PROBE: Vec2 = Vec2::new(900.0, 800.0);
    const NOW_MS: u64 = 1_761_134_400_000;

    fn context() -> Context {
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&Tokens::DARK));
        ctx
    }

    fn input(events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, PROBE)),
            events,
            ..Default::default()
        }
    }

    fn run(
        ctx: &Context,
        events: Vec<Event>,
        library: &mut Library,
        art: &mut Art,
    ) -> egui::FullOutput {
        let tokens = Tokens::DARK;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut output = ctx.run_ui(input(events), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                library,
                &mut transport,
                &mut queue,
                art,
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });
        output.textures_delta.clear();
        output
    }

    fn frame(ctx: &Context, library: &mut Library, art: &mut Art) -> egui::FullOutput {
        run(ctx, Vec::new(), library, art)
    }

    fn drawn_texts(output: &egui::FullOutput) -> Vec<&str> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            })
            .collect()
    }

    fn text_rect(output: &egui::FullOutput, label: &str) -> Option<Rect> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                    Some(Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
    }

    fn playlist(uri: &str, name: &str) -> PlaylistEntry {
        PlaylistEntry {
            composed: Vec::new(),
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: None,
            access: None,
        }
    }

    fn show(uri: &str, name: &str) -> FollowedShow {
        FollowedShow {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            publisher: None,
            cover: None,
            episodes: None,
        }
    }

    fn library(playlists: Vec<PlaylistEntry>, shows: Vec<FollowedShow>) -> Library {
        let mut library = Library::default();
        library.apply(&Update::Playlists(playlists));
        library.apply(&Update::FollowedShows(shows));
        library
    }

    fn click(ctx: &Context, pos: Pos2, library: &mut Library, art: &mut Art) -> Option<Command> {
        let tokens = Tokens::DARK;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut command = None;
        for event in click_events(pos) {
            let command = &mut command;
            let library = &mut *library;
            let output = ctx.run_ui(input(vec![event]), |ui| {
                if let Some(next) = tracks_view(
                    ui,
                    Table::Full,
                    library,
                    &mut transport,
                    &mut queue,
                    art,
                    Instant::now(),
                    NOW_MS,
                    &tokens,
                ) {
                    *command = Some(next);
                }
            });
            let mut output = output;
            output.textures_delta.clear();
        }
        command
    }

    /// The rect a chip's label was painted into, plus its own padding.
    fn chip_rect(output: &egui::FullOutput, label: &str) -> Rect {
        let text = text_rect(output, &label.to_uppercase()).expect("the chip is drawn");
        text.expand2(Vec2::new(extent::CHIP_PAD_X, extent::CHIP_PAD_Y))
    }

    /// The rect the `index`-th card is drawn into, solved the way the view
    /// solves it: the head, the chip row, then the grid at the probe's width.
    fn card_rect_for(ctx: &Context, index: usize, count: usize) -> Rect {
        let tokens = Tokens::DARK;
        let mut found = None;
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                crate::view::inner(ui, |ui| {
                    page_head::show(ui, "Your Library", &library_note(count), &tokens);
                    let _ = filter_row::show_with(
                        ui,
                        &tokens,
                        &library_filter::ROW,
                        RootKind::All,
                        RootKind::control,
                        root_filter_label,
                        filter_row::RowSlot::Trailing(0.0),
                    );
                    let solved = media_grid(ui);
                    found = Some(solved.card_rect(index));
                });
            });
        });
        output.textures_delta.clear();
        found.expect("the root lays out")
    }

    fn followed_artist(uri: &str, name: &str) -> takofy_model::FollowedArtist {
        takofy_model::FollowedArtist {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            portrait: None,
            releases: None,
        }
    }

    fn saved_album(uri: &str, name: &str) -> takofy_model::SavedAlbum {
        takofy_model::SavedAlbum {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            artists: "Nils Aker".to_owned(),
            year: None,
            cover: None,
            tracks: None,
        }
    }

    /// Each of the four kinds knows the shelf it belongs to, so a menu built on
    /// it never has to read the uri back.
    #[test]
    fn media_of_gives_each_item_its_shelf() {
        let mut art = Art::default();
        let tokens = Tokens::DARK;
        let mut select = None;
        let mut open = None;
        let mut play = None;
        let library = Library::default();
        let mut draw = MediaDraw {
            art: &mut art,
            now: Instant::now(),
            tokens: &tokens,
            library: &library,
            select: &mut select,
            open: &mut open,
            play: &mut play,
        };
        let playlist = playlist("spotify:playlist:a", "A");
        let show = show("spotify:show:a", "A");
        let artist = followed_artist("spotify:artist:a", "A");
        let album = saved_album("spotify:album:a", "A");
        let cases = [
            (Item::Playlist(&playlist), SidebarKind::Playlists),
            (Item::Show(&show), SidebarKind::Podcasts),
            (Item::Artist(&artist), SidebarKind::Artists),
            (Item::Album(&album), SidebarKind::Albums),
        ];
        for (item, shelf) in cases {
            assert_eq!(draw.media_of(&item, MediaLayout::Grid).shelf, shelf);
        }
    }

    /// A rootlist, which a playlist row needs before it can be removed.
    fn rootlist(editable: bool) -> PlaylistAccess {
        PlaylistAccess {
            revision: Revision::from_wire(b"root-1").expect("a revision"),
            editable,
        }
    }

    /// The rect the `index`-th list row is drawn into, solved the way the view
    /// solves it: the head, the chips, then the rows.
    fn row_rect_for(ctx: &Context, index: usize) -> Rect {
        let tokens = Tokens::DARK;
        let mut found = None;
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            ui.add_space(space::S3);
            crate::view::inner(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        crate::view::inner(ui, |ui| {
                            page_head::show(ui, "Your Library", "", &tokens);
                            let _ = filter_row::show_with(
                                ui,
                                &tokens,
                                &library_filter::ROW,
                                RootKind::All,
                                RootKind::control,
                                root_filter_label,
                                filter_row::RowSlot::Trailing(view_toggle::reserved()),
                            );
                            let width = ui.available_width();
                            let height = space::S2 + media::head_height(ui) + space::S2;
                            let _ =
                                ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
                            let top = ui.cursor().min.y;
                            found = Some(Rect::from_min_size(
                                Pos2::new(ui.cursor().min.x, top + index as f32 * extent::ROW_H),
                                vec2(width, extent::ROW_H),
                            ));
                        });
                    });
            });
        });
        output.textures_delta.clear();
        found.expect("the root lays out")
    }

    /// One click's passes, and the request the frame posted for it.
    fn click_for_menu(
        ctx: &Context,
        events: [Event; 3],
        library: &mut Library,
        art: &mut Art,
    ) -> Option<Request> {
        let tokens = Tokens::DARK;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut request = None;
        for event in events {
            let library = &mut *library;
            let mut output = ctx.run_ui(input(vec![event]), |ui| {
                let _ = tracks_view(
                    ui,
                    Table::Full,
                    library,
                    &mut transport,
                    &mut queue,
                    art,
                    Instant::now(),
                    NOW_MS,
                    &tokens,
                );
            });
            output.textures_delta.clear();
            if let Some(posted) = context_menu::take_request(ctx) {
                request = Some(posted);
            }
        }
        request
    }

    /// The `⋯` of the root's one list row, pressed, and the request it posted.
    fn press_the_more(ctx: &Context, library: &mut Library, art: &mut Art) -> Option<Request> {
        let more = media::columns(row_rect_for(ctx, 0)).more.center();
        click_for_menu(ctx, click_events(more), library, art)
    }

    /// Presses the `⋯` on the root's one list row and checks the menu it posted.
    fn expect_row_menu(kind: SidebarKind, uri: &str, name: &str, library: &mut Library) {
        let ctx = context();
        let mut art = Art::default();
        library.layout = MediaLayout::List;
        let _ = frame(&ctx, library, &mut art);

        let request = press_the_more(&ctx, library, &mut art).expect("the `⋯` posted no request");
        assert_eq!(
            request.listing,
            Listing::Library(LibraryItem {
                kind,
                uri: uri.to_owned(),
                name: name.to_owned(),
            })
        );
        let labels: Vec<&str> = request
            .items
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(labels, vec!["Copy link", "Remove from library"]);
        assert!(
            !labels.contains(&"Rename"),
            "the root never offers a rename: {labels:?}"
        );
        assert_eq!(
            request.items.last().map(|item| item.kind),
            Some(Kind::Destructive),
            "the removal is the destructive row, drawn last"
        );
        assert_eq!(request.page, None, "the root posts no page");
        assert!(request.targets.is_empty() && request.rows.is_empty());
        assert!(
            matches!(request.door.anchor, Anchor::Button(_)),
            "the `⋯`'s own press hangs the menu off itself: {:?}",
            request.door.anchor
        );
        assert_eq!(request.door.device, Device::Pointer);
        assert!(library.selected().is_none(), "the press also selected");
        assert!(library.page().is_none(), "the press also opened a page");
    }

    #[test]
    fn a_playlist_row_more_posts_the_sidebar_rows_menu() {
        let mut library = library(vec![playlist("spotify:playlist:a", "Evening")], vec![]);
        library.root_filter.choose(RootKind::Playlists);
        library.apply(&Update::RootlistAccess(rootlist(true)));
        expect_row_menu(
            SidebarKind::Playlists,
            "spotify:playlist:a",
            "Evening",
            &mut library,
        );
    }

    #[test]
    fn a_podcast_row_more_posts_the_sidebar_rows_menu() {
        let mut library = library(vec![], vec![show("spotify:show:s", "Show One")]);
        library.root_filter.choose(RootKind::Podcasts);
        expect_row_menu(
            SidebarKind::Podcasts,
            "spotify:show:s",
            "Show One",
            &mut library,
        );
    }

    #[test]
    fn an_artist_row_more_posts_the_sidebar_rows_menu() {
        let mut library = library(vec![], vec![]);
        library.root_filter.choose(RootKind::Artists);
        library.apply(&Update::FollowedArtists(vec![followed_artist(
            "spotify:artist:a",
            "Nils Aker",
        )]));
        expect_row_menu(
            SidebarKind::Artists,
            "spotify:artist:a",
            "Nils Aker",
            &mut library,
        );
    }

    #[test]
    fn an_album_row_more_posts_the_sidebar_rows_menu() {
        let mut library = library(vec![], vec![]);
        library.root_filter.choose(RootKind::Albums);
        library.apply(&Update::SavedAlbums(vec![saved_album(
            "spotify:album:a",
            "Slow Tide",
        )]));
        expect_row_menu(
            SidebarKind::Albums,
            "spotify:album:a",
            "Slow Tide",
            &mut library,
        );
    }

    #[test]
    fn a_playlist_row_the_user_cannot_write_offers_only_its_link() {
        let mut library = library(vec![playlist("spotify:playlist:a", "Evening")], vec![]);
        library.root_filter.choose(RootKind::Playlists);
        library.apply(&Update::RootlistAccess(rootlist(false)));
        let ctx = context();
        let mut art = Art::default();
        library.layout = MediaLayout::List;
        let _ = frame(&ctx, &mut library, &mut art);

        let request =
            press_the_more(&ctx, &mut library, &mut art).expect("the `⋯` posted no request");
        let labels: Vec<&str> = request
            .items
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(
            labels,
            vec!["Copy link"],
            "a row that cannot be removed keeps its link"
        );
    }

    #[test]
    fn a_right_click_on_a_playlist_row_posts_the_same_request_at_the_pointer() {
        let mut library = library(vec![playlist("spotify:playlist:a", "Evening")], vec![]);
        library.root_filter.choose(RootKind::Playlists);
        library.apply(&Update::RootlistAccess(rootlist(true)));
        let ctx = context();
        let mut art = Art::default();
        library.layout = MediaLayout::List;
        let _ = frame(&ctx, &mut library, &mut art);

        let by_more =
            press_the_more(&ctx, &mut library, &mut art).expect("the `⋯` posted no request");
        let body = media::columns(row_rect_for(&ctx, 0)).body.center();
        let by_right = click_for_menu(&ctx, secondary_click_events(body), &mut library, &mut art)
            .expect("a right-click posted no request");

        assert_eq!(by_right.listing, by_more.listing);
        assert_eq!(by_right.items, by_more.items);
        assert_eq!(by_right.page, by_more.page);
        let Anchor::Pointer(at) = by_right.door.anchor else {
            panic!(
                "a right-click anchored to the `⋯`: {:?}",
                by_right.door.anchor
            );
        };
        assert!(
            (at - body).length() < 0.01,
            "a right-click at {body:?} opened at {at:?}"
        );
        assert_ne!(
            by_right.door.opener, by_more.door.opener,
            "the pointer door names the row, not the `⋯`"
        );
    }

    /// A tile has neither a row nor a `⋯`, so no press on it posts a menu: both
    /// buttons do what a tile does, which is activate.
    #[test]
    fn a_grid_tile_posts_no_menu_on_a_click_or_a_right_click() {
        let ctx = context();
        let mut art = Art::default();
        let gridded = || {
            let mut library = library(vec![playlist("spotify:playlist:a", "Evening")], vec![]);
            library.root_filter.choose(RootKind::Playlists);
            library.apply(&Update::RootlistAccess(rootlist(true)));
            library.layout = MediaLayout::Grid;
            library
        };

        // The tile the grid solved, and the name it painted inside it: the press
        // lands on a drawn tile and not on the gap a row's `⋯` would fill.
        let mut library = gridded();
        let before = frame(&ctx, &mut library, &mut art);
        let card = card_rect_for(&ctx, 0, 1);
        let name = text_rect(&before, "Evening").expect("the grid paints the tile's name");
        assert!(
            card.contains_rect(name),
            "the tile's name {name:?} sits outside its card {card:?}"
        );

        let command = click(&ctx, card.center(), &mut library, &mut art);
        assert_eq!(
            command,
            Some(Command::LoadTracks {
                playlist: "spotify:playlist:a".to_owned()
            }),
            "a tile press selects its playlist, as it always did"
        );
        assert_eq!(library.selected(), Some("spotify:playlist:a"));
        assert_eq!(
            context_menu::take_request(&ctx),
            None,
            "a tile press posted a row menu"
        );

        // The right-click is its own press on a fresh library, and it must leave
        // the selection alone.
        let mut library = gridded();
        let _ = frame(&ctx, &mut library, &mut art);
        let request = click_for_menu(
            &ctx,
            secondary_click_events(card.center()),
            &mut library,
            &mut art,
        );
        assert_eq!(request, None, "a right-click on a tile posted a row menu");
        assert!(
            library.selected().is_none() && library.page().is_none(),
            "a right-click on a tile activated it"
        );
    }

    #[test]
    fn idle_view_artists_ready_loading_failed_and_empty() {
        let mut root = library(vec![playlist("a", "A")], vec![]);
        root.root_filter.choose(RootKind::Artists);

        // Not asked for yet: loading, not the empty copy.
        assert_eq!(
            idle_view(&root),
            IdleView::Grid {
                items: Vec::new(),
                note: LoadNote::Loading(FollowedRead::Artists),
            }
        );

        root.apply(&Update::FollowedArtists(vec![
            followed_artist("spotify:artist:1", "One"),
            followed_artist("spotify:artist:2", "Two"),
        ]));
        let IdleView::Grid { items, note } = idle_view(&root) else {
            panic!("a ready artists list is a grid");
        };
        assert_eq!(items.len(), 2, "both artists draw as cards");
        assert!(matches!(items[0], Item::Artist(artist) if artist.uri.ends_with(":1")));
        assert_eq!(note, LoadNote::None);

        root.apply(&Update::FollowedArtistsFailed(Failure::FollowedArtists));
        assert_eq!(
            idle_view(&root),
            IdleView::Grid {
                items: Vec::new(),
                note: LoadNote::Failed(FollowedRead::Artists, Failure::FollowedArtists),
            }
        );

        let mut empty = library(vec![playlist("a", "A")], vec![]);
        empty.root_filter.choose(RootKind::Artists);
        empty.apply(&Update::FollowedArtists(Vec::new()));
        assert_eq!(
            idle_view(&empty),
            IdleView::Empty(RootKind::Artists),
            "nothing to show is the chip's own empty line"
        );
    }

    #[test]
    fn idle_view_albums_carry_their_own_load() {
        let mut library = library(vec![playlist("a", "A")], vec![]);
        library.root_filter.choose(RootKind::Albums);
        library.apply(&Update::SavedAlbums(vec![saved_album(
            "spotify:album:1",
            "Slow Tide",
        )]));

        let IdleView::Grid { items, note } = idle_view(&library) else {
            panic!("a ready albums list is a grid");
        };
        assert_eq!(items.len(), 1);
        assert_eq!(note, LoadNote::None);

        library.apply(&Update::SavedAlbumsFailed(Failure::SavedAlbums));
        let IdleView::Grid { note, .. } = idle_view(&library) else {
            panic!("a failed albums list keeps the grid");
        };
        assert_eq!(
            note,
            LoadNote::Failed(FollowedRead::Albums, Failure::SavedAlbums)
        );
    }

    /// The note's Retry asks the read it names, not the shows.
    #[test]
    fn the_artists_note_retries_the_artists() {
        let ctx = context();
        let mut library = library(vec![playlist("a", "A")], vec![]);
        library.root_filter.choose(RootKind::Artists);
        library.apply(&Update::FollowedArtistsFailed(Failure::FollowedArtists));
        let mut art = Art::default();

        let before = frame(&ctx, &mut library, &mut art);
        let retry = text_rect(&before, "Retry").expect("the note's retry");
        let command = click(&ctx, retry.center(), &mut library, &mut art);

        assert_eq!(command, Some(Command::LoadFollowedArtists));
        assert_eq!(library.followed_artists, Load::Loading);
    }

    #[test]
    fn a_show_card_click_opens_the_show_and_returns_load_show() {
        let ctx = context();
        let mut library = library(vec![playlist("a", "A")], vec![show("s", "Show One")]);
        let mut art = Art::default();
        let _ = frame(&ctx, &mut library, &mut art);

        let card = card_rect_for(&ctx, 1, 2);
        let command = click(&ctx, card.center(), &mut library, &mut art);

        assert_eq!(
            command,
            Some(Command::LoadShow {
                show: "s".to_owned()
            }),
            "a show card opens its page"
        );
        assert_eq!(library.page(), Some(&Page::Show("s".to_owned())));
    }

    #[test]
    fn a_playlist_card_click_still_selects() {
        let ctx = context();
        let mut library = library(vec![playlist("a", "A")], vec![show("s", "Show One")]);
        let mut art = Art::default();
        let _ = frame(&ctx, &mut library, &mut art);

        let card = card_rect_for(&ctx, 0, 2);
        let command = click(&ctx, card.center(), &mut library, &mut art);

        assert_eq!(
            command,
            Some(Command::LoadTracks {
                playlist: "a".to_owned()
            })
        );
    }

    #[test]
    fn choosing_podcasts_draws_only_show_cards() {
        let ctx = context();
        let mut library = library(vec![playlist("a", "List One")], vec![show("s", "Show One")]);
        let mut art = Art::default();

        let before = frame(&ctx, &mut library, &mut art);
        let chip = chip_rect(&before, "Podcasts");
        let _ = click(&ctx, chip.center(), &mut library, &mut art);

        let after = frame(&ctx, &mut library, &mut art);
        let texts = drawn_texts(&after);
        assert!(texts.contains(&"Show One"), "{texts:?}");
        assert!(
            !texts.contains(&"List One"),
            "the playlists card must be gone: {texts:?}"
        );
    }

    /// The two densities draw the same items and differ in what they add: the
    /// list has the columns' head and the type and size cells, the grid has
    /// neither.
    #[test]
    fn the_list_density_adds_the_head_and_the_two_cells() {
        let ctx = context();
        let mut library = library(vec![playlist("a", "Evening")], vec![]);
        library.layout = MediaLayout::List;
        let mut art = Art::default();

        let listed = frame(&ctx, &mut library, &mut art);
        let texts = drawn_texts(&listed);
        for heading in ["TITLE", "TYPE", "SIZE"] {
            assert!(
                texts.contains(&heading),
                "the list density draws the {heading} head: {texts:?}"
            );
        }
        assert!(
            texts.contains(&"PLAYLIST"),
            "a row carries its type: {texts:?}"
        );

        library.layout = MediaLayout::Grid;
        let gridded = frame(&ctx, &mut library, &mut art);
        let texts = drawn_texts(&gridded);
        assert!(!texts.contains(&"TITLE"), "the grid has no head: {texts:?}");
        assert!(
            texts.iter().any(|text| text.contains("Playlist")),
            "the tile still says what the thing is: {texts:?}"
        );
    }

    /// A press on the toggle moves the library's density, and the frame after it
    /// draws the other one.
    #[test]
    fn pressing_the_list_toggle_draws_rows_next_frame() {
        let ctx = context();
        let mut library = library(vec![playlist("a", "Evening")], vec![]);
        let mut art = Art::default();

        let tokens = Tokens::DARK;
        let before = frame(&ctx, &mut library, &mut art);
        assert!(
            !drawn_texts(&before).contains(&"TITLE"),
            "the grid has no head to press"
        );

        let grid_cell = pressed_layout_cell(&before, &tokens);
        let list_cell = Pos2::new(
            grid_cell.right() + extent::BUTTON / 2.0,
            grid_cell.center().y,
        );
        let _ = click(&ctx, list_cell, &mut library, &mut art);

        assert_eq!(
            library.layout,
            MediaLayout::List,
            "the press moved the density"
        );
        let after = frame(&ctx, &mut library, &mut art);
        assert!(
            drawn_texts(&after).contains(&"TITLE"),
            "the next frame draws the list"
        );
    }

    /// The pressed density cell, found by its own chrome: a button-sized square
    /// bordered in the text colour, which nothing else in the root draws.
    fn pressed_layout_cell(output: &egui::FullOutput, tokens: &Tokens) -> Rect {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if (shape.rect.width() - extent::BUTTON).abs() < 0.01
                        && (shape.stroke.width - crate::theme::stroke::HAIRLINE).abs() < 0.01
                        && shape.stroke.color == tokens.palette.text =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
            .expect("the chosen density is bordered in the text colour")
    }

    /// A press on a row does what the card did: the playlist is selected.
    #[test]
    fn a_click_on_a_row_selects_its_playlist() {
        let ctx = context();
        let mut library = library(vec![playlist("a", "Evening")], vec![]);
        library.layout = MediaLayout::List;
        let mut art = Art::default();

        let before = frame(&ctx, &mut library, &mut art);
        // The type cell exists only in the list density, so a click there proves
        // the row drew and took the press.
        let kind = text_rect(&before, "PLAYLIST").expect("the row paints its type");
        let command = click(&ctx, kind.center(), &mut library, &mut art);

        assert_eq!(
            command,
            Some(Command::LoadTracks {
                playlist: "a".to_owned()
            }),
            "a row selects its playlist, as the card did"
        );
        assert_eq!(library.selected(), Some("a"));
    }

    /// Pressing the Artists chip takes the choice, asks for the list once, and
    /// the next frame says it is loading rather than claiming the library is
    /// empty.
    #[test]
    fn the_artists_chip_asks_for_the_list_and_says_it_is_loading() {
        let ctx = context();
        let mut library = library(vec![playlist("a", "A")], vec![]);
        let mut art = Art::default();

        let before = frame(&ctx, &mut library, &mut art);
        let chip = chip_rect(&before, "Artists");
        let _ = click(&ctx, chip.center(), &mut library, &mut art);

        assert_eq!(library.root_filter.chosen(), RootKind::Artists);
        assert_eq!(
            library.followed_artists_request(),
            Some(Command::LoadFollowedArtists)
        );
        assert_eq!(library.followed_artists_request(), None, "asked once");

        let after = frame(&ctx, &mut library, &mut art);
        let texts = drawn_texts(&after);
        assert!(
            texts.contains(&"Loading your artists…"),
            "the root says what it is doing: {texts:?}"
        );
        assert!(
            !texts.contains(&"You don't follow any artists yet."),
            "and does not claim the list is empty: {texts:?}"
        );
    }

    #[test]
    fn the_followed_retry_returns_load_followed_shows() {
        let ctx = context();
        let mut library = library(vec![playlist("a", "A")], vec![]);
        library.apply(&Update::FollowedShowsFailed(Failure::Load));
        let mut art = Art::default();

        let before = frame(&ctx, &mut library, &mut art);
        let retry = text_rect(&before, "Retry").expect("the followed note's retry");
        let command = click(&ctx, retry.center(), &mut library, &mut art);

        assert_eq!(command, Some(Command::LoadFollowedShows));
    }

    #[test]
    fn the_page_head_note_counts_the_cards_shown() {
        let ctx = context();
        let mut library = library(
            vec![playlist("a", "A"), playlist("b", "B")],
            vec![show("s", "Show One")],
        );
        let mut art = Art::default();

        let output = frame(&ctx, &mut library, &mut art);
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&"3 SAVED"),
            "the note counts every card: {texts:?}"
        );

        library.root_filter.choose(RootKind::Podcasts);
        let output = frame(&ctx, &mut library, &mut art);
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&"1 SAVED"),
            "a narrowed chip counts only its own cards: {texts:?}"
        );
    }

    #[test]
    fn only_visible_cards_ask_for_covers() {
        let ctx = context();
        let shows: Vec<FollowedShow> = (0..40)
            .map(|index| FollowedShow {
                uri: format!("spotify:show:{index}"),
                name: format!("Show {index}"),
                named: true,
                publisher: None,
                cover: Some(
                    CoverUrl::from_wire(&format!("https://i.scdn.co/image/{index:0<40}"))
                        .expect("a cover url"),
                ),
                episodes: None,
            })
            .collect();
        let mut library = library(vec![], shows);
        let mut art = Art::default();
        let now = Instant::now();

        art.begin_frame();
        let _ = frame(&ctx, &mut library, &mut art);
        let requests = art.requests(now);
        assert!(!requests.is_empty(), "the visible cards must be fetched");
        assert!(
            requests.len() < 20,
            "only the cards the viewport shows are asked for: {}",
            requests.len()
        );
    }

    #[test]
    fn a_playlist_row_for_an_episode_draws_its_title_not_its_uri() {
        let ctx = context();
        let episode = Episode {
            uri: "spotify:episode:abc".to_owned(),
            title: "210: A dry labyrinth".to_owned(),
            show: "Lateral with Tom Scott".to_owned(),
            show_uri: Some("spotify:show:lateral".to_owned()),
            description: None,
            published: None,
            duration_ms: Some(2_773_461),
            cover: None,
            named: true,
        };
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![episode.entry()],
            cover: None,
            access: None,
        });
        let mut art = Art::default();

        let output = frame(&ctx, &mut library, &mut art);
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&"210: A dry labyrinth"),
            "the row names the episode: {texts:?}"
        );
        assert!(
            !texts
                .iter()
                .any(|text| text.starts_with("spotify:episode:")),
            "the raw URI must not be drawn: {texts:?}"
        );
    }

    #[test]
    fn a_press_on_a_cards_play_wants_the_play_and_selects_nothing() {
        let ctx = context();
        let mut art = Art::default();
        let mut library = library(vec![playlist("spotify:playlist:a", "Mix")], Vec::new());
        let card = card_rect_for(&ctx, 0, 1);

        let command = click(
            &ctx,
            media::tile_play_rect(card).center(),
            &mut library,
            &mut art,
        );

        assert_eq!(command, None, "a play press took the frame's command");
        assert_eq!(library.page(), None, "a play press opened the page");
        assert_eq!(library.selected(), None, "a play press selected the page");
        assert_eq!(
            library.play_request(),
            Some(Command::LoadTracks {
                playlist: "spotify:playlist:a".to_owned()
            }),
            "the press never became a want"
        );
    }

    #[test]
    fn a_press_on_a_rows_play_wants_the_play_and_opens_nothing() {
        let ctx = context();
        let mut art = Art::default();
        let mut library = library(vec![playlist("spotify:playlist:a", "Mix")], Vec::new());
        library.layout = MediaLayout::List;
        let row = row_rect_for(&ctx, 0);

        let command = click(
            &ctx,
            media::row_play_rect(row).center(),
            &mut library,
            &mut art,
        );

        assert_eq!(command, None, "a play press took the frame's command");
        assert_eq!(library.page(), None, "a play press opened the page");
        assert_eq!(library.selected(), None, "a play press selected the page");
        assert_eq!(
            library.play_request(),
            Some(Command::LoadTracks {
                playlist: "spotify:playlist:a".to_owned()
            }),
            "the press never became a want"
        );
    }

    #[test]
    fn a_failed_shelf_play_draws_the_load_sentence_under_the_chips() {
        let ctx = context();
        let mut art = Art::default();
        let mut library = library(vec![playlist("spotify:playlist:a", "Mix")], Vec::new());
        assert!(library.want_play("spotify:playlist:a", SidebarKind::Playlists));
        assert!(library.play_request().is_some());
        library.apply(&Update::TracksFailed {
            playlist: "spotify:playlist:a".to_owned(),
            failure: Failure::Load,
        });

        let output = frame(&ctx, &mut library, &mut art);
        let sentence = crate::signin::failure_message(Failure::Load);
        let line = text_rect(&output, sentence).expect("the failure sentence is drawn");
        let chips = text_rect(&output, "ALL").expect("the chips are drawn");
        let card = text_rect(&output, "Mix").expect("the card is drawn");

        assert!(
            line.top() >= chips.bottom(),
            "the sentence sits over the chips: {line:?} in {chips:?}"
        );
        assert!(
            line.bottom() <= card.top(),
            "the sentence sits under the cards: {line:?} in {card:?}"
        );
    }
}
