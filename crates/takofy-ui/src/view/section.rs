//! A section's head: its title, and the aside the reference sets at the far end.
//!
//! The artist page's two sections — Popular and Discography — share this. It is
//! the reference's `.section__head`: a bold title at the `lg` size on the left,
//! an uppercase mono aside on the right, and one step of space below.

use egui::{FontId, Pos2, Rect, Sense, Ui, vec2};

use crate::theme::{Tokens, Weight, fonts, space, stroke, text, tracking};

use super::text as recipe;

/// Draws a section head and leaves the cursor a step below it.
///
/// An empty aside takes no room and draws nothing, so a section with nothing to
/// count still has its title.
pub fn head(ui: &mut Ui, title: &str, aside: &str, tokens: &Tokens) {
    paint(ui, title, aside, None, tokens);
}

/// [`head`], announced as one label reading `spoken`, so the aside is heard with
/// its title.
pub fn head_spoken(ui: &mut Ui, title: &str, aside: &str, spoken: &str, tokens: &Tokens) {
    paint(ui, title, aside, Some(spoken), tokens);
}

/// The rule between two sections: the reference's `.section` margin, rule and
/// padding.
pub fn divide(ui: &mut Ui, tokens: &Tokens) {
    let height = space::S6 + stroke::HAIRLINE + space::S5;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let rule = Rect::from_min_size(
        Pos2::new(rect.left(), rect.top() + space::S6),
        vec2(rect.width(), stroke::HAIRLINE),
    );
    ui.painter().rect_filled(rule, 0.0, tokens.palette.border);
}

fn paint(ui: &mut Ui, title: &str, aside: &str, spoken: Option<&str>, tokens: &Tokens) {
    let width = ui.available_width();
    let line = recipe::line(
        ui,
        title,
        FontId::new(text::LG, fonts::sans(Weight::Bold)),
        tokens.palette.text,
        recipe::tracking_pts(text::LG, tracking::TIGHT),
        width,
    );
    let height = line.size().y;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    if let Some(spoken) = spoken {
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, spoken));
    }
    ui.painter().galley(rect.min, line, tokens.palette.text);

    if !aside.is_empty() {
        let caption = recipe::caption(ui, aside, tokens);
        ui.painter().galley(
            Pos2::new(
                rect.right() - caption.size().x,
                rect.center().y - caption.size().y / 2.0,
            ),
            caption,
            tokens.palette.text_subtle,
        );
    }

    ui.add_space(space::S4);
}

#[cfg(test)]
mod tests {
    use egui::{Pos2, RawInput, Rect, Shape};

    use super::*;

    fn run(draw: impl Fn(&mut Ui, &Tokens)) -> (egui::FullOutput, f32) {
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.enable_accesskit();
        let mut out = None;
        for _ in 0..2 {
            let mut moved = 0.0;
            let mut output = ctx.run_ui(
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
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
}
