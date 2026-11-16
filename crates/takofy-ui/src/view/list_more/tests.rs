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
    frame_with(ctx, label, Look::from_left(space::S3), events)
}

/// One frame of a row set the way `look` says.
fn frame_with(
    ctx: &Context,
    label: Option<&str>,
    look: Look,
    events: Vec<Event>,
) -> (bool, FullOutput, f32) {
    let _ = pass_with(ctx, label, look, Vec::new());
    pass_with(ctx, label, look, events)
}

/// One pass of a row set the way `look` says.
fn pass_with(
    ctx: &Context,
    label: Option<&str>,
    look: Look,
    events: Vec<Event>,
) -> (bool, FullOutput, f32) {
    let tokens = Tokens::DARK;
    let mut taken = false;
    let mut moved = 0.0;
    let mut output = ctx.run_ui(input(events), |ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        let top = ui.cursor().top();
        taken = show(ui, block(), label, look, &tokens);
        moved = ui.cursor().top() - top;
    });
    output.textures_delta.clear();
    (taken, output, moved)
}

/// The colour the words were painted in.
fn ink(output: &FullOutput, label: &str) -> Option<egui::Color32> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(text) if text.galley.text() == label => Some(text.fallback_color),
            _ => None,
        })
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
    let parts = parts(row, 100.0, Look::from_left(space::S3));
    assert_eq!(parts.label.x, row.left() + space::S3);
    assert_eq!(
        parts.label.y,
        row.center().y,
        "the label sits on the centre"
    );
    let chevron = parts.chevron.expect("a live row draws its chevron");
    assert_eq!(
        chevron.left(),
        parts.label.x + 100.0 + space::S2,
        "the chevron follows the label after one gap"
    );
    assert_eq!(chevron.size(), Vec2::splat(Size::SM.side));
    assert_eq!(chevron.center().y, row.center().y);
}

#[test]
fn a_centred_row_puts_its_words_and_their_chevron_on_the_centre_line() {
    let row = rect(block());
    let live = parts(row, 100.0, Look::centred(true));
    let chevron = live.chevron.expect("a live line still trails its chevron");
    assert_eq!(live.label.y, row.center().y);
    assert_eq!(
        (live.label.x + chevron.right()) / 2.0,
        row.center().x,
        "the words and the chevron they trail are centred as one group"
    );

    let quiet = parts(row, 100.0, Look::centred(false));
    assert_eq!(quiet.chevron, None, "a sentence draws no chevron");
    assert_eq!(
        quiet.label.x + 50.0,
        row.center().x,
        "and its words are centred on their own"
    );
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
fn an_inert_row_is_a_sentence_that_takes_no_press() {
    let tokens = Tokens::DARK;
    let ctx = context();
    let at = rect(block()).center();
    let words = "Nothing deeper from here";

    let (taken, hovered, _) = frame_with(
        &ctx,
        Some(words),
        Look::centred(false),
        vec![Event::PointerMoved(at)],
    );
    assert!(!taken, "a sentence is not a control");
    assert_eq!(
        ink(&hovered, words),
        Some(tokens.palette.text_subtle),
        "the sentence keeps the dim ink"
    );
    assert!(
        !fills(&hovered).contains(&tokens.palette.row_hover),
        "an inert row draws no wash under the pointer: {:?}",
        fills(&hovered)
    );

    let mut taken = Vec::new();
    for event in crate::view::test_support::click_events(at) {
        taken.push(frame_with(&ctx, Some(words), Look::centred(false), vec![event]).0);
    }
    assert!(
        taken.iter().all(|taken| !taken),
        "no pass of a click takes an inert row: {taken:?}"
    );
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

#[test]
fn a_quiet_row_is_a_left_aligned_sentence_that_takes_no_press() {
    let ctx = context();
    let words = "Nothing beside this one";
    let look = Look::quiet(space::S1);
    // The first pass is the one that carries the whole tree.
    let (taken, output, _) = pass_with(&ctx, Some(words), look, Vec::new());
    assert!(!taken, "a sentence is not a control");

    let parts = parts(rect(block()), 100.0, look);
    assert_eq!(
        parts.label.x,
        rect(block()).left() + space::S1,
        "a quiet row keeps the page's left edge rather than a centre"
    );
    assert_eq!(parts.chevron, None, "and draws no chevron");

    // A label node carries its words as the value, not as a label.
    let labels = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("accesskit is on")
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == egui::accesskit::Role::Label)
        .filter_map(|(_, node)| node.value())
        .collect::<Vec<_>>();
    assert_eq!(labels, [words.to_owned()]);

    let at = rect(block()).center();
    for event in crate::view::test_support::click_events(at) {
        let (taken, _, _) = frame_with(&ctx, Some(words), look, vec![event]);
        assert!(!taken, "no pass of a click takes a quiet row");
    }
}
