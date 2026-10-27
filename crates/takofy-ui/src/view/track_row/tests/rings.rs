use egui::Modifiers;

use super::super::*;
use super::test_support::*;
use crate::theme::Tokens;
use crate::view::test_support::{click_events, rings_in, tab};

/// One click on the row's body, returning the frame after it settled.
fn click_row(ctx: &egui::Context, row: &Row<'_>, modifiers: Modifiers) -> egui::FullOutput {
    let _ = paint_row(ctx, row, Vec::new(), modifiers);
    for event in click_events(body_point()) {
        let _ = paint_row(ctx, row, vec![event], modifiers);
    }
    paint_row(ctx, row, Vec::new(), modifiers)
}

#[test]
fn a_clicked_track_row_wears_no_ring() {
    let ctx = context();
    let row = row_of(1);
    let output = click_row(&ctx, &row, Modifiers::NONE);
    assert!(
        ctx.memory(|memory| memory.focused()).is_some(),
        "the click took the keyboard"
    );
    assert!(
        rings_in(&output).is_empty(),
        "a clicked track row wears no ring: {:?}",
        rings_in(&output)
    );
}

#[test]
fn a_clicked_playing_row_shows_only_its_bar() {
    let ctx = context();
    let row = Row {
        state: State::Playing,
        ..row_of(1)
    };
    let output = click_row(&ctx, &row, Modifiers::NONE);
    assert!(
        rings_in(&output).is_empty(),
        "the sounding row wears no ring"
    );

    let tokens = Tokens::DARK;
    let bar = output.shapes.iter().any(|clipped| {
        matches!(&clipped.shape, egui::epaint::Shape::Rect(shape)
            if shape.fill == tokens.palette.accent
                && (shape.rect.width() - stroke::RULE).abs() < 0.01)
    });
    assert!(bar, "the sounding row still marks its margin");
}

#[test]
fn a_shift_click_selects_without_a_ring() {
    let ctx = context();
    let row = row_of(1);
    let output = click_row(&ctx, &row, Modifiers::SHIFT);
    assert!(
        ctx.memory(|memory| memory.focused()).is_some(),
        "the click took the keyboard"
    );
    assert!(
        rings_in(&output).is_empty(),
        "a shift click wears no ring: {:?}",
        rings_in(&output)
    );
}

#[test]
fn a_drag_lift_on_a_focused_row_wears_no_ring() {
    let ctx = context();
    let row = row_of(1);
    let _ = paint_row(&ctx, &row, Vec::new(), Modifiers::NONE);
    let mut output = paint_row(&ctx, &row, Vec::new(), Modifiers::NONE);
    for event in twitch(body_point(), 7.0) {
        output = paint_row(&ctx, &row, vec![event], Modifiers::NONE);
    }
    assert!(
        ctx.memory(|memory| memory.focused()).is_some(),
        "the lift took the keyboard"
    );
    assert!(
        rings_in(&output).is_empty(),
        "a lifted row wears no ring: {:?}",
        rings_in(&output)
    );
}

#[test]
fn tab_rings_the_row_inside_the_list() {
    let ctx = context();
    let row = row_of(1);
    let _ = paint_row(&ctx, &row, Vec::new(), Modifiers::NONE);
    let [press, release] = tab();
    let _ = paint_row(&ctx, &row, vec![press], Modifiers::NONE);
    let output = paint_row(&ctx, &row, vec![release], Modifiers::NONE);

    let rings = rings_in(&output);
    assert_eq!(rings.len(), 1, "Tab rings one row: {rings:?}");
    assert!(
        body_rect().contains_rect(rings[0]),
        "the ring left the row: {:?}",
        rings[0]
    );
}
