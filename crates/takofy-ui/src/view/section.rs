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
pub fn head(ui: &mut Ui, title: &str, aside: &str, tokens: &Tokens) -> Rect {
    paint(ui, title, aside, None, tokens)
}

/// [`head`], announced as one label reading `spoken`, so the aside is heard with
/// its title.
pub fn head_spoken(ui: &mut Ui, title: &str, aside: &str, spoken: &str, tokens: &Tokens) -> Rect {
    paint(ui, title, aside, Some(spoken), tokens)
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

fn paint(ui: &mut Ui, title: &str, aside: &str, spoken: Option<&str>, tokens: &Tokens) -> Rect {
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
    rect
}

#[cfg(test)]
mod tests;
