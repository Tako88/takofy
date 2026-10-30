use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};

use super::*;
use crate::theme::{Tokens, fonts, style};

const PROBE: Vec2 = Vec2::new(600.0, 300.0);

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

fn run(ctx: &Context) -> egui::FullOutput {
    let tokens = Tokens::DARK;
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        show(ui, &tokens);
    });
    output.textures_delta.clear();
    output
}

#[test]
fn the_foot_draws_the_note_and_a_rule_above_it() {
    let ctx = context();
    let output = run(&ctx);
    let mut words = Vec::new();
    let mut rects = 0;
    for clipped in &output.shapes {
        match &clipped.shape {
            egui::epaint::Shape::Text(text) => words.push(text.galley.text().to_owned()),
            egui::epaint::Shape::Rect(_) => rects += 1,
            egui::epaint::Shape::Vec(pieces) => {
                for piece in pieces {
                    match piece {
                        egui::epaint::Shape::Text(text) => {
                            words.push(text.galley.text().to_owned())
                        }
                        egui::epaint::Shape::Rect(_) => rects += 1,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    assert!(
        words.iter().any(|word| word == HOME_COLOPHON),
        "the foot says what the page is made of: {words:?}"
    );
    assert!(rects > 0, "the note sits under a rule");
}

#[test]
fn the_foot_counts_nothing() {
    let ctx = context();
    let output = run(&ctx);
    let mut words = Vec::new();
    for clipped in &output.shapes {
        if let egui::epaint::Shape::Text(text) = &clipped.shape {
            words.push(text.galley.text().to_owned());
        }
    }
    assert!(
        !words.iter().any(|word| word.contains("shelves")),
        "the page printed a count it was told to drop: {words:?}"
    );
}
