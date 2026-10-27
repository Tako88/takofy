//! The playbar's three rows.
//!
//! The reference's bar is three grid columns; each holds one row of controls
//! above one row of readouts. This module is the arithmetic of those rows, so
//! the painter places what was solved rather than solving it as it draws.
//!
//! Every value is closed: a column too narrow for its contents yields a row that
//! still starts at a floor rather than one that runs backwards.

use egui::{Pos2, Rect, Vec2};

use crate::theme::{extent, space};

/// The transport's height: the large tile, the gap under it, and the seek row.
///
/// The play button and the seek row are one stack centred on the column, so the
/// painter and any rect it derives from that stack read this together.
pub fn transport_stack() -> f32 {
    extent::BUTTON_LG + space::S1 + extent::SLIDER_H
}

/// One run's footprint in a row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    /// An icon button, `BUTTON` on a side.
    Button,
    /// The level: `VOLUME_W` wide and a slider tall, a margin either side.
    Volume,
}

impl Slot {
    /// The width the slot takes in a row, its own margins included.
    pub fn width(self) -> f32 {
        match self {
            Self::Button => extent::BUTTON,
            Self::Volume => extent::VOLUME_W + 2.0 * space::S2,
        }
    }
}

/// The width a row of runs takes: its slots and a gap between each pair.
pub fn row_width<P: IntoIterator<Item = Slot>>(slots: P) -> f32 {
    let mut width = 0.0;
    for (index, slot) in slots.into_iter().enumerate() {
        if index > 0 {
            width += space::S1;
        }
        width += slot.width();
    }
    width
}

/// Places one run in a row and leaves `x` past it.
///
/// `first` is the row's leading run, which takes no gap before it. The painter
/// and every rect the shell derives from the row walk it through here, so the
/// two cannot disagree about where a slot sits.
pub fn place_run(x: &mut f32, first: bool, slot: Slot, centre_y: f32) -> Rect {
    if !first {
        *x += space::S1;
    }
    match slot {
        Slot::Button => {
            let rect = Rect::from_min_size(
                Pos2::new(*x, centre_y - extent::BUTTON / 2.0),
                Vec2::splat(extent::BUTTON),
            );
            *x += extent::BUTTON;
            rect
        }
        Slot::Volume => {
            *x += space::S2;
            let rect = Rect::from_min_size(
                Pos2::new(*x, centre_y - extent::SLIDER_H / 2.0),
                Vec2::new(extent::VOLUME_W, extent::SLIDER_H),
            );
            *x += extent::VOLUME_W + space::S2;
            rect
        }
    }
}

/// The transport's three runs, left to right.
///
/// The tile is centred on the column and the runs sit a margin and a gap clear
/// of it, which is the reference's `margin-inline` on the tile plus the run's
/// own gap.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TransportRow {
    pub down_x: f32,
    pub play_x: f32,
    pub up_x: f32,
}

/// The seek row's width and the track between its two readouts.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SeekRow {
    pub width: f32,
    pub track: f32,
}

/// The now column's text and the controls beside it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct NowRow {
    pub text_left: f32,
    pub text_w: f32,
    pub controls_x: f32,
}

/// Solves the transport's runs, with the tile centred on `centre_x`.
pub fn transport_row(centre_x: f32, down_w: f32) -> TransportRow {
    let play_x = centre_x - extent::BUTTON_LG / 2.0;
    TransportRow {
        down_x: play_x - (space::S1 + space::S2) - down_w,
        play_x,
        up_x: play_x + extent::BUTTON_LG + space::S2 + space::S1,
    }
}

/// Solves the seek row inside a column `column_w` wide.
///
/// The row takes the whole column until the cap, and the track is what is left
/// after a readout and a gap on each side.
pub fn seek_row(column_w: f32) -> SeekRow {
    let width = column_w.clamp(0.0, extent::PROGRESS_MAX_W);
    SeekRow {
        width,
        track: (width - 2.0 * (extent::TIME_W + space::S3)).max(0.0),
    }
}

/// The seek row's rect: centred on `centre_x`, under a transport that starts at
/// `top`.
///
/// The stack is centred on the column, so the seek row sits one tile and its
/// gap below the transport's top.
pub fn seek_line(centre_x: f32, top: f32, seek: SeekRow) -> Rect {
    Rect::from_min_size(
        Pos2::new(
            centre_x - seek.width / 2.0,
            top + extent::BUTTON_LG + space::S1,
        ),
        Vec2::new(seek.width, extent::SLIDER_H),
    )
}

/// Solves the now column's text and controls.
///
/// `natural_w` is the unclamped width of the wider text line, measured before
/// the row is given its room. A line too long for the room is elided, and the
/// controls then go to the column's edge: an elided galley is a few points short
/// of the room it was given, so pinning to the edge is what keeps the buttons
/// still while the words change.
pub fn now_row(
    col_left: f32,
    col_right: f32,
    art_w: f32,
    controls_w: f32,
    natural_w: f32,
) -> NowRow {
    let text_left = col_left + art_w + space::S3;
    let avail = ((col_right - controls_w - space::S3) - text_left).max(0.0);
    if natural_w > avail {
        NowRow {
            text_left,
            text_w: avail,
            controls_x: col_right - controls_w,
        }
    } else {
        NowRow {
            text_left,
            text_w: natural_w,
            controls_x: text_left + natural_w + space::S3,
        }
    }
}

#[cfg(test)]
mod tests;
