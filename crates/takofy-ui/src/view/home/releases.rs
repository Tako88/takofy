//! The release shelf: what the artists you follow have released lately.
//!
//! One card row of the page's four columns, each card a release with its artist
//! and the day it came out under it. A waiting shelf holds the room its cards
//! will take. A shelf with nothing to draw takes no room at all, and the rule
//! above it belongs to the page, not to this module.

use egui::{Sense, Ui};

use crate::format::{HOME_RELEASES_TITLE, release_count_label, release_sub};
use crate::home::Section;
use crate::home::releases::List;
use crate::layout::table as table_layout;
use crate::route::Page;
use crate::sidebar_kind::SidebarKind;
use crate::theme::Tokens;
use crate::view::playlist::root::MediaDraw;

use super::super::{hero, media, section};

/// Draws the shelf, or what it says instead.
pub(super) fn show(
    ui: &mut Ui,
    section: &Section<List<'_>>,
    tokens: &Tokens,
    draw: &mut MediaDraw<'_, '_>,
) {
    match section {
        Section::Absent | Section::Failed(_) => {}
        Section::Waiting => {
            section::head(ui, HOME_RELEASES_TITLE, "", tokens);
            super::frame::card_band(ui, tokens);
        }
        Section::Ready(list) => cards_row(ui, list, tokens, draw),
    }
}

/// The shelf's head and its cards, in the page's own card grid.
fn cards_row(ui: &mut Ui, list: &List<'_>, tokens: &Tokens, draw: &mut MediaDraw<'_, '_>) {
    if list.cards.is_empty() {
        return;
    }
    section::head(
        ui,
        HOME_RELEASES_TITLE,
        &release_count_label(list.cards.len()),
        tokens,
    );
    let solved = &super::grid::solve(ui);
    let row = super::grid::row_rect(ui, solved, list.cards.len());
    ui.allocate_rect(row, Sense::hover());
    let visible = ui.clip_rect();
    let want = super::grid::art_px(ui, solved);

    for (index, card) in list.cards.iter().enumerate() {
        let rect = solved.card_rect(index);
        if !table_layout::on_screen(visible, rect) {
            continue;
        }
        let release = card.release;
        let sub = release_sub(&release.artist, &release.date);
        let words = media::Media {
            uri: &release.uri,
            name: &release.name,
            kind: "Album",
            sub: &sub,
            size: "",
            round: false,
            shelf: SidebarKind::Albums,
            playable: true,
            cover: draw.art.look(release.cover.as_ref(), want, draw.now),
            picture: hero::Picture::Cover,
        };
        match media::tile(ui, rect, &words, tokens) {
            Some(media::Press::Open) => *draw.open = Some(Page::Album(release.uri.clone())),
            Some(media::Press::Play) => {
                *draw.play = Some((release.uri.clone(), SidebarKind::Albums));
            }
            Some(media::Press::Menu(_)) | None => {}
        }
    }
}

#[cfg(test)]
mod tests;
