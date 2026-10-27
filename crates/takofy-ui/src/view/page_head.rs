//! The page head: the title, the note at the far end and the rule under both.
//!
//! The reference sets the title at `--text-2xl` in 800 and the note as a small
//! tracked mono line sitting on the title's baseline, then closes the head with
//! a hairline and one `S5` of air. Above the title it asks for one `S5` too, and
//! the head brings that air itself: a page that draws a band instead must start
//! flush with the bar, so the inset cannot belong to the page.

use egui::{Pos2, Rect, Sense, Ui, vec2};

use crate::theme::{Tokens, space, stroke, text, tracking};

use super::text as recipe;

/// Draws the head and leaves the cursor a step below its rule.
///
/// The title gives way to the note, so a long one elides instead of running
/// under it. One label node speaks both, as `title, note`.
pub(super) fn show(ui: &mut Ui, title: &str, note: &str, tokens: &Tokens) {
    ui.add_space(space::S3);
    let width = ui.available_width();
    let font = recipe::page_title();
    let title_h = ui.ctx().fonts_mut(|fonts| fonts.row_height(&font));
    let height = space::S3 + title_h + space::S4;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let spoken = if note.is_empty() {
        title.to_owned()
    } else {
        format!("{title}, {note}")
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, spoken.clone()));

    let note_line = (!note.is_empty()).then(|| recipe::page_note_galley(ui, note, tokens));
    let title_w = match &note_line {
        Some(line) => (width - line.size().x - space::S4).max(0.0),
        None => width,
    };

    let top = rect.top() + space::S3;
    let line = recipe::line(
        ui,
        title,
        font,
        tokens.palette.text,
        recipe::tracking_pts(text::XXL, tracking::TIGHT),
        title_w,
    );
    ui.painter()
        .galley(Pos2::new(rect.left(), top), line, tokens.palette.text);

    if let Some(line) = note_line {
        ui.painter().galley(
            Pos2::new(rect.right() - line.size().x, top + title_h - line.size().y),
            line,
            tokens.palette.text_subtle,
        );
    }

    ui.painter().rect_filled(
        Rect::from_min_size(
            Pos2::new(rect.left(), rect.bottom() - stroke::HAIRLINE),
            vec2(rect.width(), stroke::HAIRLINE),
        ),
        0.0,
        tokens.palette.border,
    );
    ui.add_space(space::S5);
}

#[cfg(test)]
mod tests;
