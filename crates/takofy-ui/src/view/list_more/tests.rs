use egui::{Context, Event, FullOutput, Modifiers, RawInput, Shape, Vec2};

use super::*;
use crate::theme::style;

/// The block the row hangs under: a card block's own allocated width, not
/// the widened grid.
fn block() -> Rect {
    Rect::from_min_size(Pos2::new(40.0, 20.0), Vec2::new(600.0, 200.0))
}

fn context() -> Context {
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx.enable_accesskit();
    ctx
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 400.0))),
        events,
        ..Default::default()
    }
}

/// One frame of the row: what it reported, what it painted, and how far it
/// moved the cursor.
///
/// A quiet pass comes first, so the row is already laid out when the events
/// arrive and egui has had a pass to turn accesskit on.
fn frame(ctx: &Context, label: Option<&str>, events: Vec<Event>) -> (bool, FullOutput, f32) {
    let _ = pass(ctx, label, Vec::new());
    pass(ctx, label, events)
}

/// One pass of the row.
fn pass(ctx: &Context, label: Option<&str>, events: Vec<Event>) -> (bool, FullOutput, f32) {
    let tokens = Tokens::DARK;
    let mut taken = false;
    let mut moved = 0.0;
    let mut output = ctx.run_ui(input(events), |ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        let top = ui.cursor().top();
        taken = show(ui, block(), label, space::S3, &tokens);
        moved = ui.cursor().top() - top;
    });
    output.textures_delta.clear();
    (taken, output, moved)
}

/// The fills painted on the row itself.
fn fills(output: &FullOutput) -> Vec<egui::Color32> {
    let row = rect(block());
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(shape) if shape.rect == row => Some(shape.fill),
            _ => None,
        })
        .collect()
}

/// One press of `key`.
fn press(key: Key) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

#[test]
fn the_row_is_the_blocks_width_and_a_row_tall_directly_under_it() {
    let row = rect(block());
    assert_eq!(
        row.left(),
        block().left(),
        "the row starts where the block does"
    );
    assert_eq!(row.width(), block().width(), "and is as wide as it");
    assert_eq!(row.top(), block().bottom(), "the row hangs off its foot");
    assert_eq!(row.height(), extent::ROW_H);
}

#[test]
fn the_label_is_inset_and_the_chevron_trails_it_by_one_gap() {
    let row = rect(block());
    let parts = parts(row, 100.0, space::S3);
    assert_eq!(parts.label.x, row.left() + space::S3);
    assert_eq!(
        parts.label.y,
        row.center().y,
        "the label sits on the centre"
    );
    assert_eq!(
        parts.chevron.left(),
        parts.label.x + 100.0 + space::S2,
        "the chevron follows the label after one gap"
    );
    assert_eq!(parts.chevron.size(), Vec2::splat(Size::SM.side));
    assert_eq!(parts.chevron.center().y, row.center().y);
}

#[test]
fn a_row_with_nothing_hidden_draws_nothing_and_takes_no_room() {
    let ctx = context();
    let (taken, output, moved) = frame(&ctx, None, Vec::new());
    assert!(!taken);
    assert_eq!(moved, 0.0, "an absent row leaves the cursor where it was");
    assert!(output.shapes.is_empty(), "and paints nothing");
}

#[test]
fn the_row_washes_only_while_it_is_lit() {
    let tokens = Tokens::DARK;
    let ctx = context();
    let at = rect(block()).center();
    let (_, lit, _) = frame(
        &ctx,
        Some("Show 10 more tracks"),
        vec![Event::PointerMoved(at)],
    );
    assert!(
        fills(&lit).contains(&tokens.palette.row_hover),
        "a hovered row washes with the row hover: {:?}",
        fills(&lit)
    );

    let ctx = context();
    let (_, dark, _) = frame(&ctx, Some("Show 10 more tracks"), Vec::new());
    assert!(
        !fills(&dark).contains(&tokens.palette.row_hover),
        "a row with the pointer away and no focus washed"
    );
}

#[test]
fn a_click_is_taken_once() {
    let ctx = context();
    let at = rect(block()).center();
    let mut taken = Vec::new();
    for event in crate::view::test_support::click_events(at) {
        taken.push(frame(&ctx, Some("Show 10 more tracks"), vec![event]).0);
    }
    assert_eq!(
        taken.iter().filter(|taken| **taken).count(),
        1,
        "one click is one request: {taken:?}"
    );
}

#[test]
fn a_focused_row_answers_the_keyboards_enter() {
    let ctx = context();
    let at = rect(block()).center();
    for event in crate::view::test_support::click_events(at) {
        let _ = frame(&ctx, Some("Show 10 more tracks"), vec![event]);
    }
    let (taken, _, _) = frame(&ctx, Some("Show 10 more tracks"), vec![press(Key::Enter)]);
    assert!(taken, "Enter on the focused row asks for the rest");
}

#[test]
fn the_rows_node_is_a_button_labelled_with_its_words() {
    let ctx = context();
    let (_, output, _) = frame(&ctx, Some("Show 10 more tracks"), Vec::new());
    let nodes = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("accesskit is on")
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == egui::accesskit::Role::Button)
        .filter_map(|(_, node)| node.label().map(str::to_owned))
        .collect::<Vec<_>>();
    assert_eq!(nodes, ["Show 10 more tracks"]);
}
