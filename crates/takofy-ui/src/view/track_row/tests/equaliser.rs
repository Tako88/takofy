use super::super::*;

#[test]
fn the_equaliser_bars_breathe_inside_their_range() {
    for step in 0..64 {
        let time = step as f32 * eq::PULSE / 16.0;
        for scale in bar_scales(time) {
            assert!(
                (eq::MIN_SCALE..=1.0).contains(&scale),
                "{scale} at {time}s is outside the pulse"
            );
        }
    }
}

#[test]
fn a_bar_starts_short_and_peaks_halfway_through_its_pulse() {
    let at_rest = bar_scales(0.0)[0];
    assert!((at_rest - eq::MIN_SCALE).abs() < 1e-4, "{at_rest}");
    let at_peak = bar_scales(eq::PULSE / 2.0)[0];
    assert!((at_peak - 1.0).abs() < 1e-4, "{at_peak}");
}

#[test]
fn each_bar_follows_the_one_before_it_late() {
    // The reference staggers the bars in time rather than by height, so a bar's
    // height now is the bar before it's height one delay ago.
    let phase = 0.3;
    let scales = bar_scales(eq::PULSE * phase);
    for (bar, scale) in scales.iter().enumerate().skip(1) {
        let behind = bar_scales(eq::PULSE * phase - bar as f32 * eq::DELAY)[0];
        assert!(
            (*scale - behind).abs() < 1e-4,
            "bar {bar} is {scale} where its leader was {behind}"
        );
    }
}

#[test]
fn the_bars_are_not_all_the_same_height_at_once() {
    let spread = (0..64).any(|step| {
        let scales = bar_scales(step as f32 * eq::PULSE / 64.0);
        scales[0] != scales[1] || scales[1] != scales[2]
    });
    assert!(
        spread,
        "the bars pulse in unison, so the stagger is invisible"
    );
}

#[test]
fn an_absent_column_is_never_drawn_into() {
    // An absent column arrives as a zero-width rect rather than as a missing
    // field, so nothing may lay out a galley against a width it never got.
    let empty = Rect::from_min_size(Pos2::ZERO, vec2(0.0, crate::theme::extent::ROW_H));
    assert!(!drawable(empty));
    assert!(drawable(Rect::from_min_size(
        Pos2::ZERO,
        vec2(
            crate::theme::extent::TRACK_INDEX_W,
            crate::theme::extent::ROW_H
        )
    )));
}
