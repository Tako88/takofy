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

use egui::{Pos2, Rect, Sense, Ui, Vec2, pos2, vec2};
use takofy_model::TrackEntry;

use crate::availability::Control;
use crate::cover::{Cover, want_px};
use crate::format::{HOME_DOOR_LIBRARY, HOME_STATIONS_TITLE, format_position};
use crate::home::stations::{Band, ROWS, Shown};
use crate::icon::Icon;
use crate::layout::chips;
use crate::route::Page;
use crate::theme::icon::Size;
use crate::theme::{Tokens, extent, radius, space, tracking};
use crate::view::carry;
use crate::view::playlist::root::MediaDraw;

use super::super::{art, controls, section, text as recipe};
use super::Pressed;

/// The band's columns: a station beside its neighbour.
pub(super) const COLUMNS: usize = 2;

/// One station row's height: the reference's 3rem box around a 40pt cover.
const ROW_H: f32 = extent::HOME_ROW_H;

/// The station's own play, at the reference's 2rem square.
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
    let lines = line_height(ui, recipe::station_seed(), tokens)
        + space::S1
        + line_height(ui, recipe::station_source(), tokens);
    space::S1 + lines.max(PLAY) + space::S2
}

/// One line of type, measured as the painter lays it out.
fn line_height(ui: &Ui, font: egui::FontId, tokens: &Tokens) -> f32 {
    ui.painter()
        .layout_no_wrap(" ".to_owned(), font, tokens.palette.text)
        .size()
        .y
}

/// The head of one station's cell.
fn head_rect(ui: &Ui, rect: Rect, tokens: &Tokens) -> Rect {
    Rect::from_min_size(rect.min, vec2(rect.width(), head_height(ui, tokens)))
}

/// The station's own play, at the right end of its head.
fn play_rect(head: Rect) -> Rect {
    Rect::from_center_size(
        pos2(head.right() - space::S2 - PLAY / 2.0, head.center().y),
        Vec2::splat(PLAY),
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
    let play = play_rect(head);

    // The words take whatever the play leaves, and the seed's name elides rather
    // than pushing the control off the cell.
    let left = head.left() + space::S2;
    let width = (play.left() - space::S3 - left).max(0.0);
    let seed = recipe::line(
        ui,
        station.head,
        recipe::station_seed(),
        tokens.palette.text,
        recipe::tracking_pts(crate::theme::text::MD, tracking::TIGHT),
        width,
    );
    let source = recipe::line(
        ui,
        station.source,
        recipe::station_source(),
        tokens.palette.text_subtle,
        recipe::tracking_pts(crate::theme::text::XXS, tracking::WIDE),
        width,
    );
    let block = seed.size().y + space::S1 + source.size().y;
    let top = head.center().y - block / 2.0;
    ui.painter()
        .galley(Pos2::new(left, top), seed.clone(), tokens.palette.text);
    ui.painter().galley(
        Pos2::new(left, top + seed.size().y + space::S1),
        source,
        tokens.palette.text_subtle,
    );

    let slot = controls::icon_button_keyed(
        ui,
        tokens,
        Control::StationPlay,
        station.uri,
        play,
        Icon::Play,
        Size::SM,
        &format!("Play the {} station", station.head),
        !carry::carrying(ui.ctx()),
    );
    if slot.response.clicked() {
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
    // A track can be in two stations, so its row's identity is the station's as
    // well as its own: one widget cannot stand for two rows.
    let id = ui
        .scope_id()
        .with(("home-station-row", station.uri, row.uri.as_str()));
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            true,
            format!("{} by {}", row.title, row.artists),
        )
    });
    if response.clicked() {
        response.request_focus();
    }
    let over = response.hovered() || ui.rect_contains_pointer(rect);
    if over {
        ui.painter()
            .rect_filled(rect, radius::SM, tokens.palette.row_hover);
    }
    controls::focus_ring(ui, &response, rect, radius::SM, tokens);

    let want = want_px(extent::QUEUE_ART, ui.pixels_per_point());
    let cover: Cover = draw.art.look(row.cover.as_ref(), want, draw.now);
    let art_rect = Rect::from_min_size(
        pos2(
            rect.left() + space::S2,
            rect.center().y - extent::QUEUE_ART / 2.0,
        ),
        Vec2::splat(extent::QUEUE_ART),
    );
    art::tile(
        ui.painter(),
        art_rect,
        radius::XS,
        tokens.palette.border,
        cover,
        tokens,
    );

    // The length goes at the row's far end, and the words take what it leaves.
    let size = draw
        .library
        .home
        .duration(&row.uri)
        .or(row.duration_ms)
        .map(format_position);
    let meta = size.as_deref().map(|label| {
        recipe::line(
            ui,
            label,
            recipe::track_mono(crate::theme::text::XXS),
            tokens.palette.text_subtle,
            0.0,
            rect.width(),
        )
    });
    let meta_w = meta.as_ref().map_or(0.0, |line| line.size().x + space::S3);

    let body_left = art_rect.right() + space::S3;
    let body_right = (rect.right() - space::S2 - meta_w).max(body_left);
    let width = body_right - body_left;
    let name = recipe::line(
        ui,
        &row.title,
        recipe::queue_name(),
        tokens.palette.text,
        0.0,
        width,
    );
    let sub = recipe::line(
        ui,
        &row.artists,
        recipe::queue_artist(),
        tokens.palette.text_subtle,
        0.0,
        width,
    );
    let block = name.size().y + sub.size().y;
    let top = rect.center().y - block / 2.0;
    ui.painter()
        .galley(Pos2::new(body_left, top), name.clone(), tokens.palette.text);
    ui.painter().galley(
        Pos2::new(body_left, top + name.size().y),
        sub,
        tokens.palette.text_subtle,
    );
    if let Some(meta) = meta {
        ui.painter().galley(
            Pos2::new(
                rect.right() - space::S2 - meta.size().x,
                rect.center().y - meta.size().y / 2.0,
            ),
            meta,
            tokens.palette.text_subtle,
        );
    }

    if response.clicked() {
        return row.album_uri.clone();
    }
    None
}

#[cfg(test)]
mod tests;
