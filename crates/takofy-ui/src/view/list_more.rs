//! A block's reveal row: the one control that asks for the rest of a kind.
//!
//! The reference's `.list-more` is a full-width row at a block's foot, a step
//! shorter than a name: the words say how much is still hidden, a small chevron
//! trails them, and the row takes the pointer's wash and the keyboard's ring
//! like every other row. It is a request, not a switch — once taken the caller
//! shows the whole block and stops passing a label, so the row takes itself
//! away.
//!
//! The geometry is pure, so the row's rect and the place of its parts are
//! tested without a window.

use egui::{FontId, Key, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, WidgetInfo, vec2};

use crate::icon::{self, Icon};
use crate::theme::icon::Size;
use crate::theme::{Tokens, Weight, extent, fonts, radius, space, stroke, text};

use super::controls;
use super::text as recipe;

/// The row's rect: the block's own width, directly under it.
///
/// The caller hands the block's rect, not the block's grid: a card grid is
/// widened past the gutter so the art lines up with the headings, while the row
/// stays inside the page's column.
pub(crate) fn rect(block: Rect) -> Rect {
    Rect::from_min_size(
        Pos2::new(block.left(), block.bottom()),
        vec2(block.width(), extent::ROW_H),
    )
}

/// Where the row's two pieces go inside its rect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Parts {
    /// The label's left edge, at the row's vertical centre.
    pub label: Pos2,
    /// The glyph's box, after the label and the reference's `S2` gap.
    pub chevron: Rect,
}

/// The label at `inset` from the row's left, and the chevron after it.
pub(crate) fn parts(row: Rect, label_w: f32, inset: f32) -> Parts {
    let label = Pos2::new(row.left() + inset, row.center().y);
    let side = Size::SM.side;
    Parts {
        label,
        chevron: Rect::from_min_size(
            Pos2::new(label.x + label_w + space::S2, row.center().y - side / 2.0),
            vec2(side, side),
        ),
    }
}

/// Draws the row under `block` and reports whether it was taken.
///
/// `None` is a block with nothing hidden: no row is drawn and the cursor does
/// not move, which is what makes the row disappear once the rest is shown.
pub(crate) fn show(
    ui: &mut Ui,
    block: Rect,
    label: Option<&str>,
    inset: f32,
    tokens: &Tokens,
) -> bool {
    let Some(label) = label else {
        return false;
    };
    let row = rect(block);
    if row.width() <= 0.0 || row.height() <= 0.0 {
        return false;
    }
    let response = ui.allocate_rect(row, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, label));

    // The reference lights the row on hover and on a visible keyboard focus,
    // and the words step up from muted to the page's text with it.
    let lit = response.hovered() || response.has_focus();
    let color = if lit {
        tokens.palette.text
    } else {
        tokens.palette.text_muted
    };
    if lit {
        ui.painter()
            .rect_filled(row, radius::SM, tokens.palette.row_hover);
        ui.painter().rect_stroke(
            row,
            radius::SM,
            Stroke::new(stroke::HAIRLINE, tokens.palette.border),
            StrokeKind::Inside,
        );
    }

    let line = recipe::line(
        ui,
        label,
        FontId::new(text::SM, fonts::sans(Weight::Regular)),
        color,
        0.0,
        (row.width() - inset * 2.0).max(0.0),
    );
    let parts = parts(row, line.size().x, inset);
    ui.painter().galley(
        Pos2::new(parts.label.x, parts.label.y - line.size().y / 2.0),
        line,
        color,
    );
    icon::paint(
        ui.painter(),
        Icon::ChevronDown,
        parts.chevron,
        Size::SM,
        color,
    );

    controls::focus_ring(ui, &response, row, radius::SM, tokens);
    // The keyboard's Enter is answered beside the pointer's click, so a focused
    // row works with no mouse. The pressed row takes the keyboard with it, so
    // Tab continues from the row that was picked.
    let keyboard = response.has_focus() && ui.input(|input| input.key_pressed(Key::Enter));
    let taken = response.clicked() || keyboard;
    if taken {
        response.request_focus();
    }
    taken
}

#[cfg(test)]
mod tests;
