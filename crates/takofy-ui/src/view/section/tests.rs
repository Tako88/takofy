use egui::{Pos2, RawInput, Rect, Shape};

use super::*;

/// The probe's screen width, which is the head's own width.
const HEAD_W: f32 = 600.0;

fn run(draw: impl Fn(&mut Ui, &Tokens)) -> (egui::FullOutput, f32) {
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.enable_accesskit();
    let mut out = None;
    for _ in 0..2 {
        let mut moved = 0.0;
        let mut output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(HEAD_W, 400.0))),
                ..Default::default()
            },
            |ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                let top = ui.cursor().top();
                draw(ui, &Tokens::DARK);
                moved = ui.cursor().top() - top;
            },
        );
        output.textures_delta.clear();
        out = Some((output, moved));
    }
    out.expect("two passes")
}

fn labels(output: &egui::FullOutput) -> Vec<String> {
    output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("accesskit is on")
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == egui::accesskit::Role::Label)
        .filter_map(|(_, node)| node.value().map(str::to_owned))
        .collect()
}

#[test]
fn a_plain_head_adds_no_node() {
    let (output, _) = run(|ui, tokens| head(ui, "Tracks", "10 of 20", tokens));
    assert!(labels(&output).is_empty());
}

#[test]
fn a_spoken_head_is_one_label_with_exactly_that_text() {
    let (output, _) = run(|ui, tokens| {
        head_spoken(ui, "Tracks", "10 of 20", "Tracks, 10 of 20", tokens);
    });
    assert_eq!(labels(&output), ["Tracks, 10 of 20"]);
}

#[test]
fn a_cluster_that_does_not_fit_takes_the_line_under_the_title() {
    // A cluster as wide as the head leaves the title nothing beside it.
    let size = Vec2::new(HEAD_W, crate::theme::extent::BUTTON);
    let placed = std::cell::Cell::new(Rect::ZERO);
    let (output, moved) = run(|ui, tokens| {
        placed.set(head_chips(ui, "Episodes", size, tokens));
    });
    let placed = placed.get();
    let title = output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(text) if text.galley.text() == "Episodes" => {
                Some(Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
        .expect("the title is painted");

    assert!(
        placed.top() > title.bottom(),
        "the cluster sits under the title: {placed:?} against {title:?}"
    );
    assert_eq!(placed.width(), size.x);
    assert!(
        (placed.right() - HEAD_W).abs() < 0.01,
        "and still flush right: {placed:?}"
    );
    assert!(
        moved >= title.height() + size.y,
        "the head grew by the title and the cluster's line: {moved}"
    );
}

#[test]
fn a_divide_is_a_margin_a_hairline_and_padding() {
    let (output, moved) = run(divide);
    assert!((moved - (space::S6 + stroke::HAIRLINE + space::S5)).abs() < 1e-3);
    let rules = output
        .shapes
        .iter()
        .filter(|clipped| {
            matches!(&clipped.shape, Shape::Rect(rect)
                if rect.fill == Tokens::DARK.palette.border)
        })
        .count();
    assert_eq!(rules, 1);
}
