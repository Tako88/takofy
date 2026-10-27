//! The artist page's body: the plate hero, the shared action bar, Popular, and
//! the Discography. The hero is the band, its name and its shelf; the page draws
//! the row and owns the presses it reports.
//!
//! The page draws only what the library already has. `Load::Idle` has nothing to
//! say because the route asks for the artist when it opens, so it draws nothing
//! rather than a second spinner. A ready detail that does not name this page is
//! another artist's, so it draws the spinner instead of that artist's hero.
//!
//! The discography's tracklists are labelled only while their block is on
//! screen: the block's rect is solved before it is drawn, and only an on-screen
//! block asks the release store for its labels. The store's requests are drained
//! by the app after the draw, because a want cannot be dropped the way a command
//! can.
//!
//! The filter row hides a block entirely rather than drawing it shorter: a block
//! the chosen chip and the typed name do not match is skipped before its rect is
//! solved, so it reserves no space and asks for no labels and no cover. The row
//! is drawn for every artist with releases, because its field must exist even
//! when there is only one kind to chip between. A list the chip or the query
//! emptied says which of the two did it, after the body's clip is restored so a
//! released band cannot hide the sentence.

use std::time::Instant;

use egui::{Id, Rect, Sense, Ui, Vec2};
use takofy_model::{FollowedArtist, Release};

use crate::cover::{Art, Cover, want_px};
use crate::format::{discography_meta, format_position, library_kind, media_type};
use crate::layout::actionbar::Kind;
use crate::layout::sticky;
use crate::layout::{Table, table as table_layout};
use crate::library::{Library, Load};
use crate::library_filter::RootKind;
use crate::likes::Likes;
use crate::menu::LibraryItem;
use crate::message::Command;
use crate::player::Transport;
use crate::queue::Queue;
use crate::releases::Labels;
use crate::route::Page;
use crate::scroll;
use crate::sidebar_kind::SidebarKind;
use crate::theme::{Tokens, extent, space};
use crate::view::text as recipe;

use super::actionbar;
use super::artist_hero::{self, ArtistHero};
use super::carry::{self, Ghost};
use super::context_menu;
use super::discography_filter;
use super::heart;
use super::hero;
use super::media;
use super::pin;
use super::playlist::root::{grid_rect, media_grid};
use super::release_block::{self, Block};
use super::section;
use super::skeleton;
use super::track_list::{head, list_rect};
use super::track_row::{self, Click, Row, state_of};
use crate::verbs;

