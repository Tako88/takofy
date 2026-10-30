//! The dated shelf: the skeleton, the cards, and the states between them.

use egui::{Sense, Ui};

use crate::availability::Control;
use crate::cover::want_px;
use crate::format::{
    HOME_DAY_ONE_BODY, HOME_DAY_ONE_DOOR, HOME_DAY_ONE_TITLE, HOME_DOOR_SAVED, HOME_QUIET,
    HOME_SHELF_TITLE, media_type, saved_sub,
};
use crate::home::{Card, Saved, Shelf, Slot};
use crate::layout::table as table_layout;
use crate::library::MediaLayout;
use crate::library_filter::{Item, RootKind};
use crate::route::Page;
use crate::sidebar_kind::SidebarKind;
use crate::view::playlist::root::{self, MediaDraw};

use super::super::text as recipe;
use super::super::{controls, hero, media, section};
use super::{Door, Pressed, door, frame};

// The dated page's own tests read art, greeting and head through this module, so
// the three stay named here while only they use them.
#[cfg(test)]
use super::super::page_head;
#[cfg(test)]
use crate::cover::Art;
#[cfg(test)]
use crate::home::greeting;

#[cfg(test)]
mod tests;

/// Draws the dated shelf and records the presses it took.
///
/// It draws whatever the shelf's own state is, including the two states where it
/// has nothing to show, because the greeting alone must never be the page.
pub(super) fn show(
    ui: &mut Ui,
    shelf: &Shelf<'_>,
    now_ms: u64,
    draw: &mut MediaDraw<'_, '_>,
    out: &mut Pressed,
) {
    match shelf {
        Shelf::Waiting => {
            // One row of tiles and the door under them: the shelf is one row, so
            // the skeleton holds the exact shape the content will take.
            frame::card_band(ui, draw.tokens);
            frame::door_band(ui, draw.tokens);
        }
        Shelf::Unreadable(failure) | Shelf::Incomplete(failure) => {
            ui.label(recipe::muted(
                crate::signin::failure_message(*failure),
                draw.tokens,
            ));
            // The root's own failure draws the same pair: the words, then the
            // control that asks again.
            if ui.button("Retry").clicked() {
                out.retry = Some(Slot::Dated);
            }
        }
        Shelf::DayOne => {
            ui.label(recipe::muted(HOME_DAY_ONE_TITLE, draw.tokens));
            ui.label(recipe::muted(HOME_DAY_ONE_BODY, draw.tokens));
            let size = controls::text_button_size(ui, draw.tokens, HOME_DAY_ONE_DOOR);
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let slot = controls::text_button(
                ui,
                draw.tokens,
                Control::HomeSearch,
                rect,
                HOME_DAY_ONE_DOOR,
                true,
            );
            if slot.response.clicked() {
                out.focus_search = true;
            }
        }
        Shelf::Quiet => {
            ui.label(recipe::muted(HOME_QUIET, draw.tokens));
        }
        Shelf::Cards(cards) => {
            cards_row(ui, cards, draw, now_ms);
            if door::show(ui, HOME_DOOR_SAVED, draw.tokens) {
                out.door = Some(Door::Library);
            }
        }
    }
}

/// The shelf's head and its cards, in the library's own tiles.
///
/// The head is drawn here, and the grid is solved after it, so the cards cannot
/// be placed above the title that names them.
fn cards_row(ui: &mut Ui, cards: &[Card<'_>], draw: &mut MediaDraw<'_, '_>, now_ms: u64) {
    let head = section::head(ui, HOME_SHELF_TITLE, "", draw.tokens);
    let solved = &root::media_grid(ui);
    if let Some(first) = (0..cards.len()).map(|at| solved.card_rect(at)).next() {
        debug_assert!(
            first.top() >= head.bottom(),
            "the shelf's cards start above its own title"
        );
    }
    let list = root::grid_rect(ui, solved, cards.len());
    ui.allocate_rect(list, Sense::hover());
    let visible = ui.clip_rect();
    let want = want_px(solved.card_w, ui.pixels_per_point());

    for (index, card) in cards.iter().enumerate() {
        let rect = solved.card_rect(index);
        if !table_layout::on_screen(visible, rect) {
            continue;
        }
        match card.saved {
            Saved::Playlist(entry) => {
                let mut words = draw.media_of(&Item::Playlist(entry), MediaLayout::Grid, want);
                words.sub = saved_sub(media_type(RootKind::Playlists), card.at, now_ms);
                match media::tile(ui, rect, &words.as_media(), draw.tokens) {
                    Some(media::Press::Open) => {
                        draw.activate(&Item::Playlist(entry), entry.uri.as_str());
                    }
                    Some(media::Press::Play) => draw.want_play(&words),
                    Some(media::Press::Menu(_)) | None => {}
                }
            }
            Saved::Album(album) => {
                let mut words = draw.media_of(&Item::Album(album), MediaLayout::Grid, want);
                words.sub = saved_sub(media_type(RootKind::Albums), card.at, now_ms);
                match media::tile(ui, rect, &words.as_media(), draw.tokens) {
                    Some(media::Press::Open) => {
                        draw.activate(&Item::Album(album), album.uri.as_str());
                    }
                    Some(media::Press::Play) => draw.want_play(&words),
                    Some(media::Press::Menu(_)) | None => {}
                }
            }
            Saved::Track(track) => {
                // A liked song has no page of its own, so the card opens the
                // album the label named, which is the only page it belongs to.
                let sub = saved_sub("Song", card.at, now_ms);
                let cover = draw.art.look(track.cover.as_ref(), want, draw.now);
                let words = media::Media {
                    uri: &track.uri,
                    name: &track.title,
                    kind: "Song",
                    sub: &sub,
                    size: "",
                    round: false,
                    shelf: SidebarKind::Albums,
                    playable: false,
                    cover,
                    picture: hero::Picture::Cover,
                };
                if let Some(media::Press::Open) = media::tile(ui, rect, &words, draw.tokens)
                    && let Some(album) = &track.album_uri
                {
                    *draw.open = Some(Page::Album(album.clone()));
                }
            }
        }
    }
}
