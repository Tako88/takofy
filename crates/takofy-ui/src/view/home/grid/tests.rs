use egui::{Pos2, RawInput, Rect, Vec2};

use super::*;
use crate::theme::{Tokens, fonts, style};
use crate::view::media;

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx
}

/// The input a probe pass runs at.
fn input() -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0))),
        ..Default::default()
    }
}

#[test]
fn a_lead_grid_is_one_kind_line_taller_and_still_tiles_the_width() {
    let ctx = context();
    let mut solved = None;
    let mut output = ctx.run_ui(input(), |ui| {
        let plain = solve(ui);
        let lead = solve_lead(ui);
        let line = media::extra_height_stacked(ui) - media::extra_height(ui);
        solved = Some((plain, lead, line));
    });
    output.textures_delta.clear();
    let (plain, lead, line) = solved.expect("both grids are solved");

    assert_eq!(
        lead.columns, plain.columns,
        "a taller card does not move a column"
    );
    assert!(
        (lead.height(1) - plain.height(1) - line).abs() < 0.01,
        "the lead grid's card is one kind line taller: {} vs {} + {line}",
        lead.height(1),
        plain.height(1)
    );
    assert!(
        (lead.card_rect(3).right() - plain.card_rect(3).right()).abs() < 0.01,
        "the four cards still tile the width edge to edge"
    );
}
