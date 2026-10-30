use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};

use super::*;
use crate::theme::{Tokens, fonts, style};
use crate::view::test_support::click_events;

const PROBE: Vec2 = Vec2::new(600.0, 300.0);

/// The air above the door, so the test's press has a known y.
const TOP: f32 = 20.0;

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

/// One pass over the door, and whether it reported a press.
fn run(ctx: &Context, events: Vec<Event>) -> (bool, egui::FullOutput, Rect) {
    let tokens = Tokens::DARK;
    let mut pressed = false;
    let mut rect = Rect::NOTHING;
    let mut output = ctx.run_ui(input(events), |ui| {
        ui.add_space(TOP);
        pressed |= show(ui, "Your library", &tokens);
        rect = ui.min_rect();
    });
    output.textures_delta.clear();
    (pressed, output, rect)
}

#[test]
fn the_door_draws_its_words_and_a_chevron() {
    let ctx = context();
    let (_, output, _) = run(&ctx, Vec::new());
    let mut words = Vec::new();
    let mut shapes = 0;
    for clipped in &output.shapes {
        match &clipped.shape {
            egui::epaint::Shape::Text(text) => words.push(text.galley.text().to_owned()),
            egui::epaint::Shape::Vec(pieces) => {
                for piece in pieces {
                    match piece {
                        egui::epaint::Shape::Text(text) => {
                            words.push(text.galley.text().to_owned())
                        }
                        _ => shapes += 1,
                    }
                }
            }
            _ => shapes += 1,
        }
    }
    assert!(words.iter().any(|word| word == "Your library"));
    assert!(shapes > 0, "the chevron is painted beside the words");
}

#[test]
fn a_press_is_the_doors_own() {
    let ctx = context();
    let (_, _, rect) = run(&ctx, Vec::new());
    let at = Pos2::new(PROBE.x / 2.0, TOP + extent::DROP_HIT / 2.0);

    let mut pressed = false;
    for event in click_events(at) {
        pressed |= run(&ctx, vec![event]).0;
    }
    assert!(pressed, "a press on the row opens what it names");
    assert!(
        (rect.width() - PROBE.x).abs() < 0.01,
        "the door takes the whole width it is given"
    );
}

#[test]
fn the_door_announces_the_page_it_opens() {
    let ctx = context();
    ctx.enable_accesskit();
    let (_, output, _) = run(&ctx, Vec::new());
    let labels: Vec<String> = output
        .platform_output
        .accesskit_update
        .as_ref()
        .map(|update| {
            update
                .nodes
                .iter()
                .filter_map(|(_, node)| node.label().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        labels.iter().any(|label| label == "Your library"),
        "the row announced nothing: {labels:?}"
    );
}
