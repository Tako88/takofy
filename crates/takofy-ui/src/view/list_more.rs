//! A block's reveal row: the one control that asks for the rest of a kind.
//!
//! The reference's `.list-more` is a full-width row at a block's foot, a step
//! shorter than a name: the words say how much is still hidden, a small chevron
//! trails them, and the row takes the pointer's wash and the keyboard's ring
//! like every other row. It is a request, not a switch — once taken the caller
//! shows the whole block and stops passing a label, so the row takes itself
//! away.
//!
//! A row centred under a body says where that body grows, and one that has
//! nothing to ask for is a sentence with the row's height and none of its
//! affordances.
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

/// How a reveal row is set: where its words sit, and whether they can be taken.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Look {
    /// The gap between the row's left edge and its label. A centred row keeps
    /// its words off both edges instead, so this is zero there.
    pub inset: f32,
    /// Whether the words and the chevron after them sit on the row's centre,
    /// for a line that belongs to the body above it rather than to the page's
    /// own left edge.
    pub centred: bool,
    /// Whether the row can be taken. An inert row is a sentence: it keeps the
    /// row's height and loses the chevron, the wash, the ring and the press.
    pub live: bool,
}

impl Look {
    /// A live row whose words start `inset` from the row's left edge.
    pub(crate) const fn from_left(inset: f32) -> Self {
        Self {
            inset,
            centred: false,
            live: true,
        }
    }

    /// A row centred under the block it hangs from, live or inert.
    pub(crate) const fn centred(live: bool) -> Self {
        Self {
            inset: 0.0,
            centred: true,
            live,
        }
    }
}

/// Where the row's two pieces go inside its rect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Parts {
    /// The label's left edge, at the row's vertical centre.
    pub label: Pos2,
    /// The glyph's box, after the label and the reference's `S2` gap. A row
    /// that draws none has no box and so nothing to centre.
    pub chevron: Option<Rect>,
}

/// The label where `look` puts it, and the chevron after it when it draws one.
pub(crate) fn parts(row: Rect, label_w: f32, look: Look) -> Parts {
    let side = Size::SM.side;
    let trailing = if look.live { space::S2 + side } else { 0.0 };
    let left = if look.centred {
        row.center().x - (label_w + trailing) / 2.0
    } else {
        row.left() + look.inset
    };
    Parts {
        label: Pos2::new(left, row.center().y),
        chevron: look.live.then(|| {
            Rect::from_min_size(
                Pos2::new(left + label_w + space::S2, row.center().y - side / 2.0),
                vec2(side, side),
            )
        }),
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
    look: Look,
    tokens: &Tokens,
) -> bool {
    let Some(label) = label else {
        return false;
    };
    let row = rect(block);
    if row.width() <= 0.0 || row.height() <= 0.0 {
        return false;
    }
    let sense = if look.live {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.allocate_rect(row, sense);
    response.widget_info(|| {
        if look.live {
            WidgetInfo::labeled(egui::Role::Button, true, label)
        } else {
            WidgetInfo::labeled(egui::Role::Label, false, label)
        }
    });

    // The reference lights the row on hover and on a visible keyboard focus,
    // and the words step up from muted to the page's text with it. An inert
    // row is dimmer still and never lights.
    let lit = look.live && (response.hovered() || response.has_focus());
    let color = if lit {
        tokens.palette.text
    } else if look.live {
        tokens.palette.text_muted
    } else {
        tokens.palette.text_subtle
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
        (row.width() - look.inset * 2.0).max(0.0),
    );
    let parts = parts(row, line.size().x, look);
    ui.painter().galley(
        Pos2::new(parts.label.x, parts.label.y - line.size().y / 2.0),
        line,
        color,
    );
    if let Some(chevron) = parts.chevron {
        icon::paint(ui.painter(), Icon::ChevronDown, chevron, Size::SM, color);
    }

    if !look.live {
        return false;
    }
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
