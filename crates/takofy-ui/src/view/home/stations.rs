//! The stations shelf: a two-column band built from the library's own seeds.
//!
//! One cell per station, in the reference's shape: a head that names the seed and
//! says what it was built from, the control that plays the whole station at the
//! head's right end, and three rows under it. A row is the reference's `.mini-row`
//! — the cover, the track over its artist, and how long it runs — and it opens the
//! album it came from, because the band's one play is the station's own.
//!
//! Nothing here is a table row: a station cell is a quarter of the page wide, and
//! the table's kind and size columns would leave the name no room at all.

use egui::{Rect, Sense, Ui, pos2, vec2};
use takofy_model::TrackEntry;

use crate::format::{HOME_DOOR_LIBRARY, HOME_STATIONS_TITLE};
use crate::home::stations::{Band, ROWS, Shown};
use crate::layout::chips;
use crate::route::Page;
use crate::theme::{Tokens, extent, space};
use crate::view::playlist::root::MediaDraw;

use super::super::{row_head, section};
use super::Pressed;

/// The band's columns: a station beside its neighbour.
pub(super) const COLUMNS: usize = 2;

/// One station row's height: the reference's 3rem box around a 40pt cover.
const ROW_H: f32 = extent::HOME_ROW_H;

/// The station's own play, at the reference's 2rem square.
#[cfg(test)]
const PLAY: f32 = extent::BUTTON_LG;

/// Draws the shelf, and nothing until it has something to draw.
pub(super) fn show(
    ui: &mut Ui,
    band: &Band<'_>,
    _now_ms: u64,
    tokens: &Tokens,
    draw: &mut MediaDraw<'_, '_>,
    out: &mut Pressed,
) {
    section::head(ui, HOME_STATIONS_TITLE, "", tokens);
    let width = ui.available_width();
    let placed = chips::grid(
        band.stations.len(),
        width,
        COLUMNS,
        cell(ui, tokens),
        space::GUTTER,
    );
    let (list, _) = ui.allocate_exact_size(vec2(width, placed.height), Sense::hover());
    let origin = list.min.to_vec2();
    let visible = ui.clip_rect();
    for (station, at) in band.stations.iter().zip(placed.rects) {
        let rect = at.translate(origin);
        // The band is two stations tall at most, so a cell off the fold is not
        // worth laying out: its cover would be looked up for nothing.
        if !rect.intersects(visible) {
            continue;
        }
        cell_of(ui, rect, station, tokens, draw, out);
    }
    if super::door::show(ui, HOME_DOOR_LIBRARY, tokens) {
        out.door = Some(super::Door::Library);
    }
}

/// How tall one station is: its head, one step of air, and the rows it shows.
pub(super) fn cell(ui: &Ui, tokens: &Tokens) -> f32 {
    head_height(ui, tokens) + space::S1 + ROWS as f32 * ROW_H
}

/// The head's height: the seed's two lines, or the play they sit beside.
fn head_height(ui: &Ui, tokens: &Tokens) -> f32 {
    row_head::height(ui, tokens, row_head::SeedRank::Row)
}

/// The head of one station's cell.
fn head_rect(ui: &Ui, rect: Rect, tokens: &Tokens) -> Rect {
    Rect::from_min_size(rect.min, vec2(rect.width(), head_height(ui, tokens)))
}

/// The station's own play, at the right end of its head.
#[cfg(test)]
fn play_rect(head: Rect) -> Rect {
    Rect::from_center_size(
        pos2(head.right() - space::S2 - PLAY / 2.0, head.center().y),
        egui::Vec2::splat(PLAY),
    )
}

/// One station's cell: the head, its play, and the rows under them.
fn cell_of(
    ui: &mut Ui,
    rect: Rect,
    station: &Shown<'_>,
    tokens: &Tokens,
    draw: &mut MediaDraw<'_, '_>,
    out: &mut Pressed,
) {
    let head = head_rect(ui, rect, tokens);

    let opened = row_head::show(
        ui,
        head.shrink2(vec2(space::S2, 0.0)),
        row_head::Head {
            seed: station.head,
            key: station.uri,
            seed_page: None,
            provenance: station.source,
            ledger: None,
            reroll: None,
            waited: None,
            play: Some(row_head::Button {
                control: crate::availability::Control::StationPlay,
                key: station.uri,
                live: !crate::view::carry::carrying(ui.ctx()),
                busy: false,
            }),
            rank: row_head::SeedRank::Row,
        },
        tokens,
    );
    if opened.play {
        out.station = Some((station.uri.to_owned(), station.whole.to_vec()));
    }

    for (at, row) in shown_rows(station) {
        if let Some(album) = row_of(ui, row_rect(rect, head, at), station, row, tokens, draw) {
            *draw.open = Some(Page::Album(album));
        }
    }
}

/// The rects of the rows under `head`, one per shown row.
fn row_rect(rect: Rect, head: Rect, at: usize) -> Rect {
    Rect::from_min_size(
        pos2(rect.left(), head.bottom() + space::S1 + at as f32 * ROW_H),
        vec2(rect.width(), ROW_H),
    )
}

/// The rows the band shows of one station, with the slot each takes.
fn shown_rows<'a>(station: &'a Shown<'_>) -> Vec<(usize, &'a TrackEntry)> {
    station
        .shown
        .iter()
        .take(ROWS)
        .enumerate()
        .filter_map(|(at, index)| Some((at, station.whole.get(*index)?)))
        .collect()
}

/// Draws one row and returns the album it asked to open.
fn row_of(
    ui: &mut Ui,
    rect: Rect,
    station: &Shown<'_>,
    row: &TrackEntry,
    tokens: &Tokens,
    draw: &mut MediaDraw<'_, '_>,
) -> Option<String> {
    let response = super::super::mini_row::show(
        ui,
        rect,
        super::super::mini_row::Row {
            salt: station.uri,
            track: row,
            duration: super::super::mini_row::duration_of(row, draw),
        },
        draw,
        tokens,
    );
    response.clicked().then(|| row.album_uri.clone()).flatten()
}

#[cfg(test)]
mod tests;
