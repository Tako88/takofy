//! The counted shelf: the artists the library holds the most of.

use egui::{Sense, Ui};

use crate::cover::want_px;
use crate::format::{HOME_COUNTED_TITLE, media_type};
use crate::home::counted::Tiles;
use crate::layout::table as table_layout;
use crate::library_filter::RootKind;
use crate::route::Page;
use crate::sidebar_kind::SidebarKind;
use crate::theme::Tokens;
use crate::view::playlist::root::{self, MediaDraw};

use super::super::{hero, media, section};
use super::Pressed;

/// Draws the shelf's head and its tiles, and nothing until it holds an artist.
pub(super) fn show(
    ui: &mut Ui,
    tiles: &Tiles<'_>,
    _now_ms: u64,
    tokens: &Tokens,
    draw: &mut MediaDraw<'_, '_>,
    _out: &mut Pressed,
) {
    if tiles.artists.is_empty() {
        return;
    }
    section::head(ui, HOME_COUNTED_TITLE, "", tokens);
    let solved = root::media_grid(ui);
    let list = root::grid_rect(ui, &solved, tiles.artists.len());
    ui.allocate_rect(list, Sense::hover());
    let visible = ui.clip_rect();
    let want = want_px(solved.card_w, ui.pixels_per_point());
    // An artist's subline is its kind alone: the wire carries no count to put
    // there, and the shelf's own title already says what the order is.
    let kind = media_type(RootKind::Artists);

    for (index, artist) in tiles.artists.iter().enumerate() {
        let rect = solved.card_rect(index);
        if !table_layout::on_screen(visible, rect) {
            continue;
        }
        let words = media::Media {
            uri: artist.uri,
            name: artist.name,
            kind,
            sub: kind,
            size: "",
            round: true,
            shelf: SidebarKind::Artists,
            playable: false,
            cover: draw.art.look(artist.portrait, want, draw.now),
            picture: hero::Picture::Cover,
        };
        if media::tile(ui, rect, &words, tokens) == Some(media::Press::Open) {
            *draw.open = Some(Page::Artist(artist.uri.to_owned()));
        }
    }
}

#[cfg(test)]
mod tests;
