//! The artist page's body: the photograph hero, the shared action bar, Popular,
//! and the Discography. The hero is only the band and the name; the page draws
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

use egui::{Id, Rect, Sense, Ui, Vec2, vec2};
use takofy_model::{FollowedArtist, Release};

use crate::cover::{Art, Cover, want_px};
use crate::format::{discography_meta, format_position};
use crate::layout::actionbar::Kind;
use crate::layout::sticky;
use crate::layout::{Table, cards, table as table_layout};
use crate::library::{Library, Load};
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
                    page.filter.clear_query();
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

                    let hero = ArtistHero {
                        name: &artist.name,
                        portrait: art.look(
                            artist.portrait.as_ref(),
                            want_px(ui.available_width(), ui.pixels_per_point()),
                            now,
                        ),
                    };
                    artist_hero::show(ui, &hero, tokens);

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
                            page.filter.clear_query();
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

    let solved = cards::grid(
        Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 0.0)),
        extent::MEDIA_MIN_W,
        space::S3,
        media::extra_height(ui),
    );
    let rows = solved.rows(cards.len());
    let height =
        rows as f32 * solved.card_rect(0).height() + (rows.saturating_sub(1)) as f32 * space::S3;
    let list = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), height));
    ui.allocate_rect(list, egui::Sense::hover());
    let visible = ui.clip_rect();

    let want = want_px(solved.card_w, ui.pixels_per_point());
    let mut pressed = None;
    for (index, card) in cards.iter().enumerate() {
        let rect = solved.card_rect(index);
        if !table_layout::on_screen(visible, rect) {
            continue;
        }
        let media = media::Media {
            uri: &card.uri,
            name: &card.name,
            kind: "Artist",
            sub: "Artist",
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
mod tests {
    use egui::{Context, Event, Pos2, RawInput, Rect, Vec2, pos2, vec2};
    use takofy_model::{
        ArtistDetail, CoverSet, CoverUrl, Release, Release as _Release, ReleaseKind, TrackEntry,
    };

    use super::*;
    use crate::cover::Cover;
    use crate::discography::{Chip, chips};
    use crate::library::Library;
    use crate::message::{Failure, Update};
    use crate::releases::Labels;
    use crate::theme::{Tokens, extent, fonts, hero, space, stroke, style};
    use crate::view::release_block::Block;
    use crate::view::test_support::click_events;

    const ARTIST: &str = "spotify:artist:a";
    /// The artist a stale ready detail names while the page is on [`ARTIST`].
    const OTHER_ARTIST: &str = "spotify:artist:b";

    fn context() -> Context {
        let tokens = Tokens::DARK;
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));
        ctx
    }

    fn input(size: Vec2, events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            events,
            ..Default::default()
        }
    }

    fn run(ctx: &Context, input: RawInput, draw: impl FnMut(&mut egui::Ui)) -> egui::FullOutput {
        let mut output = ctx.run_ui(input, draw);
        output.textures_delta.clear();
        output
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

    fn track(uri: &str, title: &str, artists: &str, duration_ms: u32) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: title.to_owned(),
            artists: artists.to_owned(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            artist_names: Vec::new(),
            added_at: None,
            duration_ms: Some(duration_ms),
            named: true,
            cover: None,
        }
    }

    fn release(uri: &str, name: &str, tracks: &[&str]) -> Release {
        release_of_kind(uri, name, ReleaseKind::Album, tracks)
    }

    fn release_of_kind(uri: &str, name: &str, kind: ReleaseKind, tracks: &[&str]) -> Release {
        Release {
            uri: uri.to_owned(),
            name: name.to_owned(),
            kind,
            year: Some(2024),
            cover: None,
            tracks: tracks.iter().map(|track| (*track).to_owned()).collect(),
        }
    }

    /// The rect a chip occupies, recovered from the label it painted: a chip's
    /// label sits at the chip's top-left plus its padding.
    fn chip_rect(output: &egui::FullOutput, chip: Chip) -> Option<Rect> {
        let label = crate::format::filter_label(chip).to_uppercase();
        let text = text_rect(output, &label)?;
        Some(text.expand2(Vec2::new(extent::CHIP_PAD_X, extent::CHIP_PAD_Y)))
    }

    /// The pane every block of the three-release fixture fits in.
    fn tall() -> Vec2 {
        Vec2::new(900.0, 1500.0)
    }

    /// Two albums and one EP: two chips beside `All`, and three blocks.
    fn mixed() -> Vec<Release> {
        vec![
            release_of_kind(
                "spotify:album:r1",
                "Album One",
                ReleaseKind::Album,
                &["spotify:track:1"],
            ),
            release_of_kind(
                "spotify:ep:r2",
                "An EP",
                ReleaseKind::Ep,
                &["spotify:track:2"],
            ),
            release_of_kind(
                "spotify:album:r3",
                "Album Two",
                ReleaseKind::Album,
                &["spotify:track:3"],
            ),
        ]
    }

    /// One release of every kind, so the row has all five chips.
    fn every_kind() -> Vec<Release> {
        vec![
            release_of_kind("spotify:a", "An Album", ReleaseKind::Album, &["t1"]),
            release_of_kind("spotify:b", "An EP", ReleaseKind::Ep, &["t2"]),
            release_of_kind("spotify:c", "A Single", ReleaseKind::Single, &["t3"]),
            release_of_kind(
                "spotify:d",
                "A Compilation",
                ReleaseKind::Compilation,
                &["t4"],
            ),
        ]
    }

    /// The sizes the five chips need, as the row measures them.
    fn chip_sizes(ctx: &Context) -> Vec<Vec2> {
        let tokens = Tokens::DARK;
        let row = chips(&every_kind());
        let mut sizes = Vec::new();
        run(ctx, input(Vec2::new(900.0, 800.0), Vec::new()), |ui| {
            sizes = row
                .iter()
                .map(|chip| {
                    crate::view::controls::chip_size(
                        ui,
                        &tokens,
                        crate::format::filter_label(*chip),
                    )
                })
                .collect();
        });
        sizes
    }

    fn drawn(output: &egui::FullOutput, name: &str) -> bool {
        drawn_texts(output).contains(&name)
    }

    fn cover_url(tail: &str) -> CoverUrl {
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
    }

    fn cover(tail: &str) -> CoverSet {
        CoverSet::single(cover_url(tail))
    }

    const PAGE: Vec2 = Vec2::new(900.0, 1500.0);

    fn detail(top: Vec<TrackEntry>, releases: Vec<Release>) -> ArtistDetail {
        ArtistDetail {
            uri: ARTIST.to_owned(),
            name: "Vela Moreno".to_owned(),
            portrait: None,
            top_tracks: top,
            releases,
            related: Vec::new(),
        }
    }

    #[test]
    fn a_right_click_posts_the_popular_menu() {
        let ctx = context();
        let mut library = library_with(detail(
            vec![
                track("spotify:track:t0", "One", "Vela", 200_000),
                track("spotify:track:t1", "Two", "Vela", 210_000),
            ],
            Vec::new(),
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let (_, output) = frame(
            &ctx,
            PAGE,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let at = text_rect(&output, "Two")
            .expect("the second popular row is painted")
            .center();

        for event in crate::view::test_support::secondary_click_events(at) {
            let _ = frame(
                &ctx,
                PAGE,
                vec![event],
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
        }
        let request = crate::view::context_menu::take_request(&ctx)
            .expect("a right-click on a popular row posted a request");
        assert_eq!(request.targets.len(), 1);
        assert_eq!(request.targets[0].uri, "spotify:track:t1");
        assert_eq!(request.page, Some(Page::Artist(ARTIST.to_owned())));
        assert!(
            !request
                .items
                .iter()
                .any(|item| item.verb == crate::menu::Verb::GoToArtist),
            "the artist this page already is is not one of its verbs"
        );
    }

    /// The likeness row draws under the discography, and a press opens that
    /// artist's page.
    #[test]
    fn the_likeness_row_draws_its_cards_and_a_press_opens_the_artist() {
        let ctx = context();
        let mut detail = detail(
            vec![track("spotify:track:t", "Top", "Vela", 200_000)],
            vec![release(
                "spotify:album:a",
                "Album One",
                &["spotify:track:t"],
            )],
        );
        detail.related = vec!["spotify:artist:other".to_owned()];
        let mut library = library_with(detail);
        // The cards are the page's second read, so the request comes first.
        let _ = library.related_request();
        library.apply(&Update::Related {
            artist: ARTIST.to_owned(),
            cards: vec![FollowedArtist {
                uri: "spotify:artist:other".to_owned(),
                name: "Other Artist".to_owned(),
                named: true,
                portrait: None,
                releases: None,
            }],
        });
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            PAGE,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts: Vec<&str> = drawn_texts(&output);
        assert!(texts.contains(&"Fans also like"), "{texts:?}");
        assert!(texts.contains(&"Other Artist"), "{texts:?}");

        let card = text_rect(&output, "Other Artist").expect("the card's name");
        let (command, _) = frame(
            &ctx,
            PAGE,
            crate::view::test_support::click_events(card.center()).to_vec(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(
            command,
            Some(Command::LoadArtist {
                artist: "spotify:artist:other".to_owned()
            }),
            "a card opens that artist"
        );
    }

    /// An artist the wire names nobody for draws no section at all, and one with
    /// no releases still draws the row after its empty copy.
    #[test]
    fn the_likeness_row_is_absent_without_cards_and_survives_an_empty_discography() {
        let ctx = context();
        let mut bare = detail(vec![], vec![]);
        bare.related = vec!["spotify:artist:other".to_owned()];
        let mut library = library_with(bare);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            PAGE,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(!drawn_texts(&output).contains(&"Fans also like"));

        let _ = library.related_request();
        library.apply(&Update::Related {
            artist: ARTIST.to_owned(),
            cards: vec![FollowedArtist {
                uri: "spotify:artist:other".to_owned(),
                name: "Other Artist".to_owned(),
                named: true,
                portrait: None,
                releases: None,
            }],
        });
        let (_, output) = frame(
            &ctx,
            PAGE,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts = drawn_texts(&output);
        assert!(texts.contains(&"No releases."), "{texts:?}");
        assert!(
            texts.contains(&"Fans also like"),
            "the row survives an empty discography: {texts:?}"
        );
    }

    fn library_with(detail: ArtistDetail) -> Library {
        let mut library = Library::default();
        library.open(Page::Artist(detail.uri.clone()));
        library.apply(&Update::Artist(detail));
        library
    }

    fn frame(
        ctx: &Context,
        size: Vec2,
        events: Vec<Event>,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
        art: &mut Art,
    ) -> (Option<Command>, egui::FullOutput) {
        let tokens = Tokens::DARK;
        let mut command = None;
        let output = run(ctx, input(size, events), |ui| {
            // A notch lands in the frame it arrives, as the window's own draw
            // pass makes it: egui's spread of one notch over several frames
            // would still be moving the page while a probe reads a row's place.
            crate::scroll::instant(ui.ctx());
            ui.add_space(space::S3);
            if let Some(next) = artist_view(
                ui,
                Table::Full,
                library,
                transport,
                queue,
                art,
                Instant::now(),
                &tokens,
            ) {
                command = Some(next);
            }
        });
        (command, output)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "one test click needs the frame's whole state"
    )]
    fn click(
        ctx: &Context,
        size: Vec2,
        pos: Pos2,
        times: usize,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
        art: &mut Art,
    ) -> Option<Command> {
        let mut last = None;
        for _ in 0..times {
            for event in click_events(pos) {
                last = frame(ctx, size, vec![event], library, transport, queue, art).0;
            }
        }
        last
    }

    /// The size the artist's word needs, as the page measures it.
    fn word(ctx: &Context, label: &str) -> Vec2 {
        let tokens = Tokens::DARK;
        let mut size = Vec2::ZERO;
        run(ctx, input(PAGE, Vec::new()), |ui| {
            size = crate::view::controls::text_button_size(ui, &tokens, label);
        });
        size
    }

    /// The action row's parts on this page: the tools and the word.
    fn bar_parts(ctx: &Context, label: &str) -> crate::layout::actionbar::Parts {
        crate::layout::actionbar::Parts {
            shuffle: true,
            field: false,
            follow: Some(word(ctx, label)),
        }
    }

    /// The row's slot in the page's flow, solved where the page puts it: the
    /// hero's band first, then the shared row.
    fn bar_row(ctx: &Context, size: Vec2) -> Rect {
        let mut found = None;
        run(ctx, input(size, Vec::new()), |ui| {
            ui.add_space(space::S3);
            let _ = ui.allocate_exact_size(
                vec2(ui.available_width(), hero::ARTIST_MIN_H),
                Sense::hover(),
            );
            found = Some(actionbar::row_rect(ui));
        });
        found.expect("the band and its action row lay out")
    }

    /// The bar's play tile and its text control, solved by the row's own solver.
    fn bar_tiles(ctx: &Context, size: Vec2, label: &str) -> (Rect, Rect) {
        let placed =
            crate::layout::actionbar::solve(bar_row(ctx, size), Kind::Other, bar_parts(ctx, label));
        (
            placed.play,
            placed.follow.expect("the artist's bar has a text slot"),
        )
    }

    /// The row a pinned page draws, recovered from the fill it paints: the same
    /// box the page's children are solved against.
    fn bar_rect(output: &egui::FullOutput) -> Option<Rect> {
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

    /// One frame with the wheel moved by `points` at the foot of the page, which
    /// is how the artist page scrolls.
    fn wheel(
        ctx: &Context,
        size: Vec2,
        points: f32,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
        art: &mut Art,
    ) -> egui::FullOutput {
        let at = Pos2::new(size.x / 2.0, size.y - extent::ROW_H);
        frame(
            ctx,
            size,
            vec![
                Event::PointerMoved(at),
                Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0.0, points),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            library,
            transport,
            queue,
            art,
        )
        .1
    }

    /// The cells the Popular table's `index`-th row occupies, solved the way the
    /// page solves them: the title, the artist and the row itself.
    fn popular_cells(ctx: &Context, size: Vec2, index: usize, count: usize) -> (Rect, Rect, Rect) {
        let tokens = Tokens::DARK;
        let mut found = None;
        run(ctx, input(size, Vec::new()), |ui| {
            ui.add_space(space::S3);
            let _ = ui.allocate_exact_size(
                vec2(ui.available_width(), hero::ARTIST_MIN_H),
                Sense::hover(),
            );
            actionbar::row_rect(ui);
            crate::view::inner(ui, |ui| {
                section::head(ui, "Popular", "", &tokens);
                let columns = table_layout::columns(
                    ui.available_width(),
                    Table::Full.columns().for_plays_album(),
                );
                head(ui, columns, &tokens);
                let list = list_rect(ui, count);
                let row = table_layout::row_rect(list, index, extent::ROW_H);
                let placed = columns.at(row.min);
                found = Some((placed.title, placed.album, row));
            });
        });
        found.expect("the popular table lays out")
    }

    #[test]
    fn the_hero_draws_the_artists_name() {
        let ctx = context();
        let mut library = library_with(detail(Vec::new(), Vec::new()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            Vec2::new(900.0, 800.0),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts = drawn_texts(&output);

        assert!(
            texts.contains(&"Vela Moreno"),
            "the hero's title: {texts:?}"
        );
        assert!(texts.contains(&"ARTIST"), "the hero's eyebrow: {texts:?}");
    }

    /// Whether `artist`'s popular list holds `index` as part of its selection.
    fn picked(ctx: &Context, artist: &str, index: usize) -> bool {
        crate::view::carry::List::open(ctx, egui::Id::unique(("artist-popular", artist)), 2)
            .look(index)
            .selected
    }

    /// Whether one release block's list holds `index` as part of its selection.
    fn release_picked(ctx: &Context, release: &str, index: usize) -> bool {
        crate::view::carry::List::open(ctx, egui::Id::unique(("release-tracks", release)), 1)
            .look(index)
            .selected
    }

    #[test]
    fn a_single_click_selects_and_a_double_click_plays_it() {
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        let top = || {
            vec![
                track("spotify:track:a", "One", "Vela Moreno", 222_000),
                track("spotify:track:b", "Two", "Vela Moreno", 198_000),
            ]
        };
        let mut library = library_with(detail(top(), Vec::new()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let title = popular_cells(&ctx, size, 1, 2).0;
        let command = click(
            &ctx,
            size,
            title.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(command, None, "a single click plays nothing");
        assert!(transport.now_playing.is_none(), "and starts nothing");
        assert!(picked(&ctx, ARTIST, 1), "the row under the click is picked");
        assert!(!picked(&ctx, ARTIST, 0), "and only that row");

        // A fresh page for the double click: a third click inside the window is
        // a triple click, and that plays nothing.
        let ctx = context();
        let mut library = library_with(detail(top(), Vec::new()));
        let mut transport = Transport::default();
        let command = click(
            &ctx,
            size,
            title.center(),
            2,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "spotify:track:b".to_owned()
            }),
            "a double click starts the track"
        );
    }

    #[test]
    fn release_blocks_keep_their_own_selection_until_navigation() {
        let ctx = context();
        let mut library = library_with(detail(
            Vec::new(),
            vec![
                release("spotify:album:r1", "Album One", &["spotify:track:1"]),
                release("spotify:album:r2", "Album Two", &["spotify:track:2"]),
            ],
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        // The blocks ask for their labels, then the answers land.
        let _ = frame(
            &ctx,
            tall(),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(
            library.releases.requests().len(),
            2,
            "both blocks ask for their labels"
        );
        for (release_uri, title) in [
            ("spotify:album:r1", "First One"),
            ("spotify:album:r2", "Second One"),
        ] {
            library.apply(&Update::ReleaseTracks {
                release: release_uri.to_owned(),
                tracks: vec![track(
                    &format!("spotify:track:{release_uri}"),
                    title,
                    "Vela Moreno",
                    200_000,
                )],
            });
        }

        let (_, output) = frame(
            &ctx,
            tall(),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(drawn(&output, "First One"), "the first block's row");
        assert!(drawn(&output, "Second One"), "the second block's row");
        let row = text_rect(&output, "Second One").expect("the second block's row");

        let command = click(
            &ctx,
            tall(),
            row.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(command, None, "a single click plays nothing");
        assert!(
            release_picked(&ctx, "spotify:album:r2", 0),
            "the block under the click picks its row"
        );
        assert!(
            !release_picked(&ctx, "spotify:album:r1", 0),
            "the other block keeps its own selection"
        );

        let _ = library.open(Page::Album("spotify:album:other".to_owned()));
        crate::view::carry::follow_page(&ctx, library.page());

        assert!(
            !release_picked(&ctx, "spotify:album:r1", 0),
            "navigation forgets the first block's selection"
        );
        assert!(
            !release_picked(&ctx, "spotify:album:r2", 0),
            "and the second's"
        );
    }

    #[test]
    fn a_release_heading_click_opens_the_album_page() {
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        let mut library = library_with(detail(
            Vec::new(),
            vec![release(
                "spotify:album:r1",
                "Album One",
                &["spotify:track:1"],
            )],
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let name = text_rect(&output, "Album One").expect("the release's name is painted");
        let command = click(
            &ctx,
            size,
            name.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(
            library.page(),
            Some(&Page::Album("spotify:album:r1".to_owned())),
            "the release's heading does not open its album"
        );
        assert_eq!(
            command,
            Some(Command::LoadAlbum {
                album: "spotify:album:r1".to_owned()
            })
        );
        assert!(!queue.can_next(), "opening a page must not start the queue");
    }

    #[test]
    fn a_double_click_on_a_popular_row_plays_that_track() {
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        let top = vec![
            track("spotify:track:a", "One", "Vela Moreno", 222_000),
            track("spotify:track:b", "Two", "Vela Moreno", 198_000),
        ];
        let mut library = library_with(detail(top, Vec::new()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let title = popular_cells(&ctx, size, 1, 2).0;
        let command = click(
            &ctx,
            size,
            title.center(),
            2,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "spotify:track:b".to_owned()
            }),
            "the row under the double click is what plays"
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("spotify:track:b")
        );
    }

    #[test]
    fn the_bars_play_starts_the_top_list() {
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        let top = vec![
            track("spotify:track:a", "One", "Vela Moreno", 222_000),
            track("spotify:track:b", "Two", "Vela Moreno", 198_000),
        ];
        let mut library = library_with(detail(top, Vec::new()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (play, _) = bar_tiles(&ctx, size, "Follow");
        let command = click(
            &ctx,
            size,
            play.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "spotify:track:a".to_owned()
            }),
            "the hero plays the artist from the top of the list"
        );
        assert!(
            queue.can_next(),
            "the whole top list is queued, not just its first track"
        );
    }

    #[test]
    fn the_follow_press_asks_for_the_negated_state() {
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        let mut library = library_with(detail(
            vec![track("spotify:track:a", "One", "Vela Moreno", 222_000)],
            Vec::new(),
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, follow) = bar_tiles(&ctx, size, "Follow");
        let command = click(
            &ctx,
            size,
            follow.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(
            command,
            Some(Command::SetFollow {
                artist: ARTIST.to_owned(),
                followed: true,
            }),
            "an unconfirmed artist is followed, not unfollowed"
        );
        assert_eq!(
            library.follow.label(),
            "Follow",
            "the press alone must not flip the label"
        );
    }

    #[test]
    fn the_follow_press_targets_unfollow_once_the_server_confirmed() {
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        let mut library = library_with(detail(Vec::new(), Vec::new()));
        library.apply(&Update::Followed {
            artist: ARTIST.to_owned(),
            followed: true,
        });
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, follow) = bar_tiles(&ctx, size, "Following");
        let command = click(
            &ctx,
            size,
            follow.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(
            command,
            Some(Command::SetFollow {
                artist: ARTIST.to_owned(),
                followed: false,
            })
        );
    }

    #[test]
    fn the_follow_press_writes_nothing_while_the_sidebar_removes_that_artist() {
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        let mut library = library_with(detail(Vec::new(), Vec::new()));
        library.apply(&Update::FollowedArtists(vec![FollowedArtist {
            uri: ARTIST.to_owned(),
            name: "Vela Moreno".to_owned(),
            named: true,
            portrait: None,
            releases: None,
        }]));
        library.apply(&Update::Followed {
            artist: ARTIST.to_owned(),
            followed: true,
        });
        assert!(
            library
                .remove_from_library(&crate::menu::LibraryItem {
                    kind: crate::sidebar_kind::SidebarKind::Artists,
                    uri: ARTIST.to_owned(),
                    name: "Vela Moreno".to_owned(),
                })
                .is_some(),
            "the sidebar removal never started"
        );
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, follow) = bar_tiles(&ctx, size, "Following");
        let command = click(
            &ctx,
            size,
            follow.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(command, None, "a follow write raced the sidebar's removal");
        assert!(
            !library.follow.pending(),
            "the page armed a write the sidebar already sent"
        );
        assert_eq!(
            library.follow.label(),
            "Following",
            "the label moved before the removal answered"
        );
    }

    #[test]
    fn a_follow_failure_draws_its_copy_beside_the_confirmed_label() {
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        let mut library = library_with(detail(Vec::new(), Vec::new()));
        library.apply(&Update::Followed {
            artist: ARTIST.to_owned(),
            followed: true,
        });
        let _ = library.follow.pressed();
        library.apply(&Update::FollowFailed {
            artist: ARTIST.to_owned(),
            failure: Failure::Save,
        });
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts = drawn_texts(&output);
        let copy = crate::signin::failure_message(Failure::Save);
        assert!(
            texts.contains(&copy),
            "the failed write's copy is drawn: {texts:?}"
        );
        assert!(
            texts.contains(&"Following"),
            "the label stays where the server last left it: {texts:?}"
        );

        // The row's name box, in front of the control: the row has no second
        // line, so a message may not add one.
        let (play, follow) = bar_tiles(&ctx, size, "Following");
        let hint = text_rect(&output, copy).expect("the failure's copy is painted");
        assert!(
            hint.left() >= play.right() + crate::theme::actionbar::GAP - 0.01,
            "the copy starts where the name's box starts: {hint:?} against {play:?}"
        );
        assert!(
            hint.right() <= follow.left() - crate::theme::actionbar::GAP + 0.01,
            "the copy stops short of the control: {hint:?} against {follow:?}"
        );
        assert!(
            (hint.center().y - follow.center().y).abs() < 1.0,
            "the copy shares the control's line: {hint:?} against {follow:?}"
        );

        // The row keeps its resting height, so the message moved nothing below
        // it: the sections sit where they sit without one.
        let head = text_rect(&output, "Popular").expect("the Popular head is painted");
        let mut quiet = library_with(detail(Vec::new(), Vec::new()));
        quiet.apply(&Update::Followed {
            artist: ARTIST.to_owned(),
            followed: true,
        });
        let (_, plain) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut quiet,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(
            head,
            text_rect(&plain, "Popular").expect("the Popular head is painted without one"),
            "a failure may not push the page down"
        );
    }

    #[test]
    fn the_follow_press_still_writes_when_the_bar_is_pinned() {
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        // A discography long enough to scroll the bar out of the viewport.
        let releases: Vec<Release> = (1..=12)
            .map(|index| {
                release(
                    &format!("spotify:album:r{index}"),
                    &format!("Release {index}"),
                    &["spotify:track:a", "spotify:track:b"],
                )
            })
            .collect();
        let mut library = library_with(detail(
            vec![track("spotify:track:a", "One", "Vela Moreno", 222_000)],
            releases,
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let mut output = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        )
        .1;
        for _ in 0..40 {
            if bar_rect(&output).is_some() {
                break;
            }
            output = wheel(
                &ctx,
                size,
                -extent::ROW_H,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
        }
        let bar = bar_rect(&output).expect("the artist's row never pinned");
        let follow = crate::layout::actionbar::solve(bar, Kind::Other, bar_parts(&ctx, "Follow"))
            .follow
            .expect("the pinned bar keeps its text control");

        let command = click(
            &ctx,
            size,
            follow.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(
            command,
            Some(Command::SetFollow {
                artist: ARTIST.to_owned(),
                followed: true,
            }),
            "a pinned bar's Follow writes what it wrote in the band"
        );
    }

    /// A Popular row reveals through the page's seam: a row the pinned bar
    /// partly covers is asked for clear of the bar, not just into the scroll
    /// area's own clip.
    #[test]
    fn a_focused_popular_row_partly_under_the_bar_is_scrolled_clear_of_it() {
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        // Enough rows that the row can be walked up under the pinned bar and
        // still have somewhere to scroll back from.
        let top: Vec<TrackEntry> = (0..40)
            .map(|index| {
                track(
                    &format!("spotify:track:p{index}"),
                    &format!("P{index}"),
                    "Vela Moreno",
                    200_000,
                )
            })
            .collect();
        let mut library = library_with(detail(top, Vec::new()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let mut output = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        )
        .1;
        for _ in 0..40 {
            if bar_rect(&output).is_some() {
                break;
            }
            output = wheel(
                &ctx,
                size,
                -extent::ROW_H,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
        }
        let bar = bar_rect(&output).expect("the artist's row never pinned");

        // The first row is wholly below the pinned row, so a press on it lands
        // on the row and not in the strip the row owns.
        let title = text_rect(&output, "P0").expect("the row paints its title");
        let _ = click(
            &ctx,
            size,
            Pos2::new(title.left() + 2.0, title.center().y),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let focused = ctx
            .memory(|memory| memory.focused())
            .expect("the row took the keyboard");

        // Walk the row up until its top is inside the strip the pinned row
        // covers, then let what the reveal asked for settle.
        let mut covered = false;
        for _ in 0..80 {
            let row = ctx.read_response(focused).expect("the row is drawn").rect;
            if row.top() < bar.bottom() - 0.5 {
                covered = true;
                break;
            }
            let _ = wheel(
                &ctx,
                size,
                -6.0,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
        }
        assert!(covered, "the row never slid under the pinned row");

        let mut output = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        )
        .1;
        let mut settled = false;
        for _ in 0..240 {
            if let Some(row) = ctx.read_response(focused)
                && row.rect.top() >= bar.bottom() - 0.01
            {
                settled = true;
                break;
            }
            output = frame(
                &ctx,
                size,
                Vec::new(),
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            )
            .1;
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
        let bar = bar_rect(&output).expect("the row is still pinned");
        let row = ctx
            .read_response(focused)
            .expect("the row is drawn again")
            .rect;
        assert!(
            row.top() >= bar.bottom() - 0.01,
            "the focused row rests below the pinned row: {row:?} against {bar:?}"
        );
    }

    /// A Popular row the cull skipped is still asked for through the page's
    /// seam: a row wholly above the strip the pinned bar owns draws nothing, so
    /// it has no reveal of its own and the section has to ask for it.
    #[test]
    fn a_focused_popular_row_wholly_under_the_bar_is_still_brought_out() {
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        // Enough rows that the row can be jumped wholly above the pinned bar
        // and still have somewhere to scroll back from.
        let top: Vec<TrackEntry> = (0..40)
            .map(|index| {
                track(
                    &format!("spotify:track:p{index}"),
                    &format!("P{index}"),
                    "Vela Moreno",
                    200_000,
                )
            })
            .collect();
        let mut library = library_with(detail(top, Vec::new()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let mut output = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        )
        .1;
        for _ in 0..40 {
            if bar_rect(&output).is_some() {
                break;
            }
            output = wheel(
                &ctx,
                size,
                -extent::ROW_H,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
        }
        assert!(
            bar_rect(&output).is_some(),
            "the artist's row is pinned before the press"
        );

        // The first row the list draws, so a press on its title lands on the row
        // and not in the strip the pinned row owns.
        let title = text_rect(&output, "P0").expect("the row paints its title");
        let _ = click(
            &ctx,
            size,
            Pos2::new(title.left() + 2.0, title.center().y),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let focused = ctx
            .memory(|memory| memory.focused())
            .expect("the row took the keyboard");
        // The press lands on whichever row was under the pointer when the click
        // resolved, so the focused row is read from the frame it was drawn in.
        let after = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        )
        .1;
        let rect = ctx
            .read_response(focused)
            .expect("the focused row is drawn")
            .rect;
        let label = drawn_texts(&after)
            .into_iter()
            .find(|text| {
                text.strip_prefix('P')
                    .and_then(|tail| tail.parse::<usize>().ok())
                    .is_some()
                    && text_rect(&after, text).is_some_and(|box_| rect.contains_rect(box_))
            })
            .map(str::to_owned)
            .expect("the focused row paints its title");

        // One wheel jump of at least the pinned row and a whole row, and enough
        // to leave this row wholly above the strip, so the frame after it draws
        // nothing at the focused row: the cull skips it and `rows::body`, which
        // is a drawn row's own reveal, never runs.
        let strip = bar_rect(&after).expect("the row is pinned").bottom();
        let jump = (rect.bottom() - strip).max(crate::theme::actionbar::STUCK_H) + extent::ROW_H;
        let mut output = wheel(
            &ctx,
            size,
            -jump,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let mut culled = false;
        let mut settled = false;
        for _ in 0..240 {
            let strip = bar_rect(&output).expect("the row is still pinned").bottom();
            if text_rect(&output, &label).is_none() {
                culled = true;
            } else if culled
                && ctx
                    .read_response(focused)
                    .is_some_and(|row| row.rect.top() >= strip - 0.01)
            {
                settled = true;
                break;
            }
            output = frame(
                &ctx,
                size,
                Vec::new(),
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            )
            .1;
        }
        assert!(
            culled,
            "the jump never put the focused row wholly under the pinned row"
        );
        assert!(
            settled,
            "the focused row never came out from under the pinned row"
        );
        let bar = bar_rect(&output).expect("the row is still pinned");
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
    fn an_off_screen_block_asks_for_no_labels() {
        let ctx = context();
        let size = Vec2::new(900.0, 600.0);
        let mut library = library_with(detail(
            Vec::new(),
            vec![
                release("spotify:album:r1", "One", &["spotify:track:1"]),
                release("spotify:album:r2", "Two", &["spotify:track:2"]),
                release("spotify:album:r3", "Three", &["spotify:track:3"]),
            ],
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let _ = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        for absent in ["spotify:album:r2", "spotify:album:r3"] {
            assert_eq!(
                library.releases.look(absent),
                Labels::Idle,
                "{absent} is off screen and must not be wanted"
            );
        }
    }

    /// One frame asks for the on-screen block's labels and nothing else: the
    /// want set is filled by the draw and drained once, afterwards.
    #[test]
    fn one_frame_asks_for_exactly_the_on_screen_labels() {
        let ctx = context();
        let size = Vec2::new(900.0, 600.0);
        let mut library = library_with(detail(
            Vec::new(),
            vec![
                release("spotify:album:r1", "One", &["spotify:track:1"]),
                release("spotify:album:r2", "Two", &["spotify:track:2"]),
                release("spotify:album:r3", "Three", &["spotify:track:3"]),
            ],
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let _ = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let requests = library.releases.requests();

        assert_eq!(
            requests,
            vec![Command::LoadReleaseTracks {
                release: "spotify:album:r1".to_owned(),
                tracks: vec!["spotify:track:1".to_owned()],
            }],
            "exactly the block that is on screen is asked for: {requests:?}"
        );
        assert!(
            library.releases.requests().is_empty(),
            "the drain clears the want set, so one frame is one request"
        );
    }

    /// A realistic discography: forty-eight releases, far more than fits. The
    /// frame must draw, and must ask only for the blocks it can show.
    #[test]
    fn a_long_discography_asks_only_for_what_is_on_screen() {
        let ctx = context();
        // 900 tall, not 800: the filter row takes its own height above the
        // blocks, and at 800 only one block fits, which would not exercise the
        // two-request cap.
        let size = Vec2::new(900.0, 900.0);
        let releases: Vec<Release> = (1..=48)
            .map(|index| {
                release(
                    &format!("spotify:album:r{index}"),
                    &format!("Release {index}"),
                    &["spotify:track:a", "spotify:track:b"],
                )
            })
            .collect();
        let mut library = library_with(detail(Vec::new(), releases));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert!(
            drawn_texts(&output).contains(&"Release 1"),
            "the first block's head is drawn"
        );
        let requests = library.releases.requests();
        assert_eq!(
            requests.len(),
            2,
            "the top of the discography is what fits, and the cap holds: {requests:?}"
        );
        for command in &requests {
            let Command::LoadReleaseTracks { release, .. } = command else {
                panic!("a label drain emits only label requests: {command:?}");
            };
            assert!(
                release == "spotify:album:r1" || release == "spotify:album:r2",
                "a block far below the fold was asked for: {release}"
            );
        }
    }

    #[test]
    fn a_release_rows_double_click_plays_that_track() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let size = Vec2::new(900.0, 800.0);
        let release = _Release {
            uri: "spotify:album:r1".to_owned(),
            name: "One".to_owned(),
            kind: ReleaseKind::Album,
            year: Some(2024),
            cover: None,
            tracks: vec!["spotify:track:1".to_owned(), "spotify:track:2".to_owned()],
        };
        let labels = vec![
            track("spotify:track:1", "One", "", 222_000),
            track("spotify:track:2", "Two", "", 198_000),
        ];
        let mut transport = Transport::default();
        let mut clicked = None;
        let likes = crate::likes::Likes::default();

        // The second row's rect inside a block drawn at the top of the pane.
        let mut row_rect = None;
        run(&ctx, input(size, Vec::new()), |ui| {
            let top = ui.cursor().min;
            let list_top = top.y + stroke::HAIRLINE + space::S5 + extent::RELEASE_ART + space::S4;
            let list = Rect::from_min_size(
                pos2(top.x, list_top),
                vec2(ui.available_width(), 2.0 * extent::ROW_H),
            );
            row_rect = Some(table_layout::row_rect(list, 1, extent::ROW_H));
        });
        let row_rect = row_rect.expect("the block's body lays out");

        for _ in 0..2 {
            for event in click_events(row_rect.center()) {
                run(&ctx, input(size, vec![event]), |ui| {
                    let block = Block {
                        release: &release,
                        labels: Labels::Ready(&labels),
                        cover: Cover::Missing,
                        on_screen: true,
                        playable: true,
                        here: None,
                        writing: false,
                        likes: &likes,
                        saved: None,
                    };
                    let out =
                        super::release_block::show(ui, Table::Full, &block, &transport, &tokens);
                    clicked = Some(out);
                });
            }
        }

        assert_eq!(
            clicked.and_then(|clicked| clicked.row),
            Some(1),
            "the row under the double click is what plays"
        );
        let _ = &mut transport;
    }

    /// A debug build paints egui's `Unaligned` marker inside any widget whose
    /// rect is off the rounding grid, which makes the chip row unreadable. The
    /// page must draw none.
    #[test]
    fn the_page_paints_no_unaligned_marker() {
        let ctx = context();
        let mut library = library_with(detail(
            vec![track(
                "spotify:track:1",
                "Slow Tide",
                "Vela Moreno",
                222_000,
            )],
            vec![
                release("spotify:album:r1", "Slow Tide", &["spotify:track:1"]),
                release("spotify:album:r2", "Kestrel", &["spotify:track:1"]),
            ],
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let art = &mut Art::default();

        let (_, output) = frame(
            &ctx,
            Vec2::new(900.0, 800.0),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            art,
        );

        assert!(
            !drawn_texts(&output).contains(&"Unaligned"),
            "the page paints an unaligned widget: {:?}",
            drawn_texts(&output)
        );
    }

    /// The sections sit one gutter inside the pane, so a release's art does not
    /// rub the panel's edge and the count does not touch the far one. The hero
    /// stays full-bleed above them.
    #[test]
    fn the_sections_sit_one_gutter_inside_the_pane() {
        let ctx = context();
        let mut library = library_with(detail(
            vec![track(
                "spotify:track:1",
                "Slow Tide",
                "Vela Moreno",
                222_000,
            )],
            vec![release(
                "spotify:album:r1",
                "Slow Tide",
                &["spotify:track:1"],
            )],
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let art = &mut Art::default();

        let probe = Vec2::new(900.0, 800.0);
        let (_, output) = frame(
            &ctx,
            probe,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            art,
        );

        let popular = text_rect(&output, "Popular").expect("the section head is painted");
        assert!(
            (popular.left() - space::GUTTER).abs() < 0.01,
            "the section title sits at {}, the pane's gutter is {}",
            popular.left(),
            space::GUTTER
        );

        let aside = drawn_texts(&output)
            .into_iter()
            .find(|text| text.contains("RELEASE"))
            .expect("the discography counts its releases");
        let aside = text_rect(&output, aside).expect("the aside is painted");
        assert!(
            (aside.right() - (probe.x - space::GUTTER)).abs() < 0.01,
            "the aside ends at {}, the pane's far gutter starts at {}",
            aside.right(),
            probe.x - space::GUTTER
        );

        // The hero above them is still full-bleed: its title starts at the
        // gutter because the band pads itself, and its photograph spans the pane.
        let title = text_rect(&output, "Vela Moreno").expect("the hero title is painted");
        assert!(
            (title.left() - space::GUTTER).abs() < 0.01,
            "the hero title sits at {}",
            title.left()
        );
    }

    #[test]
    fn an_empty_artist_draws_both_empty_copies() {
        let ctx = context();
        let mut library = library_with(detail(Vec::new(), Vec::new()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            Vec2::new(900.0, 800.0),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts = drawn_texts(&output);

        assert!(
            texts.contains(&"No popular tracks."),
            "the Popular empty copy: {texts:?}"
        );
        assert!(
            texts.contains(&"No releases."),
            "the Discography empty copy: {texts:?}"
        );
        assert!(texts.contains(&"Popular"), "the section head: {texts:?}");
        assert!(texts.contains(&"Discography"), "{texts:?}");
    }

    #[test]
    fn a_failure_draws_the_message_and_a_retry_that_asks_again() {
        let ctx = context();
        let mut library = Library::default();
        library.open(Page::Artist(ARTIST.to_owned()));
        library.apply(&Update::ArtistFailed {
            artist: ARTIST.to_owned(),
            failure: Failure::Load,
        });
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            Vec2::new(900.0, 800.0),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&crate::signin::failure_message(Failure::Load)),
            "the failure's own copy: {texts:?}"
        );

        let retry = text_rect(&output, "Retry").expect("a retry control");
        let command = click(
            &ctx,
            Vec2::new(900.0, 800.0),
            retry.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(
            command,
            Some(Command::LoadArtist {
                artist: ARTIST.to_owned()
            }),
            "Retry re-issues the same load"
        );
        assert_eq!(library.artist, Load::Loading);
    }

    #[test]
    fn a_ready_artist_for_another_page_draws_the_loading_state() {
        let ctx = context();
        let mut library = Library::default();
        library.open(Page::Artist(ARTIST.to_owned()));
        // The state the library guard exists for: a ready detail left from an
        // earlier page.
        library.artist = Load::Ready(ArtistDetail {
            uri: OTHER_ARTIST.to_owned(),
            name: "Another Artist".to_owned(),
            ..detail(Vec::new(), Vec::new())
        });
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            Vec2::new(900.0, 800.0),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts = drawn_texts(&output);

        assert!(
            texts.is_empty(),
            "the page paints no words for another artist: {texts:?}"
        );
        assert!(
            output.shapes.iter().any(|clipped| matches!(
                &clipped.shape,
                egui::epaint::Shape::Rect(rect) if rect.fill == Tokens::DARK.palette.skeleton_rule
            )),
            "the page holds its rows open instead of naming another artist"
        );
    }

    #[test]
    fn the_row_has_all_plus_the_kinds_the_artist_has() {
        let ctx = context();
        let mut library = library_with(detail(Vec::new(), mixed()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            tall(),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts = drawn_texts(&output);

        assert!(texts.contains(&"ALL"), "{texts:?}");
        assert!(texts.contains(&"ALBUMS"), "{texts:?}");
        assert!(texts.contains(&"EPS"), "{texts:?}");
        assert!(!texts.contains(&"SINGLES"), "no single: {texts:?}");
        assert!(
            !texts.contains(&"COMPILATIONS"),
            "no compilation: {texts:?}"
        );
    }

    #[test]
    fn the_row_sits_between_the_head_and_the_first_block() {
        let ctx = context();
        let mut library = library_with(detail(Vec::new(), mixed()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            tall(),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let head = text_rect(&output, "Discography").expect("the section head");
        let first_chip = chip_rect(&output, Chip::All).expect("the All chip");
        let first_block = text_rect(&output, "Album One").expect("the first block");
        assert!(
            head.bottom() < first_chip.top(),
            "the row is under the head: {head:?} against {first_chip:?}"
        );
        assert!(
            first_chip.bottom() < first_block.top(),
            "the row is above the first block: {first_chip:?} against {first_block:?}"
        );
    }

    #[test]
    fn all_is_pressed_on_first_draw() {
        let ctx = context();
        let releases = mixed();
        let mut library = library_with(detail(Vec::new(), releases.clone()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            tall(),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(library.filter.chosen(), Chip::All);
        assert_eq!(
            library.filter.effective(&chips(&releases)),
            Chip::All,
            "the pressed chip is All, so every block draws"
        );
        for name in ["Album One", "An EP", "Album Two"] {
            assert!(drawn(&output, name), "{name} is drawn: {name}");
        }
    }

    #[test]
    fn opening_another_artist_draws_all_even_when_it_has_the_same_kind() {
        let ctx = context();
        let releases = mixed();
        let mut library = library_with(detail(Vec::new(), releases.clone()));
        library.filter.choose(Chip::Kind(ReleaseKind::Ep));
        let mut other = detail(Vec::new(), releases);
        other.uri = OTHER_ARTIST.to_owned();
        library.open(Page::Artist(other.uri.clone()));
        library.apply(&Update::Artist(other));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            tall(),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(
            library.filter.chosen(),
            Chip::All,
            "another artist's page starts on All"
        );
        assert!(
            drawn(&output, "EPS"),
            "the other artist has an EP chip too: {:?}",
            drawn_texts(&output)
        );
        for name in ["Album One", "An EP", "Album Two"] {
            assert!(
                drawn(&output, name),
                "All filters nothing on the new artist: {name}"
            );
        }
    }

    #[test]
    fn pressing_a_chip_filters_the_blocks_in_place() {
        let ctx = context();
        let size = tall();
        let mut library = library_with(detail(Vec::new(), mixed()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, before) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let albums = chip_rect(&before, Chip::Kind(ReleaseKind::Album)).expect("the Albums chip");
        let page = library.page().cloned();
        let can_back = library.can_back();

        let command = click(
            &ctx,
            size,
            albums.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(command, None, "a filter press asks for nothing");
        assert_eq!(library.page(), page.as_ref(), "it does not navigate");
        assert_eq!(library.can_back(), can_back, "it adds no history entry");
        assert!(
            matches!(library.artist, Load::Ready(_)),
            "the artist is not refetched"
        );
        assert_eq!(
            library.filter.chosen(),
            Chip::Kind(ReleaseKind::Album),
            "the pressed chip is the choice"
        );

        let (_, after) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(drawn(&after, "Album One"), "an album block stays");
        assert!(drawn(&after, "Album Two"), "every album block stays");
        assert!(
            !drawn(&after, "An EP"),
            "the EP block draws nothing at all: {:?}",
            drawn_texts(&after)
        );
    }

    #[test]
    fn pressing_the_pressed_chip_does_nothing() {
        let ctx = context();
        let size = tall();
        let mut library = library_with(detail(Vec::new(), mixed()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, before) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let albums = chip_rect(&before, Chip::Kind(ReleaseKind::Album)).expect("the Albums chip");
        let _ = click(
            &ctx,
            size,
            albums.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let _ = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let _ = click(
            &ctx,
            size,
            albums.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let (_, after) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(
            library.filter.chosen(),
            Chip::Kind(ReleaseKind::Album),
            "the row is exclusive: a second press is not a way back out"
        );
        assert!(!drawn(&after, "An EP"), "the filter is still applied");
    }

    #[test]
    fn all_restores_every_block() {
        let ctx = context();
        let size = tall();
        let mut library = library_with(detail(Vec::new(), mixed()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, before) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let albums = chip_rect(&before, Chip::Kind(ReleaseKind::Album)).expect("the Albums chip");
        let all = chip_rect(&before, Chip::All).expect("the All chip");
        let _ = click(
            &ctx,
            size,
            albums.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let _ = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let _ = click(
            &ctx,
            size,
            all.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let (_, after) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(library.filter.chosen(), Chip::All);
        for name in ["Album One", "An EP", "Album Two"] {
            assert!(drawn(&after, name), "{name} is back");
        }
    }

    #[test]
    fn a_hidden_block_draws_nothing_and_reserves_nothing() {
        let ctx = context();
        let size = tall();
        let mut unfiltered = library_with(detail(Vec::new(), mixed()));
        let mut filtered = library_with(detail(Vec::new(), mixed()));
        filtered.filter.choose(Chip::Kind(ReleaseKind::Ep));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, before) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut unfiltered,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let (_, after) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut filtered,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts = drawn_texts(&after);

        assert!(!texts.contains(&"Album One"), "{texts:?}");
        assert!(!texts.contains(&"Album Two"), "{texts:?}");

        let first_unfiltered = text_rect(&before, "Album One").expect("the first block");
        let first_filtered = text_rect(&after, "An EP").expect("the only block");
        assert!(
            (first_filtered.top() - first_unfiltered.top()).abs() < 0.01,
            "the hidden blocks reserved no space: {} against {}",
            first_filtered.top(),
            first_unfiltered.top()
        );
    }

    #[test]
    fn a_hidden_block_has_no_wants() {
        let ctx = context();
        let size = tall();
        let mut releases = mixed();
        releases[0].cover = Some(cover("album"));
        releases[1].cover = Some(cover("ep"));
        let now = Instant::now();

        // The control: unfiltered, both blocks are on screen and ask.
        let mut open = library_with(detail(Vec::new(), releases.clone()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        art.begin_frame();
        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut open,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(drawn(&output, "An EP"), "the EP block is on screen");
        assert!(drawn(&output, "Album One"), "the album block is on screen");
        let wanted_labels = open.releases.requests();
        let wanted_covers = art.requests(now);
        assert!(
            wanted_labels
                .iter()
                .any(|command| matches!(command, Command::LoadReleaseTracks { release, .. } if release == "spotify:album:r1")),
            "the control frame asks for the album's labels: {wanted_labels:?}"
        );
        assert!(
            wanted_covers.iter().any(
                |command| matches!(command, Command::Fetch { url } if *url == cover_url("album"))
            ),
            "the control frame asks for the album's cover: {wanted_covers:?}"
        );

        // The hidden album asks for neither.
        let mut hidden = library_with(detail(Vec::new(), releases));
        hidden.filter.choose(Chip::Kind(ReleaseKind::Ep));
        let mut art = Art::default();
        art.begin_frame();
        let _ = frame(
            &ctx,
            size,
            Vec::new(),
            &mut hidden,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let wanted_labels = hidden.releases.requests();
        let wanted_covers = art.requests(now);
        assert!(
            !wanted_labels.iter().any(|command| matches!(
                command,
                Command::LoadReleaseTracks { release, .. } if release == "spotify:album:r1"
            )),
            "a hidden block asks for no labels: {wanted_labels:?}"
        );
        assert!(
            !wanted_covers.iter().any(
                |command| matches!(command, Command::Fetch { url } if *url == cover_url("album"))
            ),
            "a hidden block fetches no cover: {wanted_covers:?}"
        );
        assert!(
            wanted_covers.iter().any(
                |command| matches!(command, Command::Fetch { url } if *url == cover_url("ep"))
            ),
            "the block the filter keeps still fetches its cover: {wanted_covers:?}"
        );
    }

    #[test]
    fn an_on_screen_matching_block_still_asks_for_labels() {
        let ctx = context();
        let size = tall();
        let mut library = library_with(detail(Vec::new(), mixed()));
        library.filter.choose(Chip::Kind(ReleaseKind::Ep));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert!(drawn(&output, "An EP"));
        assert_eq!(
            library.releases.requests(),
            vec![Command::LoadReleaseTracks {
                release: "spotify:ep:r2".to_owned(),
                tracks: vec!["spotify:track:2".to_owned()],
            }],
            "the block the filter keeps is still labelled"
        );
    }

    #[test]
    fn the_aside_keeps_the_artists_totals_while_filtered() {
        let ctx = context();
        let size = tall();
        let mut library = library_with(detail(Vec::new(), mixed()));
        library.filter.choose(Chip::Kind(ReleaseKind::Album));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let aside = discography_meta(3, 3).to_uppercase();
        assert!(
            drawn(&output, &aside),
            "the aside describes the artist, not the filter: {aside}"
        );
    }

    #[test]
    fn a_choice_with_no_chip_after_a_reload_draws_all() {
        let ctx = context();
        let size = tall();
        let mut library = library_with(detail(Vec::new(), mixed()));
        library.filter.choose(Chip::Kind(ReleaseKind::Single));

        // A reload of the same artist: home, then Back, then the reply.
        library.to_playlists();
        let _ = library.back();
        library.apply(&Update::Artist(detail(Vec::new(), mixed())));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        for name in ["Album One", "An EP", "Album Two"] {
            assert!(
                drawn(&output, name),
                "{name} draws under the fallback to All"
            );
        }
        assert_eq!(
            library.filter.chosen(),
            Chip::Kind(ReleaseKind::Single),
            "the choice is remembered, not replaced"
        );
    }

    #[test]
    fn a_single_kind_artist_still_gets_all_and_that_kind() {
        let ctx = context();
        let mut library = library_with(detail(
            Vec::new(),
            vec![release("spotify:album:r1", "One", &["spotify:track:1"])],
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            tall(),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert!(drawn(&output, "ALL"), "{:?}", drawn_texts(&output));
        assert!(drawn(&output, "ALBUMS"), "{:?}", drawn_texts(&output));
        assert!(!drawn(&output, "EPS"), "{:?}", drawn_texts(&output));
    }

    #[test]
    fn an_artist_with_releases_always_has_the_row_and_the_field() {
        let ctx = context();
        let mut library = library_with(detail(
            Vec::new(),
            vec![release_of_kind(
                "spotify:audiobook:r1",
                "A Book",
                ReleaseKind::Other,
                &["spotify:track:1"],
            )],
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            tall(),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert!(
            drawn(&output, "ALL"),
            "a single kind still gets the row: {:?}",
            drawn_texts(&output)
        );
        assert!(
            drawn(&output, "FILTER"),
            "the field must exist even when there is nothing to chip between: {:?}",
            drawn_texts(&output)
        );
        assert!(
            drawn(&output, "A Book"),
            "the Other release stays reachable under All"
        );
    }

    #[test]
    fn no_releases_draws_no_row_and_no_field() {
        let ctx = context();
        let mut library = library_with(detail(Vec::new(), Vec::new()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            tall(),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts = drawn_texts(&output);

        assert!(texts.contains(&"No releases."), "{texts:?}");
        assert!(
            !texts.contains(&"ALL"),
            "there is nothing to filter: {texts:?}"
        );
        assert!(
            !texts.contains(&"FILTER"),
            "a discography with no releases has no field: {texts:?}"
        );
    }

    #[test]
    fn the_chip_row_starts_at_the_gutter() {
        let ctx = context();
        let mut library = library_with(detail(Vec::new(), mixed()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            tall(),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let all = chip_rect(&output, Chip::All).expect("the All chip");
        assert!(
            (all.left() - space::GUTTER).abs() < 0.01,
            "the row starts at {}, the gutter is {}",
            all.left(),
            space::GUTTER
        );
    }

    #[test]
    fn at_a_wide_pane_the_row_is_one_line() {
        let ctx = context();
        let mut library = library_with(detail(Vec::new(), every_kind()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let sizes = chip_sizes(&ctx);
        let needed: f32 =
            sizes.iter().map(|size| size.x).sum::<f32>() + space::S1 * (sizes.len() - 1) as f32;
        let wide = needed
            + 2.0 * space::GUTTER
            + crate::theme::cluster::FIELD_W
            + crate::theme::cluster::GAP
            + 2.0 * space::GUTTER;

        let (_, output) = frame(
            &ctx,
            Vec2::new(wide, 1500.0),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let tops: Vec<f32> = [
            Chip::All,
            Chip::Kind(ReleaseKind::Album),
            Chip::Kind(ReleaseKind::Ep),
            Chip::Kind(ReleaseKind::Single),
            Chip::Kind(ReleaseKind::Compilation),
        ]
        .into_iter()
        .filter_map(|chip| chip_rect(&output, chip))
        .map(|rect| rect.top())
        .collect();
        assert_eq!(tops.len(), 5, "every chip is drawn");
        for top in &tops {
            assert!(
                (top - tops[0]).abs() < 0.01,
                "one line: {tops:?} at {wide} wide"
            );
        }
    }

    #[test]
    fn at_a_narrow_pane_the_row_wraps_inside_the_gutters() {
        let ctx = context();
        let mut library = library_with(detail(Vec::new(), every_kind()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let sizes = chip_sizes(&ctx);
        let needed: f32 =
            sizes.iter().map(|size| size.x).sum::<f32>() + space::S1 * (sizes.len() - 1) as f32;
        let narrow = 2.0 * space::GUTTER + needed / 2.0;

        let (_, output) = frame(
            &ctx,
            Vec2::new(narrow, 1500.0),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let rects: Vec<Rect> = [
            Chip::All,
            Chip::Kind(ReleaseKind::Album),
            Chip::Kind(ReleaseKind::Ep),
            Chip::Kind(ReleaseKind::Single),
            Chip::Kind(ReleaseKind::Compilation),
        ]
        .into_iter()
        .filter_map(|chip| chip_rect(&output, chip))
        .collect();
        assert_eq!(rects.len(), 5, "every chip is drawn");
        let first = rects[0].top();
        assert!(
            rects.iter().any(|rect| (rect.top() - first).abs() > 0.01),
            "the row wraps at {narrow}: {rects:?}"
        );
        for rect in &rects {
            assert!(
                rect.left() >= space::GUTTER - 0.01,
                "inside the near gutter: {rect:?}"
            );
            assert!(
                rect.right() <= narrow - space::GUTTER + 0.01,
                "inside the far gutter: {rect:?}"
            );
        }
    }

    #[test]
    fn the_row_grows_with_its_wraps_and_pushes_the_blocks_down() {
        let ctx = context();
        let sizes = chip_sizes(&ctx);
        let needed: f32 =
            sizes.iter().map(|size| size.x).sum::<f32>() + space::S1 * (sizes.len() - 1) as f32;
        // One point short of fitting on one line: the row wraps a chip, and the
        // blocks keep a legible width.
        let narrow =
            needed + 2.0 * space::GUTTER + crate::theme::cluster::FIELD_W + space::S1 - 1.0;
        let wide = narrow + 4.0;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let mut wide_library = library_with(detail(Vec::new(), every_kind()));
        let (_, wide_out) = frame(
            &ctx,
            Vec2::new(wide, 1500.0),
            Vec::new(),
            &mut wide_library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let mut narrow_library = library_with(detail(Vec::new(), every_kind()));
        let (_, narrow_out) = frame(
            &ctx,
            Vec2::new(narrow, 1500.0),
            Vec::new(),
            &mut narrow_library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let chips_on_narrow: Vec<Rect> = [
            Chip::All,
            Chip::Kind(ReleaseKind::Album),
            Chip::Kind(ReleaseKind::Ep),
            Chip::Kind(ReleaseKind::Single),
            Chip::Kind(ReleaseKind::Compilation),
        ]
        .into_iter()
        .filter_map(|chip| chip_rect(&narrow_out, chip))
        .collect();
        let mut tops: Vec<f32> = chips_on_narrow.iter().map(Rect::top).collect();
        tops.dedup_by(|a, b| (*a - *b).abs() < 0.01);
        let rows = tops.len();
        assert!(rows >= 2, "the narrow row wraps: {tops:?}");

        let chip_height = chips_on_narrow[0].height();
        let expected = (rows - 1) as f32 * (chip_height + space::S1);
        let wide_block = text_rect(&wide_out, "An Album").expect("the first block");
        let narrow_block = text_rect(&narrow_out, "An Album").expect("the first block");
        assert!(
            (narrow_block.top() - wide_block.top() - expected).abs() < 0.01,
            "the blocks move down by the wrapped rows: {} against {} for {rows} rows",
            narrow_block.top() - wide_block.top(),
            expected
        );
    }

    /// The row reserves the cluster's whole height, not the chip's: the first
    /// block sits below the band by the field's height and the step under it.
    #[test]
    fn the_row_reserves_its_height_above_the_first_block() {
        let ctx = context();
        let size = Vec2::new(900.0, 1500.0);
        let mut library = library_with(detail(
            Vec::new(),
            vec![release("spotify:album:r1", "A Book", &["spotify:track:1"])],
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let head = text_rect(&output, "Discography").expect("the section head");
        let chip = chip_rect(&output, Chip::All).expect("the All chip");
        let row_top = chip.center().y - crate::theme::cluster::FIELD_H / 2.0;
        let block_top = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape)
                    if shape.fill == Tokens::DARK.palette.border
                        && (shape.rect.height() - stroke::HAIRLINE).abs() < 0.01
                        && shape.rect.top() > row_top + crate::theme::cluster::FIELD_H =>
                {
                    Some(shape.rect.top())
                }
                _ => None,
            })
            .fold(f32::INFINITY, f32::min);
        let above_block = block_top - row_top;
        let expected = crate::theme::cluster::FIELD_H + space::S4 + space::S2;

        assert!(
            (above_block - expected).abs() < 0.01,
            "the row takes its cluster's height and the step under it: {above_block} against {expected}"
        );
        assert!(
            row_top > head.bottom(),
            "the row still starts under the section head"
        );
    }

    #[test]
    fn the_wrapped_row_never_overlaps_the_first_block() {
        let ctx = context();
        let sizes = chip_sizes(&ctx);
        let needed: f32 =
            sizes.iter().map(|size| size.x).sum::<f32>() + space::S1 * (sizes.len() - 1) as f32;
        let narrow = 2.0 * space::GUTTER + needed - 1.0;
        let mut library = library_with(detail(Vec::new(), every_kind()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            Vec2::new(narrow, 1500.0),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let first_block = text_rect(&output, "An Album").expect("the first block");
        for chip in [
            Chip::All,
            Chip::Kind(ReleaseKind::Album),
            Chip::Kind(ReleaseKind::Ep),
            Chip::Kind(ReleaseKind::Single),
            Chip::Kind(ReleaseKind::Compilation),
        ] {
            let rect = chip_rect(&output, chip).expect("every chip is drawn");
            assert!(
                rect.bottom() < first_block.top(),
                "{chip:?} ends at {}, the first block starts at {}",
                rect.bottom(),
                first_block.top()
            );
        }
    }

    // -----------------------------------------------------------------------
    // The chip row as a band: it holds under the bar, paints its own surface,
    // lets go at the end of the discography and joins the reveal seam.
    // -----------------------------------------------------------------------

    /// Eight releases over four kinds, two tracks each: a discography long
    /// enough to scroll and mixed enough to offer every chip.
    fn eight() -> Vec<Release> {
        [
            ("Album One", ReleaseKind::Album),
            ("An EP", ReleaseKind::Ep),
            ("Album Two", ReleaseKind::Album),
            ("A Single", ReleaseKind::Single),
            ("Album Three", ReleaseKind::Album),
            ("Another EP", ReleaseKind::Ep),
            ("Another Single", ReleaseKind::Single),
            ("A Compilation", ReleaseKind::Compilation),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (name, kind))| {
            let uri = format!("spotify:release:r{index}");
            Release {
                uri: uri.clone(),
                name: name.to_owned(),
                kind,
                year: Some(2024),
                cover: None,
                // The track's own URI is the row's title, so a row can be named
                // without depending on the order the store happened to ask.
                tracks: vec![format!("T{index}.1"), format!("T{index}.2")],
            }
        })
        .collect()
    }

    /// Answers every release's label request, walking the page so each block is
    /// drawn and asked for, then walks back up to where the band still rests.
    ///
    /// The store only accepts a reply for a release it has asked about, so a
    /// label cannot simply be handed over for every release up front.
    fn label_every_release(
        ctx: &Context,
        size: Vec2,
        releases: &[Release],
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
        art: &mut Art,
    ) {
        let mut labelled = 0;
        let mut output = frame(ctx, size, Vec::new(), library, transport, queue, art).1;
        for _ in 0..400 {
            for command in library.releases.requests() {
                let Command::LoadReleaseTracks { release, tracks } = command else {
                    continue;
                };
                labelled += 1;
                let tracks = tracks
                    .iter()
                    .map(|uri| track(uri, uri, "Vela Moreno", 200_000))
                    .collect();
                library.apply(&Update::ReleaseTracks { release, tracks });
            }
            if labelled >= releases.len() {
                break;
            }
            output = wheel(ctx, size, -extent::ROW_H, library, transport, queue, art);
        }
        assert_eq!(
            labelled,
            releases.len(),
            "not every release was drawn and labelled"
        );
        // Back to where the band is in its slot, so the caller can scroll down
        // into the band it is about.
        for _ in 0..400 {
            if band_rect(&output).is_none() {
                return;
            }
            output = wheel(ctx, size, extent::ROW_H, library, transport, queue, art);
        }
        panic!("the page never scrolled back above the band");
    }

    /// A page on [`ARTIST`] with the discography and a likeness row that is ready
    /// to draw: the row the band must not band over.
    fn with_fans(releases: Vec<Release>) -> Library {
        let mut detail = detail(Vec::new(), releases);
        detail.related = vec!["spotify:artist:other".to_owned()];
        let mut library = library_with(detail);
        let _ = library.related_request();
        library.apply(&Update::Related {
            artist: ARTIST.to_owned(),
            cards: vec![FollowedArtist {
                uri: "spotify:artist:other".to_owned(),
                name: "Other Artist".to_owned(),
                named: true,
                portrait: None,
                releases: None,
            }],
        });
        library
    }

    /// The chip row's own surface, recovered from the fill it paints. The bar's
    /// fill is the only other one on the page, and it is the bar's height.
    fn band_rect(output: &egui::FullOutput) -> Option<Rect> {
        let tokens = Tokens::DARK;
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape)
                    if shape.fill == tokens.palette.surface
                        && (shape.rect.height() - crate::theme::actionbar::STUCK_H).abs()
                            > 0.01 =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
    }

    /// The rect a shape paints into.
    fn shape_rect(shape: &egui::epaint::Shape) -> Option<Rect> {
        match shape {
            egui::epaint::Shape::Rect(rect) => Some(rect.rect),
            egui::epaint::Shape::Text(text) => {
                Some(Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        }
    }

    /// The shape painting `label`, with the clip it was drawn through.
    fn clipped_text(output: &egui::FullOutput, label: &str) -> Option<Rect> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                    Some(clipped.clip_rect)
                }
                _ => None,
            })
    }

    /// The band's hairline: the edge it wears while it is out of its slot.
    fn band_edge(output: &egui::FullOutput, band: Rect) -> Option<Rect> {
        let tokens = Tokens::DARK;
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape)
                    if shape.fill == tokens.palette.border
                        && (shape.rect.top() - (band.bottom() - stroke::HAIRLINE)).abs() < 0.01
                        && (shape.rect.width() - band.width()).abs() < 0.01 =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
    }

    /// The name of a track row of the fixture, if `label` is one.
    fn row_title(label: &str) -> bool {
        label.strip_prefix('T').is_some_and(|tail| {
            !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit() || c == '.')
        })
    }

    /// Wheels the page until the chip row holds under the bar, and answers the
    /// frame it held on.
    fn held_frame(
        ctx: &Context,
        size: Vec2,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
        art: &mut Art,
    ) -> egui::FullOutput {
        let mut output = frame(ctx, size, Vec::new(), library, transport, queue, art).1;
        for _ in 0..400 {
            if let (Some(bar), Some(band)) = (bar_rect(&output), band_rect(&output))
                && (band.top() - bar.bottom()).abs() < 0.5
            {
                return output;
            }
            output = wheel(ctx, size, -space::S4, library, transport, queue, art);
        }
        panic!("the chip row never held under the bar");
    }

    /// Wheels on until the chip row lets go of the floor and rides the end of the
    /// discography, with enough of it below the bar to be seen.
    fn released_frame(
        ctx: &Context,
        size: Vec2,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
        art: &mut Art,
    ) -> egui::FullOutput {
        let mut output = frame(ctx, size, Vec::new(), library, transport, queue, art).1;
        for _ in 0..800 {
            if let (Some(bar), Some(band)) = (bar_rect(&output), band_rect(&output))
                && band.top() < bar.bottom() - 0.5
                && band.bottom() > bar.bottom() + 2.0
            {
                return output;
            }
            output = wheel(ctx, size, -space::S2, library, transport, queue, art);
        }
        panic!("the chip row never let go of the floor");
    }

    #[test]
    fn the_wheel_pins_the_artist_bar() {
        let ctx = context();
        let size = Vec2::new(900.0, 600.0);
        let mut library = library_with(detail(Vec::new(), eight()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let mut output = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        )
        .1;
        assert!(
            bar_rect(&output).is_none(),
            "nothing is pinned at the top of the page"
        );
        for _ in 0..40 {
            if bar_rect(&output).is_some() {
                break;
            }
            output = wheel(
                &ctx,
                size,
                -extent::ROW_H,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
        }
        assert!(
            bar_rect(&output).is_some(),
            "the wheel scrolls the artist page and its bar pins"
        );
    }

    #[test]
    fn the_artist_bar_pins_at_the_top_with_the_name_beside_play() {
        let ctx = context();
        let size = Vec2::new(900.0, 600.0);
        let mut library = library_with(detail(Vec::new(), eight()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let output = held_frame(
            &ctx,
            size,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let bar = bar_rect(&output).expect("the action row is pinned");
        assert!(
            (bar.top() - space::S3).abs() < 0.01,
            "the row pins at the viewport's top: {}",
            bar.top()
        );
        let name = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == "Vela Moreno" => {
                    Some(Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
            .find(|rect| bar.contains_rect(*rect))
            .expect("the pinned row names the artist");
        assert!(
            name.left() > bar.left() + hero::PLAY,
            "the name stands beside the play tile: {name:?} in {bar:?}"
        );
    }

    #[test]
    fn the_chip_row_pins_at_sixty_under_the_bar() {
        let ctx = context();
        let size = Vec2::new(900.0, 600.0);
        let mut library = library_with(detail(Vec::new(), eight()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let output = held_frame(
            &ctx,
            size,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let bar = bar_rect(&output).expect("the action row is pinned");
        let band = band_rect(&output).expect("the chip row is filled while it holds");

        assert!(
            (band.top() - bar.bottom()).abs() < 0.01,
            "the band starts where the bar ends: {} against {}",
            band.top(),
            bar.bottom()
        );
        assert!(
            (band.width() - size.x).abs() < 0.01 && band.left().abs() < 0.01,
            "the surface bleeds to both pane edges: {band:?}"
        );
        let all = chip_rect(&output, Chip::All).expect("the All chip is drawn at the floor");
        assert!(
            all.left() >= space::GUTTER - 0.01 && all.top() >= band.top() - 0.01,
            "the chips sit inside the surface, not off its edge: {all:?} in {band:?}"
        );
        assert!(
            band_edge(&output, band).is_some(),
            "the held band wears an edge at its bottom"
        );

        // The surface is painted before the chips, because the rows sliding under
        // the band are painted after it: anything left clear would show their ink.
        let (surface, surface_clip) = output
            .shapes
            .iter()
            .enumerate()
            .find_map(|(index, clipped)| match &clipped.shape {
                egui::epaint::Shape::Rect(shape)
                    if shape.fill == Tokens::DARK.palette.surface
                        && (shape.rect.height() - band.height()).abs() < 0.01 =>
                {
                    Some((index, clipped.clip_rect))
                }
                _ => None,
            })
            .expect("the band's surface is in the frame");
        assert!(
            surface_clip.width() >= band.width() - 0.01,
            "the surface is painted through the gutter's clip, not the pane's: {surface_clip:?}"
        );
        let first_chip = output
            .shapes
            .iter()
            .position(|clipped| {
                matches!(&clipped.shape, egui::epaint::Shape::Text(text) if text.galley.text() == "ALL")
            })
            .expect("the All chip's label is in the frame");
        assert!(
            surface < first_chip,
            "the surface is painted before the chips: {surface} against {first_chip}"
        );
    }

    #[test]
    fn the_row_rests_in_its_slot_before_it_reaches_the_bar() {
        let ctx = context();
        let size = Vec2::new(900.0, 600.0);
        let mut library = library_with(detail(Vec::new(), eight()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let output = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        )
        .1;
        assert!(
            band_rect(&output).is_none(),
            "a row in its slot paints no surface of its own"
        );
        let head = text_rect(&output, "Discography").expect("the section head");
        let chip = chip_rect(&output, Chip::All).expect("the All chip");
        let first = text_rect(&output, "Album One").expect("the first block");
        assert!(
            head.bottom() < chip.top() && chip.bottom() < first.top(),
            "the row keeps the slot between the head and the blocks: {chip:?}"
        );
        assert_eq!(
            pin::inset(&ctx),
            0.0,
            "a row in its slot speaks for nothing"
        );
    }

    #[test]
    fn rows_never_paint_under_the_held_band() {
        let ctx = context();
        let size = Vec2::new(900.0, 600.0);
        let mut library = library_with(detail(Vec::new(), eight()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let output = held_frame(
            &ctx,
            size,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let band = band_rect(&output).expect("the chip row is held");

        let mut drawn = 0;
        for clipped in &output.shapes {
            let egui::epaint::Shape::Text(text) = &clipped.shape else {
                continue;
            };
            let label = text.galley.text();
            if !eight().iter().any(|release| release.name == label) {
                continue;
            }
            drawn += 1;
            assert!(
                clipped.clip_rect.top() >= band.bottom() - 0.01,
                "{label} is drawn through a clip that starts above the band: {:?}",
                clipped.clip_rect
            );
        }
        assert!(drawn > 0, "a block is drawn under the held band");
    }

    #[test]
    fn a_press_on_a_chip_while_held_chooses_it_and_reaches_no_row() {
        let ctx = context();
        let size = Vec2::new(900.0, 600.0);
        let releases = eight();
        let mut library = library_with(detail(Vec::new(), releases.clone()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        label_every_release(
            &ctx,
            size,
            &releases,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let output = held_frame(
            &ctx,
            size,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let albums = chip_rect(&output, Chip::Kind(ReleaseKind::Album))
            .expect("the Albums chip at the floor");

        let command = click(
            &ctx,
            size,
            albums.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(command, None, "a filter press asks for nothing");
        assert_eq!(
            library.filter.chosen(),
            Chip::Kind(ReleaseKind::Album),
            "the press on the held band chooses the chip"
        );
        for release in &releases {
            assert!(
                !release_picked(&ctx, &release.uri, 0),
                "the press fell through the band onto a row"
            );
        }
    }

    #[test]
    fn the_band_releases_at_the_end_of_the_discography() {
        let ctx = context();
        let size = Vec2::new(900.0, 420.0);
        let mut library = with_fans(eight());
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let output = released_frame(
            &ctx,
            size,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let band = band_rect(&output).expect("the chip row rides the end");
        let fans = text_rect(&output, "Fans also like").expect("the likeness head is drawn");
        assert!(
            (band.bottom() - (fans.top() - space::S2 - space::S6)).abs() < 0.01,
            "the band ends where the discography ends: {} against {}",
            band.bottom(),
            fans.top() - space::S2 - space::S6
        );
    }

    #[test]
    fn rows_above_the_released_band_stay_visible_and_fans_is_not_clipped() {
        let ctx = context();
        let size = Vec2::new(900.0, 420.0);
        let releases = eight();
        let mut library = with_fans(releases.clone());
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        label_every_release(
            &ctx,
            size,
            &releases,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let output = released_frame(
            &ctx,
            size,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let bar = bar_rect(&output).expect("the action row is pinned");
        let band = band_rect(&output).expect("the chip row rides the end");
        assert!(
            band.top() < bar.bottom() - 0.5,
            "the band is riding the end, not holding the floor"
        );

        // No row of the discography paints inside the strip the riding band
        // covers: the body is drawn through a clip that stops above it.
        let fixture = eight();
        let names: Vec<&str> = fixture
            .iter()
            .map(|release| release.name.as_str())
            .collect();
        for clipped in &output.shapes {
            let Some(rect) = shape_rect(&clipped.shape) else {
                continue;
            };
            let egui::epaint::Shape::Text(text) = &clipped.shape else {
                continue;
            };
            let label = text.galley.text();
            if !names.contains(&label) && !row_title(label) {
                continue;
            }
            assert!(
                rect.bottom() <= band.top() + 0.01 || rect.top() >= band.bottom() - 0.01,
                "{label} paints inside the riding band: {rect:?} against {band:?}"
            );
        }

        // The likeness row below it is drawn whole: the body's clip went back
        // before the section that is not part of the discography.
        let fans = text_rect(&output, "Fans also like").expect("the likeness head is drawn");
        let clip = clipped_text(&output, "Fans also like").expect("the likeness head has a clip");
        assert!(
            clip.contains_rect(fans),
            "the likeness head is clipped away: {fans:?} through {clip:?}"
        );
        let card = text_rect(&output, "Other Artist").expect("the card is drawn");
        let clip = clipped_text(&output, "Other Artist").expect("the card has a clip");
        assert!(
            clip.contains_rect(card),
            "the likeness card is clipped away: {card:?} through {clip:?}"
        );
    }

    #[test]
    fn the_hairline_stays_while_the_band_rides_up() {
        let ctx = context();
        let size = Vec2::new(900.0, 420.0);
        let mut library = with_fans(eight());
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let output = released_frame(
            &ctx,
            size,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let band = band_rect(&output).expect("the chip row rides the end");
        assert!(
            band_edge(&output, band).is_some(),
            "a band out of its slot keeps the edge that says rows pass under it"
        );
    }

    #[test]
    fn the_inset_is_the_lowest_bottom() {
        let ctx = context();
        let size = Vec2::new(900.0, 600.0);
        let mut library = library_with(detail(Vec::new(), eight()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let output = held_frame(
            &ctx,
            size,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let bar = bar_rect(&output).expect("the action row is pinned");
        let band = band_rect(&output).expect("the chip row is held");
        assert!(
            (pin::inset(&ctx) - (band.bottom() - bar.top())).abs() < 0.01,
            "the seam holds the lowest band's bottom: {} against {}",
            pin::inset(&ctx),
            band.bottom() - bar.top()
        );
    }

    #[test]
    fn a_focused_release_row_comes_out_from_under_the_held_band() {
        let ctx = context();
        let size = Vec2::new(900.0, 600.0);
        let releases = eight();
        let mut library = library_with(detail(Vec::new(), releases.clone()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        label_every_release(
            &ctx,
            size,
            &releases,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let output = held_frame(
            &ctx,
            size,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let band = band_rect(&output).expect("the chip row is held");

        // The first row drawn wholly below the band, so the press lands on the row
        // and not in the strip the band owns.
        let (label, title) = drawn_texts(&output)
            .into_iter()
            .filter(|text| row_title(text))
            .filter_map(|text| text_rect(&output, text).map(|rect| (text.to_owned(), rect)))
            .find(|(_, rect)| rect.top() >= band.bottom() + 1.0)
            .expect("a row is drawn below the held band");
        let _ = click(
            &ctx,
            size,
            Pos2::new(title.left() + 2.0, title.center().y),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let focused = ctx
            .memory(|memory| memory.focused())
            .expect("the row took the keyboard");

        // Walk the row up until its top is inside the strip the band covers, then
        // let what the reveal asked for settle.
        let mut covered = false;
        for _ in 0..120 {
            let row = ctx.read_response(focused).expect("the row is drawn").rect;
            if row.top() < band.bottom() - 0.5 {
                covered = true;
                break;
            }
            let _ = wheel(
                &ctx,
                size,
                -space::S2,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
        }
        assert!(covered, "the row never slid under the held band");

        let mut output = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        )
        .1;
        let mut settled = false;
        for _ in 0..240 {
            if let Some(row) = ctx.read_response(focused)
                && row.rect.top() >= band.bottom() - 0.01
            {
                settled = true;
                break;
            }
            output = frame(
                &ctx,
                size,
                Vec::new(),
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            )
            .1;
        }
        assert_eq!(
            ctx.memory(|memory| memory.focused()),
            Some(focused),
            "the row keeps the keyboard"
        );
        assert!(
            settled,
            "the focused row never came out from under the held band: {label}"
        );
        let band = band_rect(&output).expect("the chip row is still held");
        let row = ctx
            .read_response(focused)
            .expect("the row is drawn again")
            .rect;
        assert!(
            row.top() >= band.bottom() - 0.01,
            "the focused row rests below the band: {row:?} against {band:?}"
        );
    }

    #[test]
    fn a_focused_row_in_the_last_block_asks_for_no_scroll_while_the_band_is_released() {
        let ctx = context();
        let size = Vec2::new(900.0, 420.0);
        let releases = eight();
        let mut library = with_fans(releases.clone());
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        label_every_release(
            &ctx,
            size,
            &releases,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        // The last row of the last block, drawn below the band while it still
        // holds the floor: the row a reader would reach for at the end.
        let mut output = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        )
        .1;
        let mut last = None;
        for _ in 0..800 {
            let Some(band) = band_rect(&output) else {
                output = wheel(
                    &ctx,
                    size,
                    -space::S2,
                    &mut library,
                    &mut transport,
                    &mut queue,
                    &mut art,
                );
                continue;
            };
            if let Some(rect) = text_rect(&output, "T7.2")
                && rect.top() >= band.bottom() + 1.0
                && rect.bottom() <= size.y - 1.0
            {
                last = Some(rect);
                break;
            }
            output = wheel(
                &ctx,
                size,
                -space::S2,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
        }
        let title = last.expect("the last row is drawn below the held band");
        let _ = click(
            &ctx,
            size,
            Pos2::new(title.left() + 2.0, title.center().y),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let focused = ctx
            .memory(|memory| memory.focused())
            .expect("the last row took the keyboard");

        // Scroll on until the discography's end carries the band up: the row is
        // behind the bar now, and a band measured from the viewport's top would
        // drag the page upward every frame.
        let output = released_frame(
            &ctx,
            size,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let bar = bar_rect(&output).expect("the action row is pinned");
        let band = band_rect(&output).expect("the chip row rides the end");
        assert!(
            band.top() < bar.bottom() - 0.5,
            "the band is riding the end, not holding the floor"
        );
        assert_eq!(
            pin::inset(&ctx),
            crate::theme::actionbar::STUCK_H,
            "a released band leaves the seam to the bar"
        );

        // The page does not move on its own: the row it still holds the keyboard
        // for asks for nothing. Let whatever the last wheel left pending settle
        // first, so the comparison is of a quiet page.
        let mut output = output;
        for _ in 0..6 {
            output = frame(
                &ctx,
                size,
                Vec::new(),
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            )
            .1;
        }
        let fans = text_rect(&output, "Fans also like").expect("the likeness head is drawn");
        let mut still = output;
        for _ in 0..8 {
            still = frame(
                &ctx,
                size,
                Vec::new(),
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            )
            .1;
        }
        let moved = text_rect(&still, "Fans also like").expect("the likeness head is still drawn");
        assert!(
            (moved.top() - fans.top()).abs() < 0.01,
            "the page moved while a row was focused under a released band: {} against {}",
            moved.top(),
            fans.top()
        );
        assert!(
            ctx.read_response(focused)
                .is_none_or(|row| row.rect.top() >= 0.0),
            "the focused row was dragged above the viewport"
        );
    }

    #[test]
    fn a_three_line_band_at_a_narrow_pane_covers_the_last_row() {
        let ctx = context();
        // Narrow enough that the five chips need three lines: the room a 240-wide
        // cluster leaves beside them on a rail-width window, where the band's own
        // height is the trailing step of the last block and then some.
        let size = Vec2::new(200.0, 380.0);
        let mut library = with_fans(eight());
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let output = released_frame(
            &ctx,
            size,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let band = band_rect(&output).expect("the chip row rides the end");

        let mut tops: Vec<f32> = [
            Chip::All,
            Chip::Kind(ReleaseKind::Album),
            Chip::Kind(ReleaseKind::Ep),
            Chip::Kind(ReleaseKind::Single),
            Chip::Kind(ReleaseKind::Compilation),
        ]
        .into_iter()
        .filter_map(|chip| chip_rect(&output, chip))
        .map(|rect| rect.top())
        .collect();
        tops.dedup_by(|a, b| (*a - *b).abs() < 0.01);
        assert!(
            tops.len() >= 3,
            "the band wraps to three lines at {size:?}: {tops:?}"
        );

        // The last row of the last block sits wholly inside the band's rect: a
        // wrapped band is taller than the step it rides, so it covers the row
        // that would otherwise be the last thing on the page.
        let end = band.bottom();
        let row_top = end - space::S6 - extent::ROW_H;
        let row_bottom = end - space::S6;
        assert!(
            band.top() <= row_top && row_bottom <= band.bottom(),
            "the wrapped band covers the last row: {band:?} against {row_top}..{row_bottom}"
        );
    }

    #[test]
    fn a_page_with_no_band_leaves_the_bar_inset_alone() {
        let ctx = context();
        let size = Vec2::new(900.0, 600.0);
        // A genuinely empty discography draws no row and no band; a tall
        // likeness grid gives the page enough after it to scroll the bar's row
        // past the floor.
        let mut detail = detail(Vec::new(), Vec::new());
        detail.related = (0..12)
            .map(|index| format!("spotify:artist:other{index}"))
            .collect();
        let mut library = library_with(detail);
        let _ = library.related_request();
        library.apply(&Update::Related {
            artist: ARTIST.to_owned(),
            cards: (0..12)
                .map(|index| FollowedArtist {
                    uri: format!("spotify:artist:other{index}"),
                    name: format!("Other Artist {index}"),
                    named: true,
                    portrait: None,
                    releases: None,
                })
                .collect(),
        });
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let mut output = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        )
        .1;
        // To the foot of the page, where an empty discography's row would hold
        // the floor if it had one.
        for _ in 0..200 {
            output = wheel(
                &ctx,
                size,
                -extent::ROW_H,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
        }
        let bar = bar_rect(&output).expect("the action row pins without a band under it");
        assert!(
            band_rect(&output).is_none(),
            "an empty discography draws no row, so nothing holds the floor"
        );
        assert_eq!(
            pin::inset(&ctx),
            crate::theme::actionbar::STUCK_H,
            "a page with no band leaves the seam to the bar"
        );
        let _ = bar;
    }

    /// The field's box in the page: flush with the row's far edge, on the chip
    /// line the cluster shares.
    fn field_rect(output: &egui::FullOutput, size: Vec2) -> Rect {
        let chip = chip_rect(output, Chip::All).expect("the All chip is drawn");
        Rect::from_min_size(
            pos2(
                size.x - space::GUTTER - crate::theme::cluster::FIELD_W,
                chip.center().y - crate::theme::cluster::FIELD_H / 2.0,
            ),
            vec2(
                crate::theme::cluster::FIELD_W,
                crate::theme::cluster::FIELD_H,
            ),
        )
    }

    /// Types `text` into the page's release field, returning the frame the
    /// keystrokes landed on.
    fn type_into_field(
        ctx: &Context,
        size: Vec2,
        text: &str,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
        art: &mut Art,
    ) -> egui::FullOutput {
        let (_, output) = frame(ctx, size, Vec::new(), library, transport, queue, art);
        let field = field_rect(&output, size);
        for event in click_events(field.center()) {
            let _ = frame(ctx, size, vec![event], library, transport, queue, art);
        }
        // The field's own key filter is stored for the next pass, so the text
        // event belongs to a frame that already knows the field has the
        // keyboard.
        let _ = frame(ctx, size, Vec::new(), library, transport, queue, art);
        frame(
            ctx,
            size,
            vec![Event::Text(text.to_owned())],
            library,
            transport,
            queue,
            art,
        )
        .1
    }

    fn repaint_delay(output: &egui::FullOutput) -> std::time::Duration {
        output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map_or(std::time::Duration::MAX, |viewport| viewport.repaint_delay)
    }

    #[test]
    fn the_field_sits_flush_right_in_the_chip_row_240_wide() {
        let ctx = context();
        let mut library = library_with(detail(Vec::new(), mixed()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let size = PAGE;
        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let field = field_rect(&output, size);
        assert_eq!(field.width(), crate::theme::cluster::FIELD_W);
        assert_eq!(field.height(), crate::theme::cluster::FIELD_H);
        assert!(
            (field.right() - (size.x - space::GUTTER)).abs() < 0.01,
            "the field is flush with the row's far edge: {field:?}"
        );
        assert!(drawn(&output, "FILTER"), "{:?}", drawn_texts(&output));
    }

    #[test]
    fn typing_narrows_by_name_on_top_of_the_chip() {
        let ctx = context();
        let size = PAGE;
        let mut library = library_with(detail(Vec::new(), mixed()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let output = type_into_field(
            &ctx,
            size,
            "album",
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(library.filter.needle().as_deref(), Some("album"));
        assert!(drawn(&output, "Album One"), "{:?}", drawn_texts(&output));
        assert!(drawn(&output, "Album Two"), "{:?}", drawn_texts(&output));
        assert!(
            !drawn(&output, "An EP"),
            "the name axis composes with the chip: {:?}",
            drawn_texts(&output)
        );

        library.filter.choose(Chip::Kind(ReleaseKind::Ep));
        let (_, after) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(
            !drawn(&after, "An EP"),
            "the album query still narrows the EP chip: {:?}",
            drawn_texts(&after)
        );
    }

    #[test]
    fn the_count_reads_n_of_m_over_all_releases_and_only_while_typing() {
        let ctx = context();
        let size = PAGE;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let mut albums = library_with(detail(Vec::new(), mixed()));
        albums.filter.choose(Chip::Kind(ReleaseKind::Album));
        albums.filter.edit().push_str("album");
        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut albums,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(
            drawn(&output, "2 OF 3 RELEASES"),
            "the count is shown over every release, not the chip's subset: {:?}",
            drawn_texts(&output)
        );

        let mut eps = library_with(detail(Vec::new(), mixed()));
        eps.filter.choose(Chip::Kind(ReleaseKind::Ep));
        eps.filter.edit().push_str("album");
        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut eps,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(
            drawn(&output, "0 OF 3 RELEASES"),
            "an empty slice of the whole still counts the whole: {:?}",
            drawn_texts(&output)
        );

        let mut quiet = library_with(detail(Vec::new(), mixed()));
        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut quiet,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(
            !drawn_texts(&output)
                .iter()
                .any(|text| text.contains(" OF 3 RELEASES")),
            "no query, no count: {:?}",
            drawn_texts(&output)
        );
    }

    #[test]
    fn the_count_paints_inside_the_band_while_it_holds() {
        let ctx = context();
        let size = Vec2::new(900.0, 420.0);
        let mut library = with_fans(eight());
        library.filter.edit().push_str("album");
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let mut output = held_frame(
            &ctx,
            size,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        // Scroll on while the band keeps the floor: the count has to follow the
        // painted band, not the slot it left behind well above it.
        let bar = bar_rect(&output).expect("the action row is pinned");
        for _ in 0..8 {
            let next = wheel(
                &ctx,
                size,
                -space::S4,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
            let Some(band) = band_rect(&next) else {
                break;
            };
            if (band.top() - bar.bottom()).abs() > 0.5 {
                break;
            }
            output = next;
        }
        let band = band_rect(&output).expect("the chip row holds under the bar");
        let count = text_rect(&output, "3 OF 8 RELEASES").expect("the count is painted");
        assert!(
            band.contains_rect(count),
            "the count moves with the held band: {count:?} outside {band:?}"
        );
    }

    #[test]
    fn the_count_paints_inside_the_band_while_it_releases() {
        let ctx = context();
        let size = Vec2::new(900.0, 420.0);
        let mut library = with_fans(eight());
        library.filter.edit().push_str("album");
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let output = released_frame(
            &ctx,
            size,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let band = band_rect(&output).expect("the chip row rides the end of the discography");
        let count = text_rect(&output, "3 OF 8 RELEASES").expect("the count is painted");
        assert!(
            band.contains_rect(count),
            "the count rides the released band: {count:?} outside {band:?}"
        );
    }

    #[test]
    fn the_status_node_is_polite_and_exists_only_while_typing() {
        let ctx = context();
        ctx.enable_accesskit();
        let size = PAGE;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let tokens = Tokens::DARK;

        let mut library = library_with(detail(Vec::new(), mixed()));
        library.filter.edit().push_str("album");
        let mut read = None;
        let _ = run(&ctx, input(size, Vec::new()), |ui| {
            crate::scroll::instant(ui.ctx());
            ui.add_space(space::S3);
            let _ = artist_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
                Instant::now(),
                &tokens,
            );
            read = ui
                .ctx()
                .accesskit_node_builder(egui::Id::unique("release-count"), |node| {
                    (node.role(), node.live(), node.label().map(str::to_owned))
                });
        });
        let read = read.expect("the count has a status node");
        assert_eq!(read.0, egui::Role::Status);
        assert_eq!(read.1, Some(egui::accesskit::Live::Polite));
        assert_eq!(read.2.as_deref(), Some("2 of 3 releases"));

        let mut quiet = library_with(detail(Vec::new(), mixed()));
        let mut stray = false;
        let _ = run(&ctx, input(size, Vec::new()), |ui| {
            crate::scroll::instant(ui.ctx());
            ui.add_space(space::S3);
            let _ = artist_view(
                ui,
                Table::Full,
                &mut quiet,
                &mut transport,
                &mut queue,
                &mut art,
                Instant::now(),
                &tokens,
            );
            stray = ui
                .ctx()
                .has_accesskit_node(egui::Id::unique("release-count"));
        });
        assert!(!stray, "an unfiltered discography announces nothing");
    }

    #[test]
    fn the_section_head_keeps_its_totals_while_typing() {
        let ctx = context();
        let size = PAGE;
        let mut library = library_with(detail(Vec::new(), mixed()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let output = type_into_field(
            &ctx,
            size,
            "album",
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let aside = discography_meta(3, 3).to_uppercase();
        assert!(
            drawn(&output, &aside),
            "the section head describes the artist, not the query: {aside}"
        );
    }

    #[test]
    fn the_field_is_a_tab_stop_after_the_chips_and_before_the_blocks() {
        let ctx = context();
        let size = PAGE;
        let mut library = library_with(detail(Vec::new(), eight()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let field = field_rect(&output, size);
        for event in click_events(field.center()) {
            let _ = frame(
                &ctx,
                size,
                vec![event],
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
        }
        let _ = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(ctx.text_edit_focused(), "the field took the keyboard");

        // Forward, the next stop is a row below the field: the field comes
        // before the blocks.
        let [press, release] = crate::view::test_support::tab();
        let _ = frame(
            &ctx,
            size,
            vec![press.clone()],
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let _ = frame(
            &ctx,
            size,
            vec![release.clone()],
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let focused = ctx
            .memory(|memory| memory.focused())
            .expect("Tab left the field for a control");
        let row = ctx
            .read_response(focused)
            .expect("the focused control is drawn")
            .rect;
        assert!(
            !ctx.text_edit_focused() && row.top() >= field.bottom() - 0.01,
            "the field comes before the blocks: {row:?} against {field:?}"
        );

        // Backwards, the next stop is a chip on the field's own line: the field
        // comes after the chips.
        for event in click_events(field.center()) {
            let _ = frame(
                &ctx,
                size,
                vec![event],
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
        }
        let _ = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(ctx.text_edit_focused(), "the field took the keyboard");
        let back = |pressed| Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::SHIFT,
        };
        let _ = frame(
            &ctx,
            size,
            vec![back(true)],
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let _ = frame(
            &ctx,
            size,
            vec![back(false)],
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let focused = ctx
            .memory(|memory| memory.focused())
            .expect("Shift+Tab left the field for a control");
        let chip = ctx
            .read_response(focused)
            .expect("the focused control is drawn")
            .rect;
        assert!(
            chip.top() < field.bottom() && chip.bottom() > field.top(),
            "the field comes after the chips: {chip:?} against {field:?}"
        );
    }

    #[test]
    fn the_field_keeps_its_focus_and_text_across_the_pin() {
        let ctx = context();
        let size = Vec2::new(900.0, 500.0);
        let mut library = library_with(detail(Vec::new(), eight()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let _ = type_into_field(
            &ctx,
            size,
            "album",
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(ctx.text_edit_focused(), "the field took the keyboard");
        let output = held_frame(
            &ctx,
            size,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(
            drawn(&output, "album"),
            "the field's text survives the pin: {:?}",
            drawn_texts(&output)
        );
        assert!(
            ctx.text_edit_focused(),
            "the field keeps the keyboard across the pin"
        );
    }

    #[test]
    fn the_rule_follows_the_row() {
        let ctx = context();
        let size = Vec2::new(900.0, 500.0);
        let mut library = with_fans(eight());
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let tokens = Tokens::DARK;

        let (_, resting) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let field = field_rect(&resting, size);
        let line = |output: &egui::FullOutput, field: Rect| {
            output.shapes.iter().any(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape)
                    if (shape.rect.height() - stroke::HAIRLINE).abs() < 0.01
                        && (shape.rect.width() - field.width()).abs() < 0.01
                        && (shape.rect.bottom() - field.bottom()).abs() < 0.01 =>
                {
                    shape.fill == tokens.palette.border || shape.fill == tokens.palette.text
                }
                _ => false,
            })
        };
        assert!(line(&resting, field), "a resting row underlines its field");

        let held = held_frame(
            &ctx,
            size,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let chip = chip_rect(&held, Chip::All).expect("the All chip is drawn at the floor");
        let held_field = Rect::from_min_size(
            pos2(
                size.x - space::GUTTER - crate::theme::cluster::FIELD_W,
                chip.center().y - crate::theme::cluster::FIELD_H / 2.0,
            ),
            vec2(
                crate::theme::cluster::FIELD_W,
                crate::theme::cluster::FIELD_H,
            ),
        );
        assert!(
            !line(&held, held_field),
            "a held row gives the field's rule to its own hairline"
        );

        let released = {
            let ctx = context();
            let size = Vec2::new(900.0, 420.0);
            let mut library = with_fans(eight());
            let mut transport = Transport::default();
            let mut queue = Queue::default();
            let mut art = Art::default();
            let output = released_frame(
                &ctx,
                size,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
            let chip = chip_rect(&output, Chip::All).expect("the All chip rides the end");
            let field = Rect::from_min_size(
                pos2(
                    size.x - space::GUTTER - crate::theme::cluster::FIELD_W,
                    chip.center().y - crate::theme::cluster::FIELD_H / 2.0,
                ),
                vec2(
                    crate::theme::cluster::FIELD_W,
                    crate::theme::cluster::FIELD_H,
                ),
            );
            (output, field)
        };
        assert!(
            !line(&released.0, released.1),
            "a riding row also gives the field's rule to its own hairline"
        );
    }

    #[test]
    fn the_field_does_not_move_when_the_count_arrives() {
        let ctx = context();
        let size = PAGE;
        let mut library = library_with(detail(Vec::new(), mixed()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, before) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let before = field_rect(&before, size);
        library.filter.edit().push_str("album");
        let (_, after) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let after = field_rect(&after, size);
        assert_eq!(before, after, "the count arrives beside the field");
    }

    #[test]
    fn a_steady_page_stops_asking_for_frames_after_an_edit() {
        let ctx = context();
        let size = PAGE;
        let mut library = library_with(detail(Vec::new(), mixed()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        // A quiet page: nothing has been typed, so nothing asks for a frame.
        let mut steady = std::time::Duration::ZERO;
        for _ in 0..8 {
            let (_, output) = frame(
                &ctx,
                size,
                Vec::new(),
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
            steady = repaint_delay(&output);
        }
        assert_ne!(
            steady,
            std::time::Duration::ZERO,
            "a steady page must not look like an edited one"
        );

        // The keystroke narrows the blocks on the frame it lands: the post-edit
        // set is what is drawn. The end and the count were reserved from the
        // pre-edit state, so the page asks for one more frame to settle them.
        let typed = type_into_field(
            &ctx,
            size,
            "album",
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(drawn(&typed, "Album One"), "{:?}", drawn_texts(&typed));
        assert!(
            !drawn(&typed, "An EP"),
            "the blocks use the post-edit set: {:?}",
            drawn_texts(&typed)
        );

        // Escape is the clean test of the page's own request: the field handles
        // the key itself and gives the keyboard back, so nothing else in the row
        // has a reason to ask for the next frame.
        let _ = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let escape = Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let (_, cleared) = frame(
            &ctx,
            size,
            vec![escape],
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(library.filter.needle(), None, "Escape cleared the query");
        assert!(
            drawn(&cleared, "An EP"),
            "the cleared list draws its hidden block again: {:?}",
            drawn_texts(&cleared)
        );
        // The page's own request for the settling frame is deliberately not
        // pinned here: Escape and the focus change request frames on their own,
        // whatever `request_repaint` did, so a ZERO delay on this frame would
        // prove neither.

        // The focus ring and the field's own state take a few frames to stop
        // moving; the page must not keep asking once they have.
        let mut quiet = false;
        for _ in 0..24 {
            let (_, settled) = frame(
                &ctx,
                size,
                Vec::new(),
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
            if repaint_delay(&settled) != std::time::Duration::ZERO {
                quiet = true;
                break;
            }
        }
        assert!(quiet, "the page never stopped asking for frames");
    }

    #[test]
    fn typing_reaches_no_other_control() {
        let ctx = context();
        let size = PAGE;
        let mut library = library_with(detail(Vec::new(), mixed()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let page = library.page().cloned();

        let (command, output) = {
            let _ = frame(
                &ctx,
                size,
                Vec::new(),
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
            let field = field_rect(
                &frame(
                    &ctx,
                    size,
                    Vec::new(),
                    &mut library,
                    &mut transport,
                    &mut queue,
                    &mut art,
                )
                .1,
                size,
            );
            for event in click_events(field.center()) {
                let _ = frame(
                    &ctx,
                    size,
                    vec![event],
                    &mut library,
                    &mut transport,
                    &mut queue,
                    &mut art,
                );
            }
            let _ = frame(
                &ctx,
                size,
                Vec::new(),
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
            frame(
                &ctx,
                size,
                vec![Event::Text("album".to_owned())],
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            )
        };
        assert_eq!(command, None, "typing asks for nothing");
        assert_eq!(library.page(), page.as_ref(), "typing navigates nowhere");
        assert_eq!(library.filter.chosen(), Chip::All);
        assert!(drawn(&output, "Album One"));
        assert!(!drawn(&output, "An EP"));
    }

    #[test]
    fn the_page_paints_no_unaligned_marker_with_a_query() {
        let ctx = context();
        let mut library = library_with(detail(
            Vec::new(),
            vec![
                release("spotify:album:r1", "Slow Tide", &["spotify:track:1"]),
                release("spotify:album:r2", "Kestrel", &["spotify:track:1"]),
            ],
        ));
        library.filter.edit().push_str("slow");
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            Vec2::new(900.0, 800.0),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(
            !drawn_texts(&output).contains(&"Unaligned"),
            "a queried page paints an unaligned widget: {:?}",
            drawn_texts(&output)
        );
    }

    #[test]
    fn the_query_sentence_wins() {
        let ctx = context();
        let size = PAGE;
        let mut library = library_with(detail(Vec::new(), mixed()));
        library.filter.edit().push_str("zzz");
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&"Nothing in this list matches “zzz”"),
            "the query sentence: {texts:?}"
        );
        assert!(
            !texts.contains(&"Nothing in this format yet"),
            "only one of the two sentences is ever on screen: {texts:?}"
        );
        assert!(!texts.contains(&"Album One"), "{texts:?}");
        assert!(!texts.contains(&"An EP"), "{texts:?}");
    }

    #[test]
    fn the_format_empty_state_names_the_artist() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let output = run(&ctx, input(PAGE, Vec::new()), |ui| {
            ui.add_space(space::S3);
            draw_empty(
                ui,
                crate::discography::Emptied::Format,
                "Vela Moreno",
                "",
                &tokens,
            );
        });
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&"Nothing in this format yet"),
            "the format's heading: {texts:?}"
        );
        assert!(
            texts.contains(&"Vela Moreno has no releases of that type."),
            "the format's sentence names the artist: {texts:?}"
        );
    }

    #[test]
    fn the_band_still_releases_over_an_empty_body() {
        let ctx = context();
        let size = Vec2::new(900.0, 420.0);
        let mut library = with_fans(eight());
        library.filter.edit().push_str("zzz");
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let output = released_frame(
            &ctx,
            size,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(
            band_rect(&output).is_some(),
            "the row still rides the empty body"
        );
        assert!(
            drawn(&output, "Nothing in this list matches “zzz”"),
            "the sentence is drawn after the body clip is restored: {:?}",
            drawn_texts(&output)
        );
    }

    /// A track that names the record it is on, which is the cell's credit.
    fn on_album(uri: &str, title: &str, album: &str) -> TrackEntry {
        TrackEntry {
            album: Some(album.to_owned()),
            album_uri: Some(uri.to_owned()),
            ..track("spotify:track:a", title, "Vela Moreno", 200_000)
        }
    }

    #[test]
    fn a_popular_row_asks_for_its_own_cover() {
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        let top = vec![
            TrackEntry {
                cover: Some(cover("a")),
                ..track("spotify:track:a", "One", "Vela Moreno", 222_000)
            },
            TrackEntry {
                cover: Some(cover("b")),
                ..track("spotify:track:b", "Two", "Vela Moreno", 198_000)
            },
        ];
        let mut library = library_with(detail(top, Vec::new()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let now = Instant::now();

        art.begin_frame();
        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let requests = art.requests(now);

        assert_eq!(
            requests.len(),
            2,
            "one lookup per on-screen row: {requests:?}"
        );
        assert!(
            requests
                .iter()
                .all(|command| matches!(command, Command::Fetch { .. })),
            "{requests:?}"
        );

        let (title_cell, _, _) = popular_cells(&ctx, size, 0, 2);
        let name = text_rect(&output, "One").expect("the row's name");
        assert_eq!(
            name.left(),
            title_cell.left() + extent::LIBRARY_ART + space::S3,
            "the name starts past the row's own art box and its gutter"
        );
    }

    #[test]
    fn a_popular_row_opens_its_album() {
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        let mut library = library_with(detail(
            vec![on_album("spotify:album:slow-tide", "One", "Slow Tide")],
            Vec::new(),
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let words = text_rect(&output, "Slow Tide").expect("the credit is painted");
        let command = click(
            &ctx,
            size,
            words.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(
            library.page(),
            Some(&Page::Album("spotify:album:slow-tide".to_owned())),
            "the credit opens the record it names"
        );
        assert_eq!(
            command,
            Some(Command::LoadAlbum {
                album: "spotify:album:slow-tide".to_owned()
            })
        );
        assert!(!queue.can_next(), "opening a page must not start the queue");
    }

    #[test]
    fn a_click_past_the_credit_selects_the_row_and_a_double_click_plays() {
        // The link is the painted words, not the credit cell: a click past them
        // belongs to the row.
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        let mut library = library_with(detail(
            vec![on_album("spotify:album:slow-tide", "One", "Slow Tide")],
            Vec::new(),
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, credit, row) = popular_cells(&ctx, size, 0, 1);
        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let words = text_rect(&output, "Slow Tide").expect("the credit is painted");
        let past = Pos2::new(
            words.right() + (credit.right() - words.right()) / 2.0,
            row.center().y,
        );
        assert!(
            words.right() < past.x && past.x < credit.right(),
            "the probe at x={} must be past the credit, which ends at x={}",
            past.x,
            words.right()
        );

        let command = click(
            &ctx,
            size,
            past,
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(command, None, "a click past the words is the row's own");
        assert_eq!(
            library.page(),
            Some(&Page::Artist(ARTIST.to_owned())),
            "a click past the credit must not navigate"
        );
        assert!(!queue.can_next(), "a click past the words must not play");

        // A fresh page, so the double click is not the third of a sequence.
        let ctx = context();
        let mut library = library_with(detail(
            vec![on_album("spotify:album:slow-tide", "One", "Slow Tide")],
            Vec::new(),
        ));
        let command = click(
            &ctx,
            size,
            past,
            2,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(
            command,
            Some(Command::Play {
                uri: "spotify:track:a".to_owned()
            }),
            "a double click past the credit plays the row"
        );
        assert_eq!(
            library.page(),
            Some(&Page::Artist(ARTIST.to_owned())),
            "playing the row stays on the artist"
        );
    }

    #[test]
    fn a_popular_row_without_an_album_still_plays_on_a_double_click() {
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        // No album on the entry, so the credit cell is empty and takes no press.
        let mut library = library_with(detail(
            vec![track("spotify:track:a", "One", "Vela Moreno", 222_000)],
            Vec::new(),
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, credit, _) = popular_cells(&ctx, size, 0, 1);
        let command = click(
            &ctx,
            size,
            credit.center(),
            2,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "spotify:track:a".to_owned()
            }),
            "an empty credit cell is not a link, so the press reaches the row"
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("spotify:track:a")
        );
    }

    #[test]
    fn an_open_does_not_overwrite_a_same_frame_command() {
        let ctx = context();
        // Keep the focused row's focus while the pointer presses elsewhere, so
        // one frame can answer both the keyboard and the credit's click.
        ctx.options_mut(|options| {
            options.input_options.surrender_focus_on = egui::SurrenderFocusOn::Never;
        });
        let size = Vec2::new(900.0, 800.0);
        let top = vec![
            track("spotify:track:a", "One", "Vela Moreno", 222_000),
            on_album("spotify:album:slow-tide", "Two", "Slow Tide"),
        ];
        let mut library = library_with(detail(top, Vec::new()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        // Both solves run before the focus is taken: a solve is a frame of its
        // own, and a frame that does not draw the focused row drops it.
        let (title, _, _) = popular_cells(&ctx, size, 0, 2);
        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let at = text_rect(&output, "Slow Tide")
            .expect("the second row's credit is painted")
            .center();

        // Focus the first row, so Enter on it asks for a play.
        let _ = click(
            &ctx,
            size,
            title.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(
            ctx.memory(|memory| memory.focused()).is_some(),
            "the row took the keyboard"
        );
        // Press on the second row's credit first, so the release that reports
        // the click lands in the frame that also answers Enter on the first.
        let _ = frame(
            &ctx,
            size,
            vec![
                Event::PointerMoved(at),
                Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let events = vec![
            Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
            Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
            Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ];
        let (command, _) = frame(
            &ctx,
            size,
            events,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "spotify:track:a".to_owned()
            }),
            "the same-frame play is not replaced by the open"
        );
        assert_eq!(
            library.page(),
            Some(&Page::Artist(ARTIST.to_owned())),
            "the deferred open did not navigate"
        );
    }

    #[test]
    fn the_head_names_an_album_column_beside_the_hero_eyebrow() {
        let ctx = context();
        let mut library = library_with(detail(
            vec![track("spotify:track:a", "One", "Vela Moreno", 222_000)],
            Vec::new(),
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            Vec2::new(900.0, 800.0),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts = drawn_texts(&output);

        assert_eq!(
            texts.iter().filter(|text| **text == "ARTIST").count(),
            1,
            "the hero's eyebrow alone: {texts:?}"
        );
        assert_eq!(
            texts.iter().filter(|text| **text == "ALBUM").count(),
            1,
            "the popular table's column heading: {texts:?}"
        );
    }

    #[test]
    fn the_loading_skeleton_and_the_loaded_popular_put_the_title_at_the_same_left() {
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        let mut library = crate::library::Library::default();
        library.open(Page::Artist(ARTIST.to_owned()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let loading = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(rect)
                    if rect.fill == Tokens::DARK.palette.skeleton_rule
                        && (rect.rect.height() - extent::SKELETON_LINE).abs() < 0.01 =>
                {
                    Some(rect.rect)
                }
                _ => None,
            })
            .next()
            .expect("the loading list holds a title bar open");

        let ctx = context();
        let mut library = library_with(detail(
            vec![track("spotify:track:a", "One", "Vela Moreno", 222_000)],
            Vec::new(),
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let loaded = text_rect(&output, "One").expect("the row's name");

        assert_eq!(
            loading.left(),
            loaded.left(),
            "the title does not shift when Popular lands"
        );
    }
}
