use super::*;
use crate::theme::{extent, fonts, style};
use crate::track_filter::QUERY_MAX;

/// The field's whole height, which is the reference's 1.75rem row.
const FIELD_H: f32 = extent::BUTTON;

/// The field's box in these tests: free-standing, so nothing else decides
/// its geometry.
const RECT: Rect = Rect {
    min: egui::Pos2::new(20.0, 20.0),
    max: egui::Pos2::new(220.0, 20.0 + FIELD_H),
};

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx
}

/// One frame of the field alone, with `events` in the pass.
fn frame(
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    text: &mut String,
) -> (egui::FullOutput, Edited) {
    frame_with(ctx, Rule::Resting, events, text)
}

/// One frame of the field alone under `rule`, with `events` in the pass.
fn frame_with(
    ctx: &egui::Context,
    rule: Rule,
    events: Vec<egui::Event>,
    text: &mut String,
) -> (egui::FullOutput, Edited) {
    let tokens = Tokens::DARK;
    let mut edited = Edited::default();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(240.0, 80.0))),
            events,
            ..Default::default()
        },
        |ui| {
            edited = show(
                ui,
                Field {
                    id: ui.scope_id().with("field"),
                    rect: RECT,
                    text,
                    limit: QUERY_MAX,
                    placeholder: "2 saved",
                    label: "Filter your library",
                    rule,
                },
                &tokens,
            );
        },
    );
    output.textures_delta.clear();
    (output, edited)
}

/// The ink of the hairline painted under the field, if there is one.
fn hairline(output: &egui::FullOutput) -> Option<egui::Color32> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape)
                if (shape.rect.height() - stroke::HAIRLINE).abs() < 0.01
                    && (shape.rect.bottom() - RECT.bottom()).abs() < 0.01
                    && (shape.rect.width() - RECT.width()).abs() < 0.01 =>
            {
                Some(shape.fill)
            }
            _ => None,
        })
}

/// Whether a line of text was painted at all.
fn painted(output: &egui::FullOutput, label: &str) -> bool {
    output.shapes.iter().any(|clipped| {
        matches!(&clipped.shape,
            egui::epaint::Shape::Text(text) if text.galley.text() == label)
    })
}

/// The clear control's centre, solved the way the field places it.
fn clear_point() -> egui::Pos2 {
    egui::Pos2::new(
        RECT.right() - space::S1 - (space::S3 + Size::SM.side) / 2.0,
        RECT.center().y,
    )
}

#[test]
fn the_placeholder_is_painted_upper_cased_only_while_the_text_is_empty() {
    let ctx = context();
    let mut text = String::new();

    let (output, edited) = frame(&ctx, Vec::new(), &mut text);
    assert!(!edited.changed, "a redraw moved the text");
    assert!(painted(&output, "2 SAVED"), "the field paints its line");
    assert!(
        !painted(&output, "2 saved"),
        "the painted line is the upper-cased one"
    );

    // The field takes the keyboard before it can be typed into.
    for event in crate::view::test_support::click_events(RECT.center()) {
        let (_, edited) = frame(&ctx, vec![event], &mut text);
        assert!(!edited.changed, "a click is not a change to the text");
    }
    assert!(ctx.text_edit_focused(), "the click focused the field");

    let (output, edited) = frame(&ctx, vec![egui::Event::Text("har".to_owned())], &mut text);
    assert!(edited.changed, "a keystroke is the caller's to see");
    assert_eq!(text, "har");
    assert!(
        !painted(&output, "2 SAVED"),
        "the placeholder left with the first character"
    );
}

#[test]
fn changed_is_true_on_a_clear_click_and_on_an_escape_clear() {
    // The clear control.
    let ctx = context();
    let mut text = "har".to_owned();
    let _ = frame(&ctx, Vec::new(), &mut text);
    let mut cleared = false;
    for event in crate::view::test_support::click_events(clear_point()) {
        let (_, edited) = frame(&ctx, vec![event], &mut text);
        cleared |= edited.changed;
    }
    assert!(cleared, "the clear click did not report the change");
    assert!(text.is_empty(), "the clear click left the text");

    // Escape, in a field that owns the keyboard.
    let ctx = context();
    let mut text = "har".to_owned();
    let _ = frame(&ctx, Vec::new(), &mut text);
    for event in crate::view::test_support::click_events(RECT.center()) {
        let _ = frame(&ctx, vec![event], &mut text);
    }
    assert!(ctx.text_edit_focused(), "the click focused the field");
    // The field's own key filter is stored a frame after the focus lands,
    // so an Escape on the very next frame would belong to nobody.
    let _ = frame(&ctx, Vec::new(), &mut text);

    let (_, edited) = frame(
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        &mut text,
    );
    assert!(edited.changed, "the Escape clear did not report itself");
    assert!(text.is_empty(), "the Escape left the text");
}

#[test]
fn focus_only_paints_no_line_at_rest_and_an_ink_line_when_focused() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let mut text = String::new();

    let (output, _) = frame_with(&ctx, Rule::FocusOnly, Vec::new(), &mut text);
    assert_eq!(
        hairline(&output),
        None,
        "an unfocused field under a stuck row paints no line of its own"
    );

    for event in crate::view::test_support::click_events(RECT.center()) {
        let _ = frame_with(&ctx, Rule::FocusOnly, vec![event], &mut text);
    }
    assert!(ctx.text_edit_focused(), "the click focused the field");
    let (output, _) = frame_with(&ctx, Rule::FocusOnly, Vec::new(), &mut text);
    assert_eq!(
        hairline(&output),
        Some(tokens.palette.text),
        "a focused field says so in full ink, whatever its resting rule"
    );

    // And the line goes again when the field gives the keyboard back.
    let _ = frame_with(
        &ctx,
        Rule::FocusOnly,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        &mut text,
    );
    let (output, _) = frame_with(&ctx, Rule::FocusOnly, Vec::new(), &mut text);
    assert_eq!(hairline(&output), None, "the line leaves with the focus");
}

#[test]
fn resting_rule_is_unchanged() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let mut text = String::new();

    let (output, _) = frame_with(&ctx, Rule::Resting, Vec::new(), &mut text);
    assert_eq!(
        hairline(&output),
        Some(tokens.palette.border),
        "a resting field is still underlined in the border ink"
    );

    for event in crate::view::test_support::click_events(RECT.center()) {
        let _ = frame_with(&ctx, Rule::Resting, vec![event], &mut text);
    }
    let (output, _) = frame_with(&ctx, Rule::Resting, Vec::new(), &mut text);
    assert_eq!(
        hairline(&output),
        Some(tokens.palette.text),
        "and still goes to full ink under the keyboard"
    );
}
