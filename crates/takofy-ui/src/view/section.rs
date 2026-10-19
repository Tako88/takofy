//! A section's head: its title, and the aside the reference sets at the far end.
//!
//! The artist page's two sections — Popular and Discography — share this. It is
//! the reference's `.section__head`: a bold title at the `lg` size on the left,
//! an uppercase mono aside on the right, and one step of space below.

use egui::{FontId, Pos2, Sense, Ui, vec2};

use crate::theme::{Tokens, Weight, fonts, space, text, tracking};

use super::text as recipe;

/// Draws a section head and leaves the cursor a step below it.
///
/// An empty aside takes no room and draws nothing, so a section with nothing to
/// count still has its title.
pub fn head(ui: &mut Ui, title: &str, aside: &str, tokens: &Tokens) {
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
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
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
