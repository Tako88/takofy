//! A section's head: its title, and the aside the reference sets at the far end.
//!
//! The artist page's two sections — Popular and Discography — share this, and so
//! does the show page, whose head carries its order chips instead of an aside. It
//! is the reference's `.section__head`: a bold title at the `lg` size on the left,
//! the trailing widgets at the right, and one step of space below.

use egui::{FontId, Pos2, Rect, Sense, Ui, Vec2, vec2};

use crate::layout::section::{self as section_layout, Placement};
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

/// A section head whose trailing edge carries a cluster of `size`.
///
/// The title is drawn exactly as [`head`] draws it. The cluster sits on the
/// title's line, flush right, when the title leaves it room; when it does not,
/// the head grows one line and the cluster takes that line. Either way the
/// returned rect is the cluster's own, and the caller paints in it.
pub fn head_chips(ui: &mut Ui, title: &str, size: Vec2, tokens: &Tokens) -> Rect {
    let width = ui.available_width();
    let line = title_line(ui, title, width, tokens);
    let title_size = line.size();
    let (head, _) = ui.allocate_exact_size(vec2(width, title_size.y.max(size.y)), Sense::hover());
    ui.painter().galley(
        Pos2::new(head.left(), head.center().y - title_size.y / 2.0),
        line,
        tokens.palette.text,
    );

    let chips = match section_layout::place(head, head.left() + title_size.x, size, space::S4) {
        Placement::Beside(chips) => chips,
        Placement::Under(chips) => {
            // The cluster's own line, reserved so whatever follows starts
            // below it; the title keeps the width it asked for.
            let row = Rect::from_min_max(Pos2::new(head.left(), chips.top()), chips.right_bottom());
            ui.allocate_rect(row, Sense::hover());
            chips
        }
    };

    ui.add_space(space::S4);
    chips
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

/// The head's title laid out at `width`, ready to paint.
///
/// Every head draws its title this way, whether or not it carries a cluster.
fn title_line(ui: &Ui, title: &str, width: f32, tokens: &Tokens) -> std::sync::Arc<egui::Galley> {
    recipe::line(
        ui,
        title,
        FontId::new(text::LG, fonts::sans(Weight::Bold)),
        tokens.palette.text,
        recipe::tracking_pts(text::LG, tracking::TIGHT),
        width,
    )
}

fn paint(ui: &mut Ui, title: &str, aside: &str, spoken: Option<&str>, tokens: &Tokens) {
    let width = ui.available_width();
    let line = title_line(ui, title, width, tokens);
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
}
