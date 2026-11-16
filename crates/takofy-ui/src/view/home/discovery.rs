//! Discovery's band: a seed, the artists the outside graph puts next to it, the
//! arithmetic of what the filter kept, and the switch that turns it off.
//!
//! The band is drawn outside the dated shelf's slot machinery: it has its own
//! state, its own asks and its own forget path, and it is absent rather than
//! "off" when it has nothing to say. Its section head is shared with the
//! stations band, and each row's head is the same head again. Off is not a
//! silence: the band collapses to the switch row that turned it off, so the
//! setting has somewhere visible to be undone. A row's foot line is the press
//! that draws one ring deeper, or the sentence that says what the last press
//! found.

use std::time::Duration;

use egui::{Rect, Sense, Ui, pos2, vec2};
use takofy_model::TrackEntry;
use takofy_model::home::{DiscoveryKind, DiscoverySeed};

use crate::availability::Control;
use crate::format::{
    HOME_DISCOVERY_ASIDE, HOME_DISCOVERY_TITLE, discovery_ledger, discovery_more_line,
    discovery_source,
};
use crate::home::discovery::body::{self, Body};
use crate::home::discovery::{Band, RowRead};
use crate::layout::Table;
use crate::layout::discovery as geometry;
use crate::route::Page;
use crate::theme::{Tokens, extent, space};
use crate::view::list_more;
use crate::view::mini_row;
use crate::view::playlist::root::MediaDraw;
use crate::view::skeleton;

use super::super::{media, row_head, section};
use super::Pressed;

mod card;
mod switch;

/// Draws the discovery band and records the presses it took.
///
/// Returns whether it drew anything. `Off` draws the switch row alone, because
/// that row is the setting's visible instance; a band at work draws the same row
/// at its foot. `Absent` and `Unreachable` draw nothing and take no room: a
/// failure is not a setting, so it gets no copy and no switch.
pub(super) fn show(
    ui: &mut Ui,
    band: &Band<'_>,
    now_ms: u64,
    tokens: &Tokens,
    table: Table,
    draw: &mut MediaDraw<'_, '_>,
    out: &mut Pressed,
) -> bool {
    let narrow = table.stacks_discovery();
    match band {
        Band::Off => {
            out.switch = switch::show(ui, false, tokens);
            true
        }
        Band::Absent | Band::Unreachable => false,
        Band::Waiting => {
            section::head(ui, HOME_DISCOVERY_TITLE, HOME_DISCOVERY_ASIDE, tokens);
            skeleton_row(ui, 4, narrow, tokens);
            ui.add_space(space::S3);
            out.switch = switch::show(ui, true, tokens);
            true
        }
        Band::Rows(rows) => {
            section::head(ui, HOME_DISCOVERY_TITLE, HOME_DISCOVERY_ASIDE, tokens);
            let mut drew = false;
            for row in rows {
                if row.read == RowRead::Unreachable {
                    continue;
                }
                if drew {
                    ui.add_space(space::S5);
                }
                row_show(ui, row, now_ms, tokens, narrow, draw, out);
                drew = true;
            }
            if drew {
                ui.add_space(space::S3);
            }
            out.switch = switch::show(ui, true, tokens);
            true
        }
    }
}

