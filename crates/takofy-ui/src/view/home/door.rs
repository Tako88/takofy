//! A shelf's foot: the one row that opens the rest of what it drew from.
//!
//! The reference calls it `.list-more` — a full-width row in the same sentence
//! voice as a menu's floor action, muted until the pointer is on it. It is the
//! shelf's own door, so a shelf whose body is a sample can say where the rest is
//! without putting a count on the title.

use egui::{Pos2, Rect, Sense, Ui, pos2, vec2};

use crate::icon::{self, Icon};
use crate::theme::icon::Size;
use crate::theme::{Tokens, extent, radius, space};

use super::super::{controls, text as recipe};

/// Draws the door and reports whether it was pressed.
pub(super) fn show(ui: &mut Ui, label: &str, tokens: &Tokens) -> bool {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, extent::DROP_HIT), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label.to_owned()));
    if response.clicked() {
        response.request_focus();
    }
    let over = response.hovered() || ui.rect_contains_pointer(rect);
    if over {
        ui.painter()
            .rect_filled(rect, radius::SM, tokens.palette.row_hover);
    }
    controls::focus_ring(ui, &response, rect, radius::SM, tokens);

    let color = if over {
        tokens.palette.text
    } else {
        tokens.palette.text_muted
    };
    let words = recipe::line(
        ui,
        label,
        recipe::track_secondary(),
        color,
        0.0,
        (width - space::S2 - Size::SM.side).max(0.0),
    );
    let top = rect.center().y - words.size().y / 2.0;
    let left = rect.left() + space::S1;
    ui.painter()
        .galley(Pos2::new(left, top), words.clone(), color);
    let chevron = Rect::from_center_size(
        pos2(
            left + words.size().x + space::S2 + Size::SM.side / 2.0,
            rect.center().y,
        ),
        vec2(Size::SM.side, Size::SM.side),
    );
    icon::paint(ui.painter(), Icon::ChevronRight, chevron, Size::SM, color);

    response.clicked()
}

#[cfg(test)]
mod tests;
