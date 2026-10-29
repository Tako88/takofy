//! The because shelf: the artists related to one followed artist.
//!
//! The head names the followed artist, and the row draws the related ones as
//! round artist cards. The cards open the artist's page, and the head carries no
//! count: the shelf is a list of neighbours, not a tally.

use egui::{Sense, Ui};

use crate::cover::want_px;
use crate::format::{HOME_BECAUSE_TITLE, library_kind, media_type};
use crate::home::because::Row;
use crate::layout::table as table_layout;
use crate::library_filter::RootKind;
use crate::route::Page;
use crate::sidebar_kind::SidebarKind;
use crate::theme::Tokens;
use crate::view::playlist::root::{self, MediaDraw};

use super::super::{hero, media, section};
use super::Pressed;

/// Draws the shelf, and nothing until it has something to draw.
pub(super) fn show(
    ui: &mut Ui,
    row: &Row<'_>,
    _now_ms: u64,
    tokens: &Tokens,
    draw: &mut MediaDraw<'_, '_>,
    _out: &mut Pressed,
) {
    section::head(
        ui,
        &format!("{HOME_BECAUSE_TITLE} {}", row.seed_name),
        "",
        tokens,
    );

    let solved = root::media_grid(ui);
    let list = root::grid_rect(ui, &solved, row.cards.len());
    ui.allocate_rect(list, Sense::hover());
    let visible = ui.clip_rect();
    let want = want_px(solved.card_w, ui.pixels_per_point());
    let sub = library_kind(media_type(RootKind::Artists), None);

    for (index, card) in row.cards.iter().enumerate() {
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
            cover: draw.art.look(card.portrait.as_ref(), want, draw.now),
            picture: hero::Picture::Cover,
        };
        if media::tile(ui, rect, &media, tokens) == Some(media::Press::Open) {
            *draw.open = Some(Page::Artist(card.uri.clone()));
        }
    }
}

#[cfg(test)]
mod tests;