/// One row: its head, the body its cards take, and the line under it.
fn row_show(
    ui: &mut Ui,
    row: &crate::home::discovery::RowView<'_>,
    now_ms: u64,
    tokens: &Tokens,
    narrow: bool,
    draw: &mut MediaDraw<'_, '_>,
    out: &mut Pressed,
) {
    let ready = row.read == RowRead::Ready;
    let more = draw.library.more_discovery(row.seed.kind, now_ms);
    if let crate::home::discovery::walk::More::Note(_, remaining) = more {
        ui.ctx()
            .request_repaint_after(Duration::from_millis(remaining));
    }
    let body = ready.then(|| body::body_of(&row.cards, narrow));
    let width = ui.available_width();
    let mut waited = None;

    let ledger = body.map(|body| {
        let (artists, tracks) = body::ledger_counts(&row.cards, body);
        discovery_ledger(artists, tracks)
    });
    let play = body.map(|body| body::row_tracks(&row.cards, body));

    let reroll = if ready {
        let state = draw.library.reroll_state_discovery(row.seed.kind, now_ms);
        if let crate::home::discovery::Reroll::Wait(_) = state {
            waited = state.waited();
            // The border draws itself as the wait runs out, so the frame has to
            // come before the wait ends: one wake at the end would freeze it at
            // whatever share the last repaint caught.
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
        // A row with nothing left to draw has no button to dim: the play keeps
        // the edge, because a box that will never come back is a promise the row
        // cannot keep.
        (state != crate::home::discovery::Reroll::Never).then(|| row_head::Button {
            control: Control::DiscoveryReroll,
            key: row.seed.uri.as_str(),
            live: state == crate::home::discovery::Reroll::Ready,
        })
    } else {
        None
    };

    let (head, _) =
        ui.allocate_exact_size(vec2(width, row_head::height(ui, tokens)), Sense::hover());
    let opened = row_head::show(
        ui,
        head.shrink2(vec2(space::S2, 0.0)),
        row_head::Head {
            seed: &row.seed.name,
            key: &row.seed.uri,
            seed_page: page_of(row.seed),
            provenance: &discovery_source(row.seed.kind, row.genre),
            ledger: ledger.as_deref(),
            reroll,
            waited,
            play: ready.then(|| row_head::Button {
                control: Control::DiscoveryRowPlay,
                key: row.seed.uri.as_str(),
                live: !crate::view::carry::carrying(ui.ctx())
                    && play.as_ref().is_some_and(|tracks| !tracks.is_empty()),
            }),
        },
        tokens,
    );
    if let Some(page) = opened.seed {
        *draw.open = Some(page);
    }
    if opened.reroll {
        out.reroll = Some(row.seed.kind);
    }
    if opened.play {
        out.discovery = play;
    }

    ui.add_space(space::S2);
    let block = match body {
        None => {
            skeleton_row(ui, 4, narrow, tokens);
            return;
        }
        Some(body) => body_show(ui, row, body, narrow, tokens, draw, out),
    };
    let Some(line) = discovery_more_line(more) else {
        return;
    };
    if list_more::show(
        ui,
        block,
        Some(line.words),
        list_more::Look::centred(line.live),
        tokens,
    ) {
        out.more = Some(row.seed.kind);
    }
}

/// The row's body, in the shape its cards take, and the block it took.
fn body_show(
    ui: &mut Ui,
    row: &crate::home::discovery::RowView<'_>,
    body: Body,
    narrow: bool,
    tokens: &Tokens,
    draw: &mut MediaDraw<'_, '_>,
    out: &mut Pressed,
) -> Rect {
    let width = ui.available_width();
    let extra = media::extra_height(ui);
    let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 0.0));

    match body {
        Body::Faces { cards } => {
            let columns = body::columns_for(body, narrow);
            let grid = geometry::faces(area, columns, space::S3, extra);
            let height = grid.height(cards);
            let (slot, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
            let origin = slot.min.to_vec2();
            for (index, data) in row.cards.iter().enumerate() {
                let rect = grid.card_rect(index).translate(origin);
                card::show(
                    ui,
                    card::Card {
                        rect,
                        art: geometry::card_art(rect),
                        data,
                        seed: &row.seed.uri,
                        words: card::Words::Below,
                    },
                    tokens,
                    draw,
                    out,
                );
            }
            slot
        }
        Body::Pair => {
            let pair = geometry::pair(
                area,
                space::S3,
                crate::theme::discovery::PAIR_ART_SHARE,
                extra,
            );
            let height = pair.cards[0].height();
            let (slot, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
            let origin = slot.min.to_vec2();
            for (index, data) in row.cards.iter().take(2).enumerate() {
                let rect = pair.cards[index].translate(origin);
                card::show(
                    ui,
                    card::Card {
                        rect,
                        art: pair.arts[index].translate(origin),
                        data,
                        seed: &row.seed.uri,
                        words: card::Words::Beside,
                    },
                    tokens,
                    draw,
                    out,
                );
            }
            slot
        }
        Body::One => {
            let rect = geometry::lone(area, crate::theme::discovery::FACE_W, extra);
            let (slot, _) = ui.allocate_exact_size(vec2(width, rect.height()), Sense::hover());
            let rect = rect.translate(slot.min.to_vec2());
            if let Some(data) = row.cards.first() {
                card::show(
                    ui,
                    card::Card {
                        rect,
                        art: geometry::card_art(rect),
                        data,
                        seed: &row.seed.uri,
                        words: card::Words::Below,
                    },
                    tokens,
                    draw,
                    out,
                );
            }
            slot
        }
        Body::Split { rows } => {
            let split = geometry::split(area, crate::theme::discovery::FACE_W, space::S5, extra);
            let (slot, _) =
                ui.allocate_exact_size(vec2(width, split.card.height()), Sense::hover());
            let origin = slot.min.to_vec2();
            let subject = split.card.translate(origin);
            let tracks_rect = split.tracks.translate(origin);
            if let Some(data) = row.cards.first() {
                card::show(
                    ui,
                    card::Card {
                        rect: subject,
                        art: geometry::card_art(subject),
                        data,
                        seed: &row.seed.uri,
                        words: card::Words::Below,
                    },
                    tokens,
                    draw,
                    out,
                );
                split_rows(ui, &tracks_rect, data, rows, tokens, draw, out);
            }
            slot
        }
    }
}

/// The mini-rows beside a split card, and the tracks a press queues.
fn split_rows(
    ui: &mut Ui,
    tracks: &Rect,
    data: &takofy_model::home::DiscoveryCard,
    rows: usize,
    tokens: &Tokens,
    draw: &mut MediaDraw<'_, '_>,
    out: &mut Pressed,
) {
    let named: Vec<&TrackEntry> = body::named_tracks(data).into_iter().take(rows).collect();
    let row_h = extent::HOME_ROW_H;
    let total = named.len() as f32 * row_h + named.len().saturating_sub(1) as f32 * space::S1;
    let top = tracks.top() + ((tracks.height() - total) / 2.0).max(0.0);
    for (index, track) in named.iter().enumerate() {
        let rect = Rect::from_min_size(
            pos2(tracks.left(), top + index as f32 * (row_h + space::S1)),
            vec2(tracks.width(), row_h),
        );
        let response = mini_row::show(
            ui,
            rect,
            mini_row::Row {
                salt: &data.artist.uri,
                track,
                duration: mini_row::duration_of(track, draw),
            },
            draw,
            tokens,
        );
        if response.clicked() {
            out.discovery = Some(
                named[index..]
                    .iter()
                    .map(|track| (*track).clone())
                    .collect(),
            );
        }
    }
}

/// A band or a row that has not landed: its faces held open in the grid.
fn skeleton_row(ui: &mut Ui, count: usize, narrow: bool, tokens: &Tokens) {
    let width = ui.available_width();
    let columns = if narrow { 2 } else { 4 };
    let extra = media::extra_height(ui);
    let grid = geometry::faces(
        Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 0.0)),
        columns,
        space::S3,
        extra,
    );
    let (slot, _) = ui.allocate_exact_size(vec2(width, grid.height(count)), Sense::hover());
    let grid = geometry::faces(
        Rect::from_min_size(slot.min, vec2(width, 0.0)),
        columns,
        space::S3,
        extra,
    );
    skeleton::tiles(ui, &grid, count, tokens);
}

/// The page a discovery seed names.
fn page_of(seed: &DiscoverySeed) -> Option<Page> {
    Some(match seed.kind {
        DiscoveryKind::Playlist => Page::Playlist(seed.uri.clone()),
        DiscoveryKind::Artist => Page::Artist(seed.uri.clone()),
        DiscoveryKind::Album => Page::Album(seed.uri.clone()),
    })
}

#[cfg(test)]
mod tests;
