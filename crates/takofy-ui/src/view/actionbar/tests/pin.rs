use egui::Rect;

use super::super::*;
use super::test_support::*;

use crate::theme::hero;
use crate::view::test_support::click_events;

#[test]
fn the_flow_slot_is_rest_height_stuck_or_not() {
    let ctx = context();
    let rested = frame(&ctx, 0.0, Vec::new());
    assert_eq!(
        rested.flow.height(),
        actionbar::REST_H,
        "the row holds its resting height in the flow: {:?}",
        rested.flow
    );
    assert_eq!(rested.flow.top(), FILLER);

    // Scrolled so the row's slot has gone past the top of the viewport.
    let pinned = frame(&ctx, FILLER + 50.0, Vec::new());
    assert!(pinned.flow.top() < 0.0, "the slot is above the clip");
    assert_eq!(
        pinned.flow.height(),
        actionbar::REST_H,
        "the slot keeps its resting height while the row is pinned"
    );
}

#[test]
fn pinned_it_is_a_surface_fill_of_stuck_height_with_a_bottom_hairline() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let pinned = frame(&ctx, FILLER + 50.0, Vec::new());

    let bands = fills(&pinned.output, tokens.palette.surface);
    let (_, band) = bands
        .iter()
        .find(|(_, rect)| (rect.top() - pinned.flow.top().max(0.0)).abs() < 0.01)
        .map(|(at, rect)| (*at, *rect))
        .unwrap_or_else(|| panic!("the pinned row paints no fill: {bands:?}"));
    assert!(
        (band.top() - 0.0).abs() < 0.01,
        "the row sits at the viewport's top: {band:?}"
    );
    assert_eq!(band.height(), actionbar::STUCK_H);
    assert_eq!(band.width(), pinned.flow.width());

    let line = fills(&pinned.output, tokens.palette.border)
        .into_iter()
        .find(|(_, rect)| {
            (rect.height() - stroke::HAIRLINE).abs() < 0.01
                && (rect.bottom() - band.bottom()).abs() < 0.01
                && (rect.width() - band.width()).abs() < 0.01
        })
        .map(|(_, rect)| rect);
    assert!(
        line.is_some(),
        "the pinned row wears a hairline across its bottom: {band:?}"
    );
}

#[test]
fn at_rest_it_paints_no_fill_and_no_edge() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let rested = frame(&ctx, 0.0, Vec::new());
    assert!(
        fills(&rested.output, tokens.palette.surface).is_empty(),
        "a row in its place veils nothing: {:?}",
        fills(&rested.output, tokens.palette.surface)
    );
    let edges: Vec<Rect> = fills(&rested.output, tokens.palette.border)
        .into_iter()
        .filter(|(_, rect)| (rect.bottom() - rested.flow.bottom()).abs() < 0.01)
        .map(|(_, rect)| rect)
        .collect();
    assert!(
        edges.is_empty(),
        "a row in its place wears no stuck edge: {edges:?}"
    );
    assert!(
        text_at(&rested.output, "Late Night Drive").is_empty(),
        "a row in its place paints no name: {:?}",
        text_at(&rested.output, "Late Night Drive")
    );
}

#[test]
fn the_fill_is_painted_before_the_widgets() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let mut rows = Rows::narrowed();
    let pinned = frame_in(
        &ctx,
        VIEWPORT,
        FILLER + 50.0,
        Vec::new(),
        Some(rows.filter()),
    );
    let band_at = fills(&pinned.output, tokens.palette.surface)
        .first()
        .map(|(at, _)| *at)
        .expect("the pinned row fills its box");
    let play_at = fills(&pinned.output, tokens.palette.accent)
        .first()
        .map(|(at, _)| *at)
        .expect("the row paints its play tile");
    let title_at = text_at(&pinned.output, "Late Night Drive")
        .first()
        .map(|(at, _)| *at)
        .expect("the pinned row paints the context's name");
    let field_at = text_at(&pinned.output, "midnight")
        .first()
        .map(|(at, _)| *at)
        .expect("the field paints its query");
    assert!(band_at < play_at, "the fill is under the play tile");
    assert!(band_at < title_at, "the fill is under the name");
    assert!(band_at < field_at, "the fill is under the field's text");

    // And the placeholder too, on a row with no query in it.
    let mut quiet = Rows::new();
    let pinned = frame_in(
        &ctx,
        VIEWPORT,
        FILLER + 50.0,
        Vec::new(),
        Some(quiet.filter()),
    );
    let band_at = fills(&pinned.output, tokens.palette.surface)
        .first()
        .map(|(at, _)| *at)
        .expect("the pinned row fills its box");
    let placeholder_at = text_at(&pinned.output, "FILTER")
        .first()
        .map(|(at, _)| *at)
        .expect("the field paints its placeholder");
    assert!(
        band_at < placeholder_at,
        "the fill is under the field's placeholder"
    );
}

#[test]
fn the_title_is_a_label_and_a_rename_never_moves_into_it() {
    let ctx = context();
    let offset = FILLER + 50.0;
    let pinned = frame(&ctx, offset, Vec::new());
    let (_, label) = text_at(&pinned.output, "Late Night Drive")
        .first()
        .copied()
        .expect("the pinned row paints the context's name");

    // A press on the name is a press on nothing: the row's name is a label,
    // so a rename in flight stays in the band's own field below it.
    let mut last = Reply::quiet();
    for event in click_events(label.center()) {
        last = frame(&ctx, offset, vec![event]).reply;
    }
    assert!(!last.played && !last.shuffled && last.saved.is_none() && last.more_door.is_none());
    assert_eq!(
        ctx.memory(|memory| memory.focused()),
        None,
        "the name takes no keyboard"
    );
}

#[test]
fn the_bar_is_drawn_once_pinned() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let pinned = frame(&ctx, FILLER + 50.0, Vec::new());
    assert_eq!(
        fills(&pinned.output, tokens.palette.surface).len(),
        1,
        "one row, one fill"
    );
    assert_eq!(
        text_at(&pinned.output, "Late Night Drive").len(),
        1,
        "one row, one name"
    );
    let tiles = fills(&pinned.output, tokens.palette.accent);
    assert_eq!(tiles.len(), 1, "one row, one play tile");
    assert_eq!(tiles[0].1.width(), hero::PLAY);
}