/// Draws the artist page and returns the command its controls asked for.
#[expect(
    clippy::too_many_arguments,
    reason = "the pane, its table, the state it draws and the frame's stores"
)]
pub fn artist_view(
    ui: &mut Ui,
    table: Table,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Option<Command> {
    // The page's own URI, so Retry asks for the artist this page is on rather
    // than whichever one happens to be in the load state.
    let Some(Page::Artist(uri)) = library.page() else {
        return None;
    };
    let uri = uri.clone();
    // The page the release blocks' menus are about, and whether a write is
    // holding the library still.
    let here = Page::Artist(uri.clone());
    let writing = library.write_in_flight();
    // The sidebar's removal of this artist is already in flight; a follow write
    // now would land in order with it and leave the row and the label apart.
    let removal_pending = library.saved_pending(&uri, false);

    let mut command = None;
    let mut retry = false;
    // A card press names an artist; opening it needs the library, which the page
    // has borrowed for the whole draw, so it waits for the scroll area to close.
    let mut open: Option<Page> = None;
    // A release's heart asks the library for the release, which the page has
    // borrowed; it is applied once the scroll area closes.
    // The release the heart named, its name, and the value it asked for.
    let mut save: Option<(String, String, bool)> = None;
    // Every release's saved state, read once: the blocks draw inside the borrow.
    let saved: Vec<(String, Option<bool>)> = library
        .current_artist()
        .map(|artist| {
            artist
                .releases
                .iter()
                .map(|release| (release.uri.clone(), library.is_saved(&release.uri)))
                .collect()
        })
        .unwrap_or_default();

    // The scroll offset is this artist's, not the album's or the playlist's that
    // share the pane.
    egui::ScrollArea::vertical()
        .id_salt(("artist", uri.as_str()))
        .show(ui, |ui| {
            scroll::follow_drag(ui);
            // The viewport's own top, read before anything shrinks the clip: the
            // action row shrinks it to its own bottom once it pins, and a band
            // under that row is measured from the viewport, not from the row.
            let viewport_top = ui.clip_rect().top();
            // The pane's full width, so the band's surface reaches both of its
            // edges even though the chips sit inside the page's gutter.
            let pane = ui.clip_rect();
            let page = library.artist_page();
            match page.load {
                Load::Idle => {}
                Load::Loading => {
                    // The rows hold the gutter Popular will draw in, so the
                    // title does not shift when the list lands.
                    super::inner(ui, |ui| {
                        skeleton::rows(
                            ui,
                            skeleton::SCREENFUL,
                            table.columns().for_plays_album(),
                            true,
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
                    // A ready detail that does not name this page is another
                    // artist's: the page keeps its loading state rather than
                    // painting that artist's hero and discography.
                    let Some(artist) = page.current else {
                        // The gutter is the loaded list's, so nothing shifts
                        // when this page's own artist arrives.
                        super::inner(ui, |ui| {
                            skeleton::rows(
                                ui,
                                skeleton::SCREENFUL,
                                table.columns().for_plays_album(),
                                true,
                                tokens,
                            );
                        });
                        return;
                    };

                    // The hero holds the plate at its own size and draws the
                    // wash from the same pixels.
                    let hero = ArtistHero {
                        name: &artist.name,
                        portrait: artist.portrait.as_ref(),
                        releases: &artist.releases,
                    };
                    artist_hero::show(ui, &hero, art, now, tokens);

                    // The action row is the shared one, so it pins while the
                    // discography scrolls under it. The artist's difference is a
                    // word where the heart would be: following is a write to a
                    // list, not a saved state, and the reference gives the artist
                    // no like at all.
                    let acted = actionbar::show(
                        ui,
                        &actionbar::Bar {
                            title: Some(&artist.name),
                            playable: !artist.top_tracks.is_empty(),
                            shuffle: true,
                            more: Some(true),
                            saved: None,
                            kind: Kind::Other,
                            text: Some(actionbar::Text {
                                label: page.follow.label(),
                                live: page.follow.pressable() && !removal_pending,
                                failed: page.follow.failed(),
                            }),
                        },
                        None,
                        tokens,
                    );
                    if acted.followed && !removal_pending {
                        command = page.follow.pressed();
                    }
                    if acted.played {
                        command = queue.start(
                            &artist.top_tracks,
                            0,
                            Some(artist.uri.as_str()),
                            transport,
                        );
                    }
                    if acted.shuffled {
                        command = (queue.start_with_shuffle(
                            &artist.top_tracks,
                            0,
                            Some(artist.uri.as_str()),
                            transport,
                        ))
                        .or(command.take());
                    }
                    if let Some(door) = acted.more_door {
                        let item = LibraryItem {
                            kind: SidebarKind::Artists,
                            uri: uri.clone(),
                            name: artist.name.clone(),
                        };
                        context_menu::post(
                            ui.ctx(),
                            verbs::artist_request(
                                item,
                                door,
                                page.follow.confirmed() != Some(true),
                                Some(here.clone()),
                            ),
                        );
                    }

                    super::inner(ui, |ui| {
                        let width = ui.available_width();
                        popular(
                            ui,
                            table,
                            width,
                            artist,
                            transport,
                            queue,
                            art,
                            now,
                            tokens,
                            &mut command,
                            &mut open,
                            writing,
                            page.likes,
                        );

                        let tracks: usize = artist
                            .releases
                            .iter()
                            .map(|release| release.tracks.len())
                            .sum();
                        section::head(
                            ui,
                            "Discography",
                            &discography_meta(artist.releases.len(), tracks),
                            tokens,
                        );
                        if artist.releases.is_empty() {
                            ui.label(recipe::muted("No releases.", tokens));
                        } else {
                            // The row the artist's own kinds allow, and the chip it
                            // is filtered by. The effective chip, never the raw
                            // choice: a choice whose kind this artist does not have
                            // would otherwise hide every block.
                            let row = crate::discography::chips(&artist.releases);
                            let active = page.filter.effective(&row);
                            // Everything the chip and the frame's starting query
                            // keep, and the height it takes, solved before any block
                            // is drawn: the band's end has to be exact while the
                            // band is still in the flow. A keystroke resolves below
                            // and asks for the frame that makes the end agree.
                            let pre_needle = page.filter.needle();
                            let pre_hits = match &pre_needle {
                                Some(needle) => {
                                    crate::discography::narrow(&artist.releases, active, needle)
                                }
                                None => crate::discography::narrow(&artist.releases, active, ""),
                            };
                            let heights: Vec<f32> = pre_hits
                                .iter()
                                .map(|&index| {
                                    release_block::height(artist.releases[index].tracks.len())
                                })
                                .collect();
                            let body_h =
                                sticky::stack_height(&heights, ui.spacing().item_spacing.y);

                            let shown = discography_filter::show(
                                ui,
                                tokens,
                                discography_filter::Row {
                                    chips: &row,
                                    active,
                                    body_h,
                                    viewport_top,
                                    pane,
                                    releases: &artist.releases,
                                    filter: page.filter,
                                },
                            );
                            if let Some(picked) = shown.picked {
                                page.filter.choose(picked);
                            }

                            // The rows slide under the band, so the body is drawn
                            // through the clip the band's own state decides: below
                            // a held band, and above one riding the section's end.
                            // The clip goes back before the empty state and the
                            // likeness row, which are not part of the filtered body.
                            let clip = ui.clip_rect();
                            ui.set_clip_rect(sticky::body_clip(clip, shown.band, shown.hold));

                            for &index in &shown.hits {
                                let release = &artist.releases[index];
                                let on_screen = table_layout::on_screen(
                                    ui.clip_rect(),
                                    block_rect(ui, release),
                                );
                                if !on_screen {
                                    let block = Block {
                                        release,
                                        labels: Labels::Idle,
                                        cover: Cover::Missing,
                                        on_screen: false,
                                        playable: false,
                                        here: Some(&here),
                                        writing,
                                        likes: page.likes,
                                        saved: saved_of(&saved, &release.uri),
                                    };
                                    let _ =
                                        release_block::show(ui, table, &block, transport, tokens);
                                    continue;
                                }
                                let labels = page.releases.look(&release.uri);
                                let cover = art.look(
                                    release.cover.as_ref(),
                                    want_px(extent::RELEASE_ART, ui.pixels_per_point()),
                                    now,
                                );
                                let block = Block {
                                    release,
                                    labels,
                                    cover,
                                    on_screen: true,
                                    playable: matches!(labels, Labels::Ready(_)),
                                    here: Some(&here),
                                    writing,
                                    likes: page.likes,
                                    saved: saved_of(&saved, &release.uri),
                                };
                                let clicked =
                                    release_block::show(ui, table, &block, transport, tokens);
                                // The heading names its album; the page applies the
                                // open once the frame is done.
                                open = open.take().or(clicked.open.clone());
                                let ready = match labels {
                                    Labels::Ready(tracks) => Some(tracks),
                                    Labels::Idle | Labels::Loading | Labels::Failed(_) => None,
                                };
                                if clicked.played
                                    && let Some(tracks) = ready
                                {
                                    command = queue.start(
                                        tracks,
                                        0,
                                        Some(release.uri.as_str()),
                                        transport,
                                    );
                                }
                                if let (Some(index), Some(tracks)) = (clicked.row, ready) {
                                    command = queue.start(
                                        tracks,
                                        index,
                                        Some(release.uri.as_str()),
                                        transport,
                                    );
                                }
                                if let Some(asked) = clicked.saved {
                                    save = Some((release.uri.clone(), release.name.clone(), asked));
                                }
                                if let Some(door) = clicked.more {
                                    let item = LibraryItem {
                                        kind: SidebarKind::Albums,
                                        uri: release.uri.clone(),
                                        name: release.name.clone(),
                                    };
                                    // A release is an album, so its menu is an
                                    // album's: the library write and the link.
                                    let rights = verbs::LibraryRights {
                                        renamable: false,
                                        removable: saved_of(&saved, &release.uri) == Some(true),
                                        saved: saved_of(&saved, &release.uri),
                                    };
                                    context_menu::post(
                                        ui.ctx(),
                                        verbs::library_request(
                                            item,
                                            door,
                                            rights,
                                            Some(here.clone()),
                                            crate::naming::Surface::Band,
                                        ),
                                    );
                                }
                            }
                            ui.set_clip_rect(clip);

                            // After the clip, because a released band ends the body
                            // at its own top: a sentence inside the body's clip is
                            // invisible exactly when an empty result needs it.
                            if let Some(emptied) =
                                crate::discography::emptied(shown.hits.len(), shown.queried)
                            {
                                let query = page.filter.edit().trim().to_owned();
                                draw_empty(ui, emptied, &artist.name, &query, tokens);
                            }
                        }

                        if let Some(artist) = fans(ui, page.related, art, now, tokens) {
                            open = Some(Page::Artist(artist));
                        }
                    });
                }
            }
        });

    if retry {
        return library.open(Page::Artist(uri));
    }
    // A frame reports one command, so a same-frame command wins and the open is dropped.
    if let Some(page) = open
        && command.is_none()
    {
        command = library.open(page);
    }
    if let Some((uri, name, asked)) = save
        && command.is_none()
    {
        let item = LibraryItem {
            kind: SidebarKind::Albums,
            uri,
            name,
        };
        command = library.set_saved(&item, asked);
    }
    command
}

/// The sentence an emptied discography draws under its row.
///
/// The query wins whenever one is typed, and the format's own state is only ever
/// shown with no query. Both are drawn after the body's clip is restored so a
/// released band cannot hide the only thing the page has to say.
pub(super) fn draw_empty(
    ui: &mut Ui,
    emptied: crate::discography::Emptied,
    artist: &str,
    query: &str,
    tokens: &Tokens,
) {
    use crate::discography::Emptied;
    match emptied {
        Emptied::Query => {
            let line = crate::format::list_nothing(query);
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(0, space::S3 as i8))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(line)
                            .font(egui::FontId::new(
                                crate::theme::text::SM,
                                crate::theme::fonts::sans(crate::theme::Weight::Regular),
                            ))
                            .color(tokens.palette.text_subtle),
                    );
                });
        }
        Emptied::Format => {
            let (title, body) = crate::format::format_empty_copy(artist);
            ui.add_space(space::S7);
            ui.label(
                egui::RichText::new(title)
                    .font(egui::FontId::new(
                        crate::theme::text::MD,
                        crate::theme::fonts::sans(crate::theme::Weight::Semibold),
                    ))
                    .color(tokens.palette.text),
            );
            ui.add_space(space::S2);
            ui.label(
                egui::RichText::new(body)
                    .font(egui::FontId::new(
                        crate::theme::text::BASE,
                        crate::theme::fonts::sans(crate::theme::Weight::Regular),
                    ))
                    .color(tokens.palette.text_muted),
            );
            ui.add_space(space::S7);
        }
    }
}

