//! The shelf at chance: one item drawn out of the library, and the four cards
//! read from its station.
//!
//! Its seed is the shelf's own title, so the head is set at the title's rank and
//! carries one control: the quiet square that draws another. The cards stay while
//! the next draw is in flight, and the foot line is the one place a draw that
//! found nothing, or a read that failed, says so (#96).

use std::time::Duration;

use egui::{Sense, Ui, vec2};
use takofy_model::home::SeedKind;

use crate::availability::Control;
use crate::format::{DRAW_FAILED, DRAW_NOTHING, HOME_CHANCE_TITLE, draw_ledger, draw_source};
use crate::home::Section;
use crate::home::chance::{self, CardKind};
use crate::layout::table as table_layout;
use crate::route::Page;
use crate::sidebar_kind::SidebarKind;
use crate::theme::{Tokens, motion, space};
use crate::view::list_more;
use crate::view::playlist::root::MediaDraw;

use super::super::{hero, media, row_head, section};
use super::Pressed;

/// The sweep's span: three slow steps, the reference's `--draw-wait`.
const WAIT_MS: u64 = (motion::SLOW * 3.0 * 1_000.0) as u64;

/// Draws the shelf and records the presses it took.
///
/// Returns whether it drew anything. A shelf with nothing to draw takes no room,
/// so the page above it ends where it ended before this shelf existed.
pub(super) fn show(
    ui: &mut Ui,
    shelf: &Section<chance::Draw<'_>>,
    now_ms: u64,
    stacked: bool,
    tokens: &Tokens,
    draw: &mut MediaDraw<'_, '_>,
    out: &mut Pressed,
) -> bool {
    match shelf {
        Section::Absent | Section::Failed(_) => false,
        Section::Waiting => {
            section::head(ui, HOME_CHANCE_TITLE, "", tokens);
            super::frame::card_band(ui, tokens);
            true
        }
        Section::Ready(drawn) => {
            drawn_shelf(ui, drawn, now_ms, stacked, tokens, draw, out);
            true
        }
    }
}

/// The shelf with a draw in hand: its head, its cards, and the foot line.
fn drawn_shelf(
    ui: &mut Ui,
    drawn: &chance::Draw<'_>,
    now_ms: u64,
    stacked: bool,
    tokens: &Tokens,
    draw: &mut MediaDraw<'_, '_>,
    out: &mut Pressed,
) {
    let width = ui.available_width();
    let (head, _) = ui.allocate_exact_size(
        vec2(
            width,
            row_head::height(ui, tokens, row_head::SeedRank::Shelf),
        ),
        Sense::hover(),
    );
    if drawn.pending {
        // The square's edge draws itself as the wait runs out, so the frame has
        // to come before the wait ends.
        ui.ctx()
            .request_repaint_after(Duration::from_millis(crate::theme::eq::WAKE_MS));
    }
    // A failed draw keeps the seed and the four that last landed, but the ledger
    // is the one count it can no longer stand behind: the sentence on the foot
    // line replaces it.
    let ledger = (drawn.failed.is_none() && !drawn.cards.is_empty())
        .then(|| draw_ledger(drawn.tracks, drawn.albums));
    let source = draw_source(drawn.seed.kind, drawn.seed.at, now_ms);
    let opened = row_head::show(
        ui,
        head.shrink2(vec2(space::S2, 0.0)),
        row_head::Head {
            seed: &drawn.seed.name,
            key: &drawn.seed.uri,
            seed_page: page_of(drawn),
            provenance: &source,
            ledger: ledger.as_deref(),
            reroll: Some(row_head::Button {
                control: Control::DrawAnother,
                key: "draw",
                // A draw already out keeps the square focusable and inert, so a
                // burst of presses is served at the wire's rate.
                live: !drawn.pending,
                busy: drawn.pending,
            }),
            // The line is the wait itself, so it retires when the answer does: a
            // closed border left behind would say the shelf is still drawing.
            waited: drawn
                .pending
                .then(|| chance::waited(drawn.pressed_ms, now_ms, WAIT_MS))
                .flatten(),
            // The head carries one control: the draw is the band's own, and a
            // shelf play would be a queue built out of four cards.
            play: None,
            rank: row_head::SeedRank::Shelf,
        },
        tokens,
    );
    if let Some(page) = opened.seed {
        *draw.open = Some(page);
    }
    if opened.reroll {
        out.another = true;
    }

    ui.add_space(space::S2);
    // A head whose station is still out holds the room its cards will take, so the
    // shelf below does not jump a row when they land — the same wait the discovery
    // band draws.
    if drawn.cards.is_empty() && drawn.pending {
        super::frame::card_band(ui, tokens);
    }
    let block = cards(ui, drawn, stacked, tokens, draw, out);
    let sentence = match (drawn.failed.is_some(), drawn.nothing) {
        (true, _) => Some(DRAW_FAILED),
        (false, true) => Some(DRAW_NOTHING),
        (false, false) => None,
    };
    if let Some(sentence) = sentence {
        // The reference's quiet sentence, not a control: the square is the only
        // thing on this shelf that acts.
        list_more::show(
            ui,
            block,
            Some(sentence),
            list_more::Look::quiet(space::S1),
            tokens,
        );
    }
}

