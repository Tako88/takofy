use super::*;

/// The panel's content band: the region less the panel's own hairline.
fn band(width: f32, height: f32) -> Rect {
    Rect::from_min_size(pos2(10.0, 20.0), vec2(width, height))
}

/// The two heights the painter measures, at values close to what the fonts
/// give: three text lines and the chip pair.
const META: f32 = 56.0;
const TABS: f32 = 22.0;

fn parts(stack: Stack) -> [(&'static str, Rect); 5] {
    [
        ("head", stack.head),
        ("art", stack.art),
        ("meta", stack.meta),
        ("tabs", stack.tabs),
        ("pane", stack.pane),
    ]
}

/// The band a panel has at a given window size, solved the way the shell
/// solves it rather than copied: the region minus the window's padding, the
/// playbar row and the panel's hairline.
fn band_at(window: egui::Vec2) -> Rect {
    use crate::layout::{grid_for, regions_for, shell_for};

    let content = Rect::from_min_size(pos2(0.0, 0.0), window);
    let shell = shell_for(&regions_for(window.x));
    let grid = grid_for(content, &shell);
    grid.now_playing
        .expect("the panel is drawn at this width")
        .shrink(crate::theme::stroke::HAIRLINE)
}

#[test]
fn the_reservation_is_title_artist_gap_state() {
    // The three lines the meta block can draw, at heights near what the
    // fonts give: a title, an artist and a state, with space-1 before the
    // state.
    assert_eq!(meta_reserved(22.0, 18.0, 11.0), 55.0);
    assert_eq!(space::S1, 4.0, "the gap before the state is space-1");
    // Every component is named, so a state that draws no line still
    // reserves one.
    assert_eq!(meta_reserved(22.0, 18.0, 0.0), 44.0);
    assert_eq!(meta_reserved(22.0, 0.0, 11.0), 37.0);
    // A height that cannot be measured reads as none, like every other.
    assert_eq!(meta_reserved(f32::NAN, 18.0, 11.0), 33.0);
}

/// The smallest band that still holds the art at its floor, the reservation
/// in full and the pane at its minimum: everything above and below the art.
fn smallest_band() -> f32 {
    let reserved = meta_reserved(22.0, 18.0, 11.0);
    space::S3
        + HEAD_H
        + space::S3
        + extent::NOWPLAYING_ART_MIN
        + space::S4
        + reserved
        + space::S3
        + TABS
        + space::S3
        + PANE_MIN_H
}

/// The reservation, the gaps around it and the vertical order, all of which
/// a band large enough to hold the pane's minimum must keep.
fn assert_reserved_and_ordered(stack: Stack, band: Rect, reserved: f32) {
    assert_eq!(stack.meta.height(), reserved, "the reserve is not honoured");
    for (name, gap, above, below) in [
        (
            "art to meta",
            space::S4,
            stack.art.bottom(),
            stack.meta.top(),
        ),
        (
            "meta to tabs",
            space::S3,
            stack.meta.bottom(),
            stack.tabs.top(),
        ),
        (
            "tabs to pane",
            space::S3,
            stack.tabs.bottom(),
            stack.pane.top(),
        ),
    ] {
        assert!(
            (above + gap - below).abs() < 0.01,
            "{name} is {} wide of its {gap} gap",
            above + gap - below
        );
    }
    for pair in parts(stack).windows(2) {
        let ((first, above), (second, below)) = (pair[0], pair[1]);
        assert!(
            above.bottom() <= below.top() + 0.01,
            "{second} overlaps {first} in {band:?}"
        );
    }
    for (name, rect) in parts(stack) {
        assert!(
            rect.top() >= band.top() - 0.01,
            "{name} starts above {band:?}"
        );
        assert!(
            rect.bottom() <= band.bottom() + 0.01,
            "{name} ends below {band:?}"
        );
    }
    assert!(
        stack.art.height() >= extent::NOWPLAYING_ART_MIN,
        "the art is {} in {band:?}",
        stack.art.height()
    );
    assert!(
        stack.pane.height() >= PANE_MIN_H - 0.01,
        "the pane is {} in {band:?}",
        stack.pane.height()
    );
}

#[test]
fn every_band_size_keeps_the_reservation_and_the_vertical_order() {
    // The state the pane reserves: a title, an artist and a state line.
    // Nothing about a track enters it, so the stack that comes out depends
    // on the band alone. From the smallest band that still holds the pane's
    // minimum up to a tall one, the reserve is exact and nothing overlaps.
    let reserved = meta_reserved(22.0, 18.0, 11.0);
    let smallest = smallest_band();
    // The sweep starts at the tight case, where the art sits on its floor
    // and the pane on its minimum, so a larger reservation fails here first.
    let tight = stack_for(band(270.0, smallest), reserved, TABS);
    assert_eq!(tight.art.height(), extent::NOWPLAYING_ART_MIN);
    assert!((tight.pane.height() - PANE_MIN_H).abs() < 0.01);
    for width in [270.0, 240.0] {
        let steps = (900.0 - smallest) as u32;
        for step in 0..=steps {
            let band = band(width, smallest + step as f32);
            assert_reserved_and_ordered(stack_for(band, reserved, TABS), band, reserved);
        }
    }
}

#[test]
fn the_smallest_supported_window_keeps_the_reservation_and_both_floors() {
    // The shortest window the user can reach, at the pane's narrowest and
    // widest widths. The taller reservation must not squeeze the art to its
    // floor or the pane below its minimum here.
    let reserved = meta_reserved(22.0, 18.0, 11.0);
    let widths = [crate::layout::NOWPLAYING_HIDDEN_MAX + 1.0, 1440.0];
    for width in widths {
        let band = band_at(vec2(width, crate::layout::WINDOW_MIN[1]));
        assert_eq!(band.height(), 410.0, "the band at {width}");
        let stack = stack_for(band, reserved, TABS);
        assert_reserved_and_ordered(stack, band, reserved);
        assert!(
            stack.art.height() > extent::NOWPLAYING_ART_MIN,
            "the art is squeezed to its floor at {width}: {}",
            stack.art.height()
        );
    }
}

#[test]
fn the_stack_is_ordered_and_inside_the_band() {
    let band = band(270.0, 390.0);
    let stack = stack_for(band, META, TABS);
    let order = [
        stack.head.top(),
        stack.head.bottom(),
        stack.art.top(),
        stack.art.bottom(),
        stack.meta.top(),
        stack.meta.bottom(),
        stack.tabs.top(),
        stack.tabs.bottom(),
        stack.pane.top(),
    ];
    for pair in order.windows(2) {
        assert!(
            pair[0] <= pair[1] + 0.01,
            "the stack is out of order: {order:?}"
        );
    }
    for (name, rect) in parts(stack) {
        assert!(
            rect.top() >= band.top() - 0.01,
            "{name} starts above the band"
        );
        assert!(
            rect.bottom() <= band.bottom() + 0.01,
            "{name} ends below the band"
        );
        assert!(
            rect.left() >= band.left() - 0.01,
            "{name} starts left of the band"
        );
        assert!(
            rect.right() <= band.right() + 0.01,
            "{name} ends right of the band"
        );
    }
}

#[test]
fn every_rect_is_inset_s3_except_the_pane() {
    // A band tall enough that the art is not the thing giving way.
    let band = band(270.0, 900.0);
    let stack = stack_for(band, META, TABS);
    for (name, rect) in [
        ("head", stack.head),
        ("meta", stack.meta),
        ("tabs", stack.tabs),
    ] {
        assert!(
            (rect.left() - band.left() - space::S3).abs() < 0.01,
            "{name}"
        );
        assert!(
            (rect.right() - band.right() + space::S3).abs() < 0.01,
            "{name}"
        );
    }
    assert!((stack.head.top() - band.top() - space::S3).abs() < 0.01);
    assert!((stack.art.width() - (band.width() - 2.0 * space::S3)).abs() < 0.01);
    // The pane is the scroller, so it takes the band's whole width and its
    // own content is what gets inset.
    assert_eq!(stack.pane.left(), band.left());
    assert_eq!(stack.pane.right(), band.right());
}

#[test]
fn the_art_is_capped_in_a_tall_band_and_is_the_band_less_its_insets_in_a_narrow_one() {
    let tall = stack_for(band(312.0, 900.0), META, TABS);
    assert_eq!(tall.art.width(), extent::NOWPLAYING_ART_MAX);
    assert_eq!(tall.art.height(), extent::NOWPLAYING_ART_MAX);
    assert!(
        (tall.art.center().x - band(312.0, 900.0).center().x).abs() < 0.01,
        "the art is centred in the band"
    );

    let narrow = stack_for(band(270.0, 900.0), META, TABS);
    assert_eq!(narrow.art.width(), 270.0 - 2.0 * space::S3);
}

#[test]
fn the_art_shrinks_so_the_pane_keeps_its_minimum() {
    // A band tall enough for the cap, then shorter and shorter: the art gives
    // way before the pane does.
    let short = stack_for(band(270.0, 390.0), META, TABS);
    assert!(
        short.art.height() < extent::NOWPLAYING_ART_MAX,
        "a 390-point band cannot hold the cap and the pane"
    );
    assert!(
        short.pane.height() >= PANE_MIN_H - 0.01,
        "the pane fell to {}",
        short.pane.height()
    );
}

#[test]
fn the_art_never_drops_below_its_floor() {
    for height in [390.0, 300.0, 240.0, 120.0, 40.0, 0.0] {
        let stack = stack_for(band(270.0, height), META, TABS);
        assert!(
            stack.art.height() >= extent::NOWPLAYING_ART_MIN,
            "the art is {} at {height}",
            stack.art.height()
        );
        assert_eq!(stack.art.width(), stack.art.height());
    }
}

#[test]
fn the_pane_ends_at_the_bands_bottom_and_is_never_negative() {
    for height in [390.0, 200.0, 100.0, 10.0, 0.0] {
        let band = band(270.0, height);
        let stack = stack_for(band, META, TABS);
        assert_eq!(stack.pane.bottom(), band.bottom(), "at {height}");
        assert!(
            stack.pane.height() >= 0.0,
            "the pane is {} at {height}",
            stack.pane.height()
        );
    }
}

#[test]
fn the_narrowest_visible_window_clears_the_art_floor() {
    // Just above the width that hides the panel, at the shortest window.
    let band = band_at(vec2(1121.0, crate::layout::WINDOW_MIN[1]));
    assert_eq!(band.width(), 270.0);
    assert_eq!(band.height(), 410.0);

    let stack = stack_for(band, META, TABS);
    assert!(
        stack.art.height() > extent::NOWPLAYING_ART_MIN,
        "the art fell to {}, below its {} floor",
        stack.art.height(),
        extent::NOWPLAYING_ART_MIN
    );
    assert!(stack.pane.height() >= PANE_MIN_H, "{}", stack.pane.height());
}

#[test]
fn the_default_wide_window_holds_the_full_art_and_the_pane() {
    let band = band_at(vec2(1440.0, 900.0));
    let stack = stack_for(band, META, TABS);
    assert_eq!(stack.art.width(), extent::NOWPLAYING_ART_MAX);
    assert!(stack.pane.height() > PANE_MIN_H);
}

#[test]
fn a_zero_size_band_neither_panics_nor_produces_nan() {
    for band in [
        Rect::ZERO,
        band(0.0, 0.0),
        band(0.0, 400.0),
        band(400.0, 0.0),
        Rect::from_min_size(pos2(f32::NAN, 0.0), vec2(100.0, 100.0)),
        Rect::from_min_size(pos2(0.0, 0.0), vec2(f32::INFINITY, f32::INFINITY)),
        Rect::from_min_size(pos2(0.0, 0.0), vec2(-50.0, -50.0)),
    ] {
        for (meta, tabs) in [(META, TABS), (0.0, 0.0), (f32::NAN, f32::INFINITY)] {
            let stack = stack_for(band, meta, tabs);
            for (name, rect) in parts(stack) {
                assert!(
                    rect.min.x.is_finite() && rect.min.y.is_finite(),
                    "{name} in {band:?} has a non-finite origin: {rect:?}"
                );
                assert!(
                    rect.width().is_finite() && rect.height().is_finite(),
                    "{name} in {band:?} has a non-finite size: {rect:?}"
                );
                assert!(
                    rect.width() >= 0.0 && rect.height() >= 0.0,
                    "{name} in {band:?} is negative: {rect:?}"
                );
            }
        }
    }
}
