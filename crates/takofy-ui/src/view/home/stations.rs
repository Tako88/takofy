//! The stations shelf: a two-column band built from the library's own seeds.
//!
//! One cell per station: its picture, a head naming the kind and the title, the
//! control that plays the whole station, and the three rows the band shows. A
//! row opens the album its label named. A station the wire named no picture for
//! draws the hole the head reserves, so a band of them still reads as one grid.

use egui::{Rect, Sense, Ui, Vec2, pos2, vec2};

use crate::availability::Control;
use crate::cover::{Cover, want_px};
use crate::format::HOME_STATIONS_TITLE;
use crate::home::stations::{Band, ROWS, Shown};
use crate::icon::Icon;
use crate::layout::chips;
use crate::mosaic;
use crate::route::Page;
use crate::sidebar_kind::SidebarKind;
use crate::theme::{Tokens, extent, radius, space};
use crate::view::playlist::root::MediaDraw;

use super::super::{art, carry, controls, hero, media, section, text as recipe};
use super::Pressed;

/// The band's columns: a station beside its neighbour.
const COLUMNS: usize = 2;

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
    for (station, at) in band.stations.iter().zip(placed.rects) {
        cell_of(ui, at.translate(origin), station, tokens, draw, out);
    }
}

/// How tall one station is: its head, the gap under it, and the rows it shows.
fn cell(ui: &Ui, tokens: &Tokens) -> f32 {
    head_height(ui, tokens) + space::S2 + ROWS as f32 * extent::ROW_H
}

/// The head's one line, which is at least as tall as the picture it carries.
fn head_height(ui: &Ui, tokens: &Tokens) -> f32 {
    line_height(ui, tokens).max(extent::LIBRARY_ART)
}

/// One line of the head's title, measured as the painter lays it out.
fn line_height(ui: &Ui, tokens: &Tokens) -> f32 {
    ui.painter()
        .layout_no_wrap(" ".to_owned(), recipe::card_name(), tokens.palette.text)
        .size()
        .y
}

/// The head of one station's cell: the row its picture, words and play share.
fn head_rect(ui: &Ui, rect: Rect, tokens: &Tokens) -> Rect {
    Rect::from_min_size(rect.min, vec2(rect.width(), head_height(ui, tokens)))
}

/// The station's own play, at the right end of its head.
fn play_rect(head: Rect) -> Rect {
    Rect::from_center_size(
        pos2(head.right() - extent::BUTTON / 2.0, head.center().y),
        Vec2::splat(extent::BUTTON),
    )
}

/// The rects of the rows under `head`, one per shown row.
fn row_rects(rect: Rect, head: Rect, rows: usize) -> Vec<Rect> {
    (0..rows)
        .map(|at| {
            Rect::from_min_size(
                pos2(
                    rect.left(),
                    head.bottom() + space::S2 + at as f32 * extent::ROW_H,
                ),
                vec2(rect.width(), extent::ROW_H),
            )
        })
        .collect()
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
    picture_of(ui, head, station, tokens, draw);

    // The play takes the head's right end, the kind sits beside it, and the
    // title takes whatever the two leave, so a long one elides rather than
    // pushing the control off the cell.
    let play = play_rect(head);
    let kind = recipe::caption(ui, station.kind, tokens);
    let kind_at = pos2(
        play.left() - space::S3 - kind.size().x,
        head.center().y - kind.size().y / 2.0,
    );
    let title_left = head.left() + extent::LIBRARY_ART + space::S3;
    let title = recipe::line(
        ui,
        station.head,
        recipe::card_name(),
        tokens.palette.text,
        0.0,
        (kind_at.x - space::S4 - title_left).max(0.0),
    );
    ui.painter().galley(
        pos2(title_left, head.center().y - title.size().y / 2.0),
        title,
        tokens.palette.text,
    );
    ui.painter()
        .galley(kind_at, kind, tokens.palette.text_subtle);

    let slot = controls::play_tile_keyed(
        ui,
        tokens,
        Control::StationPlay,
        station.uri,
        play,
        Icon::Play,
        &format!("Play {}", station.head),
        !carry::carrying(ui.ctx()),
    );
    if slot.response.clicked() {
        out.station = Some((station.uri.to_owned(), station.whole.to_vec()));
    }

    let rows = row_rects(rect, head, station.shown.len());
    for (row_rect, row) in rows
        .into_iter()
        .zip(station.shown.iter().filter_map(|at| station.whole.get(*at)))
    {
        let words = row_words(ui, row, draw);
        if let Some(media::Press::Open) = media::row(ui, row_rect, &words, tokens)
            && let Some(album) = &row.album_uri
        {
            *draw.open = Some(Page::Album(album.clone()));
        }
    }
}

/// One station row, drawn by the same component a library row is.
fn row_words<'a>(
    ui: &Ui,
    row: &'a takofy_model::TrackEntry,
    draw: &mut MediaDraw<'_, '_>,
) -> media::Media<'a> {
    let want = want_px(extent::LIBRARY_ART, ui.pixels_per_point());
    media::Media {
        uri: &row.uri,
        name: &row.title,
        kind: "",
        sub: &row.artists,
        size: "",
        round: false,
        shelf: SidebarKind::Albums,
        // A row opens the album it came from; the station's own play is the one
        // play the band offers.
        playable: false,
        cover: draw.art.look(row.cover.as_ref(), want, draw.now),
        picture: hero::Picture::Cover,
    }
}

/// Draws the station's own picture in the head's first square, or the hole it
/// leaves when the wire named none this client can draw.
fn picture_of(
    ui: &Ui,
    head: Rect,
    station: &Shown<'_>,
    tokens: &Tokens,
    draw: &mut MediaDraw<'_, '_>,
) {
    let picture = Rect::from_center_size(
        pos2(head.left() + extent::LIBRARY_ART / 2.0, head.center().y),
        Vec2::splat(extent::LIBRARY_ART),
    );
    let want = want_px(picture.width(), ui.pixels_per_point());
    match station.art {
        Some(takofy_model::home::StationArt::Single(set)) => art::tile(
            ui.painter(),
            picture,
            radius::SM,
            tokens.palette.border,
            draw.art.look(Some(set), want, draw.now),
            tokens,
        ),
        Some(takofy_model::home::StationArt::Mosaic(parts)) => {
            let covers: Vec<Cover> = parts
                .iter()
                .map(|set| draw.art.look(Some(set), want.div_ceil(2), draw.now))
                .collect();
            match mosaic::arrange(&covers) {
                Some(quadrants) => art::mosaic(
                    ui.painter(),
                    picture,
                    radius::SM,
                    tokens.palette.border,
                    quadrants,
                    tokens,
                ),
                // A part still loading holds the whole picture back, as the
                // playlist band's mosaic does, so no corner swaps a frame early.
                None => art::hole(ui.painter(), picture, radius::SM, tokens),
            }
        }
        None => art::hole(ui.painter(), picture, radius::SM, tokens),
    }
}

#[cfg(test)]
mod tests;