/// The shelf's cards, and the block they took so the foot line sits under them.
fn cards(
    ui: &mut Ui,
    drawn: &chance::Draw<'_>,
    stacked: bool,
    tokens: &Tokens,
    draw: &mut MediaDraw<'_, '_>,
    out: &mut Pressed,
) -> egui::Rect {
    let width = ui.available_width();
    let empty = egui::Rect::from_min_size(ui.cursor().min, vec2(width, 0.0));
    if drawn.cards.is_empty() {
        return empty;
    }
    let grid = if stacked {
        super::grid::solve_lead(ui)
    } else {
        super::grid::solve(ui)
    };
    let row = super::grid::row_rect(ui, &grid, drawn.cards.len());
    ui.allocate_rect(row, Sense::hover());
    let visible = ui.clip_rect();
    let want = super::grid::art_px(ui, &grid);
    for (index, card) in drawn.cards.iter().enumerate() {
        let rect = grid.card_rect(index);
        if !table_layout::on_screen(visible, rect) {
            continue;
        }
        card_show(ui, rect, card, stacked, want, tokens, draw, out);
    }
    row
}

/// One card: the track or its album, with the kind word leading the artist's
/// line.
#[expect(
    clippy::too_many_arguments,
    reason = "a card needs its place, the draw it came from and the sinks it feeds"
)]
fn card_show(
    ui: &mut Ui,
    rect: egui::Rect,
    card: &chance::Card<'_>,
    stacked: bool,
    want: u32,
    tokens: &Tokens,
    draw: &mut MediaDraw<'_, '_>,
    out: &mut Pressed,
) {
    let album = card.row.album_uri.as_deref();
    let (uri, name, kind) = match card.kind {
        CardKind::Album => (
            album.unwrap_or(card.row.uri.as_str()),
            card.row.album.as_deref().unwrap_or(&card.row.title),
            "Album",
        ),
        CardKind::Track => (card.row.uri.as_str(), card.row.title.as_str(), "Track"),
    };
    let words = media::Media {
        uri,
        name,
        kind,
        sub: &card.row.artists,
        lead: Some(media::Lead { stacked }),
        size: "",
        round: false,
        shelf: SidebarKind::Albums,
        playable: true,
        cover: draw.art.look(card.row.cover.as_ref(), want, draw.now),
        picture: hero::Picture::Cover,
    };
    match media::tile(ui, rect, &words, tokens) {
        Some(media::Press::Open) => {
            if let Some(album) = album {
                *draw.open = Some(Page::Album(album.to_owned()));
            }
        }
        Some(media::Press::Play) => match card.kind {
            CardKind::Album => {
                if let Some(album) = album {
                    *draw.play = Some((album.to_owned(), SidebarKind::Albums));
                }
            }
            // A track card is one row: the thing the user pointed at, not the
            // four the shelf happens to show.
            CardKind::Track => out.rows = Some(vec![card.row.clone()]),
        },
        Some(media::Press::Menu(_)) | None => {}
    }
}

/// The page the drawn seed opens, when it names one.
fn page_of(drawn: &chance::Draw<'_>) -> Option<Page> {
    match drawn.seed.kind {
        SeedKind::Playlist => Some(Page::Playlist(drawn.seed.uri.clone())),
        SeedKind::Artist => Some(Page::Artist(drawn.seed.uri.clone())),
        SeedKind::Album => Some(Page::Album(drawn.seed.uri.clone())),
        SeedKind::Song => drawn.album_uri.clone().map(Page::Album),
    }
}

#[cfg(test)]
mod tests;
