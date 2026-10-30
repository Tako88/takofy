use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};

use super::*;
use crate::format::HOME_COUNTED_TITLE;
use crate::theme::{Tokens, fonts, style};
use crate::view::test_support::click_events;

/// A window wide enough for the head and one row of cards.
const PROBE: Vec2 = Vec2::new(900.0, 600.0);

fn context() -> Context {
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, PROBE)),
        events,
        ..Default::default()
    }
}

/// One pass over a failed shelf, drawing the presses it took into `out`.
fn run(ctx: &Context, events: Vec<Event>, out: &mut Pressed) -> egui::FullOutput {
    let tokens = Tokens::DARK;
    let mut output = ctx.run_ui(input(events), |ui| {
        failed(
            ui,
            &tokens,
            HOME_COUNTED_TITLE,
            Failure::Load,
            Slot::Counted,
            out,
        );
    });
    output.textures_delta.clear();
    output
}

/// The text one frame painted, in paint order.
fn painted(output: &egui::FullOutput) -> Vec<&str> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect()
}

/// Where the galley whose text is exactly `label` was painted.
fn text_rect(output: &egui::FullOutput, label: &str) -> Option<Rect> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some(Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
}

#[test]
fn a_failed_shelf_names_its_failure_and_its_retry_answers_its_own_slot() {
    let ctx = context();
    let mut out = Pressed::default();
    let output = run(&ctx, Vec::new(), &mut out);

    let drawn = painted(&output);
    assert_eq!(
        drawn.first(),
        Some(&HOME_COUNTED_TITLE),
        "a failed shelf still names itself"
    );
    assert!(
        drawn.contains(&crate::signin::failure_message(Failure::Load)),
        "the shelf says why it could not draw"
    );

    let pos = text_rect(&output, "Retry")
        .expect("a failure the user can act on offers a retry")
        .center();
    let mut pressed = Pressed::default();
    for event in click_events(pos) {
        let _ = run(&ctx, vec![event], &mut pressed);
    }

    assert_eq!(
        pressed.retry,
        Some(Slot::Counted),
        "the retry was routed to another shelf"
    );
}