/// One release's saved state, as the page read it before the draw.
fn saved_of(saved: &[(String, Option<bool>)], uri: &str) -> Option<bool> {
    saved
        .iter()
        .find(|(release, _)| release == uri)
        .and_then(|(_, saved)| *saved)
}

/// The likeness row: the artists this one is heard with, as round cards.
///
/// The row is the page's second read, so it is absent until the cards land and
/// absent for good when the artist names nobody. A press opens that artist.
fn fans(
    ui: &mut Ui,
    related: &Load<Vec<FollowedArtist>>,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Option<String> {
    let cards = match related {
        Load::Ready(cards) if !cards.is_empty() => cards,
        Load::Idle | Load::Loading | Load::Ready(_) | Load::Failed(_) => return None,
    };

    ui.add_space(space::S6);
    section::head(ui, "Fans also like", &cards.len().to_string(), tokens);

    let solved = media_grid(ui);
    let list = grid_rect(ui, &solved, cards.len());
    ui.allocate_rect(list, egui::Sense::hover());
    let visible = ui.clip_rect();

    let want = want_px(solved.card_w, ui.pixels_per_point());
    // An artist's subline is its kind alone: the wire carries no listener count,
    // which is what the reference puts there.
    let sub = library_kind(media_type(RootKind::Artists), None);
    let mut pressed = None;
    for (index, card) in cards.iter().enumerate() {
        let rect = solved.card_rect(index);
        if !table_layout::on_screen(visible, rect) {
            continue;
        }
        let media = media::Media {
            uri: &card.uri,
            name: &card.name,
            kind: media_type(RootKind::Artists),
            sub: &sub,
            size: "",
            round: true,
            shelf: SidebarKind::Artists,
            playable: false,
            cover: art.look(card.portrait.as_ref(), want, now),
            picture: hero::Picture::Cover,
        };
        if media::tile(ui, rect, &media, tokens) == Some(media::Press::Open) {
            pressed = Some(card.uri.clone());
        }
    }
    pressed
}

/// The Popular section: the artist's top tracks as a plays table.
///
/// No `Plays` column: the wire carries no play counts. A row is the plays shape,
/// numbered by its place in the list, carrying its own art and the album it is
/// on in the credit column — this page already says the artist, so the record is
/// what the column is worth. A double click starts the whole top list from here.
#[expect(
    clippy::too_many_arguments,
    reason = "the section needs its table, width, state, the frame's stores and the write gate"
)]
fn popular(
    ui: &mut Ui,
    table: Table,
    width: f32,
    artist: &takofy_model::ArtistDetail,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
    command: &mut Option<Command>,
    open: &mut Option<Page>,
    writing: bool,
    likes: &Likes,
) {
    // The section's rows are on this artist's page, so a menu opened on one of
    // them knows which page it is about.
    let here = Page::Artist(artist.uri.clone());
    section::head(ui, "Popular", "", tokens);
    if artist.top_tracks.is_empty() {
        ui.label(recipe::muted("No popular tracks.", tokens));
        return;
    }

    let columns = table_layout::columns(width, table.columns().for_plays_album());
    head(ui, columns, tokens);
    let count = artist.top_tracks.len();
    let list = list_rect(ui, count);
    ui.allocate_rect(list, Sense::hover());
    let visible = ui.clip_rect();
    let mut selection = carry::List::open(
        ui.ctx(),
        Id::unique(("artist-popular", artist.uri.as_str())),
        count,
    );
    let gate = selection.gate(writing);
    // The id the rows draw under, so a focused row is read from the same `Ui`
    // that drew them.
    let focused_now = ui.memory(|memory| memory.focused());
    let mut reveal = None;

    for (index, entry) in artist.top_tracks.iter().enumerate() {
        let rect = table_layout::row_rect(list, index, extent::ROW_H);
        if !table_layout::on_screen(visible, rect) {
            // A row the cull skipped draws nothing, so it has no reveal of its
            // own; one that holds the keyboard is asked for once the loop is
            // done.
            if focused_now == Some(track_row::id(ui, &entry.uri, index + 1)) {
                reveal = Some(rect);
            }
            continue;
        }
        let duration = entry.duration_ms.map(format_position);
        let row = Row {
            uri: &entry.uri,
            number: index + 1,
            title: &entry.title,
            artist: "",
            artist_uri: None,
            album: entry.album.as_deref(),
            album_uri: entry.album_uri.as_deref(),
            added: None,
            duration: duration.as_deref(),
            state: state_of(&entry.uri, transport),
            // Built inside the cull, so a row off screen asks for no art.
            cover: Some(art.look(
                entry.cover.as_ref(),
                want_px(extent::LIBRARY_ART, ui.pixels_per_point()),
                now,
            )),
            look: selection.look(index),
            menu: gate.allows(),
            liked: likes.state(&entry.uri),
        };
        match track_row::show(ui, rect, columns, &row, tokens) {
            Some(Click::Play) => {
                *command = queue.start(
                    &artist.top_tracks,
                    index,
                    Some(artist.uri.as_str()),
                    transport,
                );
            }
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
                    likes,
                },
                |row| artist.top_tracks.get(row).cloned(),
            ),
            Some(Click::Lift) => {
                let ghost = Ghost::Table(row.lifted(columns));
                selection.lift(index, rect.size(), ghost, |row| {
                    artist.top_tracks.get(row).cloned()
                });
            }
            Some(Click::Open(page)) => *open = Some(page),
            None => {}
        }
    }
    // A focused row the cull skipped is asked for again through the seam, which
    // shifts the request clear of the action row pinned above the list; the
    // row's own reveal knows nothing of it and would leave the row behind it.
    if let Some(row) = reveal
        && row.top() < ui.clip_rect().top()
    {
        pin::reveal(ui, row);
    }
    selection.finish(ui.ctx());
}

/// The rect a release block will occupy, solved before it is drawn so its
/// visibility decides whether its labels are wanted.
fn block_rect(ui: &Ui, release: &Release) -> Rect {
    let top = ui.cursor().min;
    let height = release_block::height(release.tracks.len());
    Rect::from_min_size(top, Vec2::new(ui.available_width(), height))
}

#[cfg(test)]
mod tests;
