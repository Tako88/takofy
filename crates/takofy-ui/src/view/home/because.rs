//! The because shelf: the artists related to one followed artist.
//!
//! The head names the followed artist, and the row draws the related ones as
//! round artist cards. The cards open the artist's page, and the head carries no
//! count: the shelf is a list of neighbours, not a tally.

use egui::{Sense, Ui};

use crate::format::{HOME_BECAUSE_TITLE, home_door_because, media_type};
use crate::home::because::Row;
use crate::layout::table as table_layout;
use crate::library_filter::RootKind;
use crate::route::Page;
use crate::sidebar_kind::SidebarKind;
use crate::theme::Tokens;
use crate::view::playlist::root::MediaDraw;

use super::super::{hero, media, section};
use super::Pressed;

/// Draws the shelf, and nothing until it has something to draw.
pub(super) fn show(
    ui: &mut Ui,
    row: &Row<'_>,
    _now_ms: u64,
    tokens: &Tokens,
    draw: &mut MediaDraw<'_, '_>,
    out: &mut Pressed,
) {
    section::head(
        ui,
        &format!("{HOME_BECAUSE_TITLE} {}", row.seed_name),
        "",
        tokens,
    );

    let solved = super::grid::solve(ui);
    let list = super::grid::row_rect(ui, &solved, row.cards.len());
    ui.allocate_rect(list, Sense::hover());
    let visible = ui.clip_rect();
    let want = super::grid::art_px(ui, &solved);

    for (index, card) in row.cards.iter().enumerate() {
        let rect = solved.card_rect(index);
        if !table_layout::on_screen(visible, rect) {
            continue;
        }
        let media = media::Media {
            uri: &card.uri,
            name: &card.name,
            kind: media_type(RootKind::Artists),
            // The reference's artist card is a face and a name: the kind is what
            // the card is, and it says nothing a reader at this shelf wants.
            sub: "",
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
    if super::door::show(ui, &home_door_because(row.seed_name), tokens) {
        out.door = Some(super::Door::Artist(row.seed_uri.to_owned()));
    }
}

#[cfg(test)]
mod tests;
