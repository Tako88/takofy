//! One results block of cards: the albums, or the artists, a query answered.
//!
//! The block is the reference's `.card-grid`: the page's column widened by one
//! card padding each side, so the cover art lines up with the headings above it
//! while the card's own hover surface still bleeds past the gutter. The cards are
//! `media::tile`'s, the same card the library draws, so an artist is a circle and
//! an album a rounded square without this file deciding anything.
//!
//! Both blocks are drawn from the whole answer's index, never from the rows a
//! neighbouring block shows, and only the cards the viewport can see are looked
//! up: a face is fetched by looking at it, and a card nobody can see must not
//! ask for one.

use std::time::Instant;

use egui::{Rect, Sense, Ui, pos2, vec2};

use crate::cover::{Art, want_px};
use crate::layout::{cards, table as table_layout};
use crate::search::Search;
use crate::search_index::{Band, Index, Kind, PAGE, tracks_phrase};
use crate::sidebar_kind::SidebarKind;
use crate::theme::{Tokens, extent, space};

use super::art;
use super::hero;
use super::list_more;
use super::media;
use super::section;
use super::skeleton;

/// What one card block's controls asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Act {
    /// The card at this index was pressed.
    Open(usize),
    /// The reveal row was taken. `focus` is the card it reveals first, so the
    /// keyboard continues where the pointer went.
    More { focus: egui::Id },
}

/// Draws `band`'s head, cards and reveal row.
pub(crate) fn show(
    ui: &mut Ui,
    band: &Band,
    index: &Index,
    search: &Search,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Option<Act> {
    section::head_spoken(ui, band.heading(), &band.share(), &band.spoken(), tokens);
    block(ui, band, index, search, art, now, tokens).0
}

/// The cards and the reveal row, under a head the caller has already drawn.
///
/// Answers what the block asked for, and the grid it solved: the page ignores
/// the grid, and a test measures the reservation with it.
fn block(
    ui: &mut Ui,
    band: &Band,
    index: &Index,
    search: &Search,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> (Option<Act>, cards::Grid) {
    let grid = solved(ui);
    let reserved = Rect::from_min_size(
        ui.cursor().min,
        vec2(ui.available_width(), grid.height(band.shown)),
    );
    ui.allocate_rect(reserved, Sense::hover());
    let visible = ui.clip_rect();
    let want = want_px(grid.card_w, ui.pixels_per_point());

    let mut act = None;
    for position in 0..band.shown {
        let rect = grid.card_rect(position);
        // A card out of sight is not looked up, so a long block does not fetch
        // faces nobody is going to see.
        if !table_layout::on_screen(visible, rect) {
            continue;
        }
        let pressed = match band.kind {
            Kind::Albums => {
                let Some(album) = index.albums.get(position) else {
                    break;
                };
                let cover = art.look(album.cover.as_ref(), want, now);
                let item = media::Media {
                    uri: &album.uri,
                    name: &album.name,
                    kind: "Album",
                    sub: &album.artist,
                    size: "",
                    round: false,
                    shelf: SidebarKind::Albums,
                    playable: false,
                    cover,
                    picture: hero::Picture::Cover,
                };
                media::tile(ui, rect, &item, tokens)
            }
            Kind::Artists => {
                let Some(artist) = index.artists.get(position) else {
                    break;
                };
                let cover = art::artist_cover(
                    search.portrait(&artist.uri),
                    artist.record_cover.as_ref(),
                    art,
                    want,
                    now,
                );
                let sub = tracks_phrase(artist.tracks);
                let item = media::Media {
                    uri: &artist.uri,
                    name: &artist.name,
                    kind: "Artist",
                    sub: &sub,
                    size: "",
                    round: true,
                    shelf: SidebarKind::Artists,
                    playable: false,
                    cover,
                    picture: hero::Picture::Cover,
                };
                media::tile(ui, rect, &item, tokens)
            }
            // A track's card is drawn from the queue, so it is no tile here.
            Kind::Tracks => None,
        };
        if pressed == Some(media::Press::Open) {
            act = Some(Act::Open(position));
        }
    }

    let label = band.more();
    if list_more::show(ui, reserved, label.as_deref(), space::S2, tokens)
        && let Some(focus) = focus_of(ui, band, index)
    {
        act = Some(Act::More { focus });
    }
    (act, grid)
}

/// The block a loading page reserves for `kind`: the grid its cards will land
/// in, drawn as placeholders.
///
/// The height is the same [`cards::Grid::height`] the answered block reserves, so
/// nothing on the page moves when the answer arrives.
pub(crate) fn loading(ui: &mut Ui, kind: Kind, tokens: &Tokens) {
    let grid = solved(ui);
    let count = match kind {
        Kind::Tracks => 0,
        Kind::Albums => PAGE.albums,
        Kind::Artists => PAGE.artists,
    };
    let block = Rect::from_min_size(
        ui.cursor().min,
        vec2(ui.available_width(), grid.height(count)),
    );
    ui.allocate_rect(block, Sense::hover());
    skeleton::tiles(ui, &grid, count, tokens);
}

/// The area the cards are solved in: the page's column, one card padding wider
/// on each side, so the art lines up with the headings.
fn area(ui: &Ui) -> Rect {
    let cursor = ui.cursor().min;
    Rect::from_min_size(
        pos2(cursor.x - space::S2, cursor.y),
        vec2(ui.available_width() + 2.0 * space::S2, 0.0),
    )
}

/// The grid the cards are laid out in at the current cursor.
fn solved(ui: &Ui) -> cards::Grid {
    cards::grid(
        area(ui),
        extent::CARD_MIN_W,
        space::S3,
        media::extra_height(ui),
    )
}

/// The id of the `band.shown`-th card: the first one a reveal would show.
///
/// It is the id `media::tile` interacts the card under, computed here on the
/// frame of the press so the next frame's card is the one that takes the
/// keyboard.
fn focus_of(ui: &Ui, band: &Band, index: &Index) -> Option<egui::Id> {
    let uri = match band.kind {
        Kind::Albums => &index.albums.get(band.shown)?.uri,
        Kind::Artists => &index.artists.get(band.shown)?.uri,
        Kind::Tracks => return None,
    };
    Some(ui.scope_id().with(("library-media", uri.as_str())))
}

#[cfg(test)]
mod tests;
