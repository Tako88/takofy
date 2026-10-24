//! The playbar's three rows.
//!
//! The reference's bar is three grid columns; each holds one row of controls
//! above one row of readouts. This module is the arithmetic of those rows, so
//! the painter places what was solved rather than solving it as it draws.
//!
//! Every value is closed: a column too narrow for its contents yields a row that
//! still starts at a floor rather than one that runs backwards.

use crate::theme::{extent, space};

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
mod tests {
    use super::*;

    /// The tile's side, the sheet's 2.125rem.
    const TILE: f32 = 34.0;
    /// The tile's `margin-inline` plus the run's gap: 8 + 4.
    const CLEARANCE: f32 = 12.0;
    /// A readout's column, 2.5rem.
    const READOUT: f32 = 40.0;
    /// The gap between a readout and the track, `space-3`.
    const GAP: f32 = 12.0;
    /// The row's cap, 32rem.
    const SEEK_MAX: f32 = 512.0;

    /// A now column with the art, the two controls and a width to spare.
    fn now_column(natural_w: f32) -> NowRow {
        now_row(100.0, 400.0, 44.0, 60.0, natural_w)
    }

    #[test]
    fn the_play_tile_is_centred_on_the_column() {
        let row = transport_row(500.0, 60.0);
        assert!((row.play_x - (500.0 - TILE / 2.0)).abs() < 1e-3, "{row:?}");
    }

    #[test]
    fn the_tile_is_twelve_clear_of_each_run() {
        let row = transport_row(500.0, 60.0);
        assert!((row.play_x - CLEARANCE - (row.down_x + 60.0)).abs() < 1e-3);
        assert!((row.up_x - CLEARANCE - (row.play_x + TILE)).abs() < 1e-3);
    }

    #[test]
    fn the_up_run_starts_after_the_tile_margin_and_gap() {
        let row = transport_row(500.0, 60.0);
        assert!((row.up_x - (row.play_x + TILE + 8.0 + 4.0)).abs() < 1e-3);
    }

    #[test]
    fn the_seek_row_is_512_end_to_end_in_a_wide_column() {
        assert_eq!(seek_row(900.0).width, SEEK_MAX);
        assert_eq!(seek_row(SEEK_MAX).width, SEEK_MAX);
    }

    #[test]
    fn a_narrow_column_gives_the_seek_row_all_it_has() {
        assert_eq!(seek_row(300.0).width, 300.0);
        assert_eq!(seek_row(SEEK_MAX - 1.0).width, SEEK_MAX - 1.0);
    }

    #[test]
    fn the_seek_track_is_the_row_less_two_readouts_and_gaps() {
        let row = seek_row(900.0);
        assert_eq!(row.track, SEEK_MAX - 2.0 * (READOUT + GAP));
    }

    #[test]
    fn the_seek_track_never_goes_negative() {
        assert_eq!(seek_row(0.0).track, 0.0);
        assert_eq!(seek_row(-40.0).track, 0.0);
        assert_eq!(seek_row(READOUT).track, 0.0);
        assert_eq!(seek_row(200.0).track, 96.0);
    }

    #[test]
    fn the_buttons_follow_a_short_title() {
        let row = now_column(120.0);
        assert_eq!(row.text_left, 100.0 + 44.0 + 12.0);
        assert_eq!(row.text_w, 120.0);
        assert_eq!(row.controls_x, 156.0 + 120.0 + 12.0);
    }

    #[test]
    fn a_long_title_pins_the_buttons_to_the_column_edge() {
        let row = now_column(500.0);
        assert_eq!(row.text_w, 172.0);
        assert_eq!(row.controls_x, 400.0 - 60.0, "{row:?}");
    }

    #[test]
    fn a_title_exactly_filling_the_room_is_not_elided() {
        let row = now_column(172.0);
        assert_eq!(row.text_w, 172.0);
        assert_eq!(row.controls_x, 400.0 - 60.0);
    }

    #[test]
    fn the_buttons_never_leave_the_column() {
        for (col_right, art_w, controls_w, natural_w) in [
            (400.0, 44.0, 60.0, 0.0),
            (400.0, 44.0, 60.0, 172.0),
            (400.0, 44.0, 60.0, 500.0),
            (900.0, 44.0, 60.0, 900.0),
            (300.0, 44.0, 28.0, 40.0),
        ] {
            let row = now_row(100.0, col_right, art_w, controls_w, natural_w);
            assert!(
                row.controls_x + controls_w <= col_right + 1e-3,
                "{row:?} spills past {col_right}"
            );
            assert!(
                row.text_left + row.text_w <= row.controls_x - 12.0 + 1e-3,
                "{row:?} overlaps the controls"
            );
        }
    }

    #[test]
    fn a_zero_width_column_collapses_without_panicking() {
        let row = now_row(100.0, 100.0, 44.0, 60.0, 80.0);
        assert_eq!(row.text_w, 0.0);
        assert_eq!(row.controls_x, 40.0);

        let seek = seek_row(0.0);
        assert_eq!(seek.width, 0.0);
        assert_eq!(seek.track, 0.0);
    }
}
