use egui::{Context, Pos2, RawInput, Rect, Vec2};

use super::*;
use crate::theme::{Tokens, fonts, style};

const PROBE: Vec2 = Vec2::new(600.0, 400.0);

fn context() -> Context {
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx
}

#[test]
fn the_page_ends_in_air_rather_than_against_its_own_edge() {
    let ctx = context();
    let mut before = 0.0;
    let mut after = 0.0;
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, PROBE)),
            ..Default::default()
        },
        |ui| {
            before = ui.cursor().top();
            show(ui);
            after = ui.cursor().top();
        },
    );
    output.textures_delta.clear();

    assert_eq!(
        after - before,
        space::S6,
        "the foot is the room under the last shelf"
    );
}
