//! The dated page: the greeting, the shelf, and the presses it returns.

use std::time::Instant;

use egui::{Sense, Ui};

use crate::availability::Control;
use crate::cover::{Art, want_px};
use crate::format::{
    HOME_DAY_ONE_BODY, HOME_DAY_ONE_DOOR, HOME_DAY_ONE_TITLE, HOME_QUIET, HOME_SHELF_TITLE,
    media_type, saved_sub,
};
use crate::home::{Card, Saved, Shelf, greeting};
use crate::layout::table as table_layout;
use crate::library::{Library, MediaLayout};
use crate::library_filter::{Item, RootKind};
use crate::message::Command;
use crate::route::Page;
use crate::sidebar_kind::SidebarKind;
use crate::theme::Tokens;
use crate::view::playlist::root::{self, MediaDraw};

use super::super::text as recipe;
use super::super::{controls, hero, media, page_head, section, skeleton};

#[cfg(test)]
mod tests;

/// Draws Home and returns the command its controls asked for.
pub(super) fn home_view(
    ui: &mut Ui,
    library: &mut Library,
    art: &mut Art,
    now: Instant,
    now_ms: u64,
    local_hour: u8,
    tokens: &Tokens,
) -> Option<Command> {
    let mut select: Option<String> = None;
    let mut open: Option<Page> = None;
    let mut play: Option<(String, SidebarKind)> = None;
    let mut focus_search = false;
    let mut retry = false;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            crate::view::inner(ui, |ui| {
                page_head::show(ui, greeting(local_hour), "", tokens);
                // The press that failed is the retry, so there is no button.
                if let Some(failure) = library.play_notice {
                    ui.label(recipe::muted(
                        crate::signin::failure_message(failure),
                        tokens,
                    ));
                }
                let columns = root::media_grid(ui).columns;
                // The shelf is one row, so the grid's own column count is the
                // number of cards to draw.
                match library.home_shelf(columns) {
                    Shelf::Waiting => {
                        // One row of tiles: the shelf is one row, so the
                        // skeleton holds the shape the cards will.
                        let count = columns;
                        let solved = root::media_grid(ui);
                        let list = root::grid_rect(ui, &solved, count);
                        ui.allocate_rect(list, Sense::hover());
                        skeleton::tiles(ui, &solved, count, tokens);
                    }
                    Shelf::Unreadable(failure) | Shelf::Incomplete(failure) => {
                        ui.label(recipe::muted(
                            crate::signin::failure_message(failure),
                            tokens,
                        ));
                        // The root's own failure draws the same pair: the words,
                        // then the control that asks again.
                        if ui.button("Retry").clicked() {
                            retry = true;
                        }
                    }
                    Shelf::DayOne => {
                        ui.label(recipe::muted(HOME_DAY_ONE_TITLE, tokens));
                        ui.label(recipe::muted(HOME_DAY_ONE_BODY, tokens));
                        let size = controls::text_button_size(ui, tokens, HOME_DAY_ONE_DOOR);
                        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                        let slot = controls::text_button(
                            ui,
                            tokens,
                            Control::HomeSearch,
                            rect,
                            HOME_DAY_ONE_DOOR,
                            true,
                        );
                        if slot.response.clicked() {
                            focus_search = true;
                        }
                    }
                    Shelf::Quiet => {
                        ui.label(recipe::muted(HOME_QUIET, tokens));
                    }
                    Shelf::Cards(cards) => {
                        let mut draw = MediaDraw {
                            art,
                            now,
                            tokens,
                            library: &*library,
                            select: &mut select,
                            open: &mut open,
                            play: &mut play,
                        };
                        cards_row(ui, &cards, &mut draw, now_ms);
                    }
                }
            });
        });

    if let Some((uri, kind)) = play
        && library.want_play(&uri, kind)
    {
        ui.ctx().request_repaint();
    }
    if focus_search {
        // The topbar draws the field and is drawn before this page, so the ask is
        // carried to it and the frame that moves the keyboard is asked for here.
        library.search.ask_focus();
        ui.ctx().request_repaint();
    }
    if retry {
        return Some(library.reload());
    }
    if let Some(uri) = select {
        return Some(library.select(&uri));
    }
    if let Some(page) = open {
        return library.open(page);
    }
    None
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
