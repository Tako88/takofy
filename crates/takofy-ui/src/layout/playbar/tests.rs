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
/// An icon button's square, 1.75rem.
const ICON: f32 = 28.0;
/// The volume slider's width, 4.5rem, and its margin either side.
const VOLUME: f32 = 72.0;
const VOLUME_MARGIN: f32 = 8.0;
/// The gap between two runs, `space-1`.
const RUN_GAP: f32 = 4.0;
/// The seek row's height, 0.875rem.
const SLIDER: f32 = 14.0;

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

#[test]
fn the_transport_stack_is_the_tile_the_gap_and_the_seek_row() {
    assert_eq!(transport_stack(), TILE + RUN_GAP + SLIDER);
}

#[test]
fn the_seek_line_hangs_a_tile_and_a_gap_below_the_transport() {
    let top = 100.0;
    let line = seek_line(500.0, top, seek_row(900.0));

    assert_eq!(line.width(), SEEK_MAX);
    assert_eq!(line.height(), SLIDER);
    assert_eq!(line.center().x, 500.0, "the row is centred on the column");
    assert_eq!(line.top(), top + TILE + RUN_GAP);
}

#[test]
fn a_row_is_its_slots_and_the_gaps_between_them() {
    let slots = [Slot::Button, Slot::Volume, Slot::Button];
    assert_eq!(
        row_width(slots),
        2.0 * ICON + VOLUME + 2.0 * VOLUME_MARGIN + 2.0 * RUN_GAP
    );
    assert_eq!(row_width([]), 0.0);
}

#[test]
fn a_row_laid_from_its_width_ends_at_the_edge_it_was_given() {
    let slots = [Slot::Volume, Slot::Button];
    let right = 700.0;
    let centre = 200.0;
    let width = row_width(slots);
    let mut x = right - width;
    let level = place_run(&mut x, true, slots[0], centre);
    let glyph = place_run(&mut x, false, slots[1], centre);

    // The slider's margin is inside its slot; the button follows a gap.
    assert_eq!(level.left(), right - width + VOLUME_MARGIN);
    assert_eq!(level.width(), VOLUME);
    assert_eq!(level.center().y, centre);
    assert_eq!(glyph.left(), level.right() + VOLUME_MARGIN + RUN_GAP);
    assert_eq!(glyph.size(), Vec2::splat(ICON));
    assert_eq!(glyph.center().y, centre);
    assert_eq!(glyph.right(), right, "the walk ends where it started");
    assert_eq!(x, right);
}

#[test]
fn the_leading_run_takes_no_gap_before_it() {
    let mut x = 100.0;
    let first = place_run(&mut x, true, Slot::Button, 0.0);
    assert_eq!(first.left(), 100.0);
    let second = place_run(&mut x, false, Slot::Button, 0.0);
    assert_eq!(second.left(), first.right() + RUN_GAP);
}
