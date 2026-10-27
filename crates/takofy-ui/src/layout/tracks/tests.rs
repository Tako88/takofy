use super::*;

fn flexible(basis: f32, min: f32, max: f32, grow: f32) -> Track {
    Track {
        basis,
        min,
        max,
        grow,
    }
}

fn assert_close(left: f32, right: f32) {
    assert!((left - right).abs() < 0.001, "{left} is not {right}");
}

#[test]
fn fixed_tracks_take_their_size() {
    let widths = solve(100.0, 0.0, &[Track::fixed(20.0), Track::fixed(30.0)]);
    assert_eq!(widths, vec![20.0, 30.0]);
}

#[test]
fn flexible_tracks_share_the_rest_by_weight() {
    let widths = solve(
        100.0,
        0.0,
        &[
            Track::fixed(20.0),
            flexible(0.0, 0.0, f32::INFINITY, 1.0),
            flexible(0.0, 0.0, f32::INFINITY, 2.0),
        ],
    );
    assert_close(widths[0], 20.0);
    assert_close(widths[1], 80.0 / 3.0);
    assert_close(widths[2], 160.0 / 3.0);
}

#[test]
fn a_track_below_its_minimum_is_held_and_the_others_share_the_remainder() {
    let widths = solve(
        120.0,
        0.0,
        &[
            flexible(0.0, 100.0, f32::INFINITY, 1.0),
            flexible(0.0, 0.0, f32::INFINITY, 1.0),
        ],
    );
    assert_eq!(widths, vec![100.0, 20.0]);
}

#[test]
fn a_track_above_its_maximum_is_capped() {
    let widths = solve(
        160.0,
        0.0,
        &[
            flexible(0.0, 0.0, 60.0, 1.0),
            flexible(0.0, 0.0, f32::INFINITY, 1.0),
        ],
    );
    assert_eq!(widths, vec![60.0, 100.0]);
}

#[test]
fn gaps_come_out_of_the_available_width() {
    let widths = solve(
        100.0,
        10.0,
        &[Track::fixed(30.0), flexible(0.0, 0.0, f32::INFINITY, 1.0)],
    );
    assert_eq!(widths, vec![30.0, 60.0]);
    assert_close(widths.iter().sum::<f32>() + 10.0, 100.0);
}

#[test]
fn the_result_sums_to_the_available_width_when_nothing_is_capped() {
    let widths = solve(
        300.0,
        12.0,
        &[
            flexible(0.0, 20.0, f32::INFINITY, 1.0),
            flexible(0.0, 20.0, f32::INFINITY, 2.0),
            flexible(0.0, 20.0, f32::INFINITY, 1.0),
        ],
    );
    assert_close(widths.iter().sum::<f32>() + 24.0, 300.0);
}

#[test]
fn when_the_minimums_do_not_fit_every_track_holds_its_minimum() {
    let widths = solve(
        100.0,
        0.0,
        &[
            flexible(0.0, 80.0, f32::INFINITY, 1.0),
            flexible(0.0, 80.0, f32::INFINITY, 1.0),
        ],
    );
    assert_eq!(widths, vec![80.0, 80.0]);
}

#[test]
fn no_width_is_negative_or_nan_for_any_input() {
    let tracks = [
        Track::fixed(20.0),
        flexible(-10.0, -5.0, f32::NAN, -1.0),
        flexible(f32::NAN, 10.0, f32::INFINITY, 1.0),
        flexible(30.0, 10.0, 20.0, f32::INFINITY),
    ];

    for available in [-100.0, 0.0, f32::NAN, f32::INFINITY, 5.0, 500.0] {
        for gap in [-4.0, 0.0, f32::NAN, 8.0] {
            let widths = solve(available, gap, &tracks);
            assert_eq!(widths.len(), tracks.len());
            for width in widths {
                assert!(width.is_finite(), "{width} is not finite");
                assert!(width >= 0.0, "{width} is negative");
            }
        }
    }
}
