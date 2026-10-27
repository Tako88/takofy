//! The filter field: the one name field the rail's head and a playlist's action
//! row both draw.
//!
//! It is underlined rather than boxed, so the boxed things around it keep their
//! own weight: a magnifier, the editable text, a clear control, and a bottom
//! hairline that goes to full ink while the field has the keyboard. The
//! placeholder is painted by hand rather than handed to egui as a hint, because
//! a hint atom paints the first keystroke a frame late and the rail's
//! placeholder is the library's own count, which changes under the field.
//!
//! The caller owns what is around the field: the row, the pin, the count beside
//! it, the rule under it, and the copy of the count it announces.

use egui::{Align2, Rect, Sense, Stroke, Ui, WidgetInfo, vec2};

use crate::icon::{self, Icon};
use crate::theme::icon::Size;
use crate::theme::{MonoWeight, Tokens, Weight, fonts, radius, space, stroke, text, tracking};

use super::name_field;
use super::text as recipe;

/// How the field's bottom hairline reads while the field is not focused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Rule {
    /// The hairline is painted at rest in the border ink, as the rail's is: the
    /// field is the only thing in its slot and the line says where it ends.
    Resting,
    /// The hairline is painted only while the field holds the keyboard.
    ///
    /// A stuck action row already wears an edge across its whole width, and a
    /// second line under the field on top of it would read as a box the field
    /// does not have.
    FocusOnly,
}

/// What the caller hands the field for one frame.
pub(super) struct Field<'a> {
    /// The widget id, and the id every part of the field is keyed under.
    pub(super) id: egui::Id,
    /// The field's whole box, hairline and clear control included.
    pub(super) rect: Rect,
    /// The text the field edits.
    pub(super) text: &'a mut String,
    /// The most characters the field accepts.
    pub(super) limit: usize,
    /// The resting line, painted upper-cased while the text is empty.
    pub(super) placeholder: &'a str,
    /// What a screen reader calls the field.
    pub(super) label: &'static str,
    /// How the bottom hairline reads while the field is unfocused.
    pub(super) rule: Rule,
}

/// What one frame of the field leaves its caller.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Edited {
    /// Whether the frame changed the text: a keystroke, the clear control, or
    /// an Escape that cleared it.
    pub(super) changed: bool,
}

/// Draws the field and answers whether its text moved.
pub(super) fn show(ui: &mut Ui, field: Field<'_>, tokens: &Tokens) -> Edited {
    let Field {
        id,
        rect,
        text,
        limit,
        placeholder,
        label,
        rule,
    } = field;
    // The text this frame started with, so the caller can tell a keystroke from
    // a redraw. The field caps its own input at `limit`, so this stays small.
    let before = text.clone();

    let glyph = Rect::from_min_size(
        egui::Pos2::new(
            rect.left() + space::S1,
            rect.center().y - Size::SM.side / 2.0,
        ),
        vec2(Size::SM.side, Size::SM.side),
    );
    icon::paint(
        ui.painter(),
        Icon::Search,
        glyph,
        Size::SM,
        tokens.palette.text_subtle,
    );

    // The clear control's box: the field's right padding and the 12px glyph.
    let clear = Rect::from_min_max(
        egui::Pos2::new(
            rect.right() - space::S3 - Size::SM.side - space::S1,
            rect.top(),
        ),
        egui::Pos2::new(rect.right() - space::S1, rect.bottom()),
    );
    // The click is read before the edit, because the edit takes the box back
    // the moment the text empties. The box is laid out every frame and it is
    // not a tab stop: while the text is empty the edit covers it and wins the
    // pointer, so a click there still focuses the field.
    let hit = ui.interact(clear, id.with("clear"), Sense::CLICK);
    let clear_clicked = !text.is_empty() && hit.clicked();
    if clear_clicked {
        text.clear();
    }

    // The clear control keeps its own box whenever the field can be typed
    // into, so a character typed on this frame never runs under the glyph.
    let pressed = ui.input(|input| {
        input.pointer.primary_pressed()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|at| rect.contains(at))
    });
    let typable = !text.is_empty() || pressed || ui.memory(|memory| memory.has_focus(id));
    let edit_rect = if typable {
        Rect::from_min_max(
            rect.min,
            egui::Pos2::new(clear.left() - space::S2, rect.bottom()),
        )
    } else {
        rect
    };
    let out = name_field::edit(
        ui,
        name_field::Spec {
            id,
            rect: edit_rect,
            text,
            writing: false,
            wants_focus: clear_clicked || pressed,
            font: egui::FontId::new(text::XS, fonts::sans(Weight::Regular)),
            hint: "",
            inset: egui::Margin {
                left: (Size::SM.side + space::S2) as i8,
                right: (Size::SM.side + space::S2) as i8,
                top: 0,
                bottom: 0,
            },
            text_color: tokens.palette.text,
            weak_color: tokens.palette.text_subtle,
            cursor: Stroke::new(stroke::HAIRLINE, tokens.palette.text),
            align: Align2::LEFT_CENTER,
            limit,
        },
    );
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(egui::Role::SearchInput);
        node.set_label(label);
    });
    if clear_clicked {
        // The click lands on the clear control, not the edit, so the edit's own
        // request is undone by the pointer. Take the keyboard back with the
        // filter the field's first Escape and arrows need.
        ui.memory_mut(|memory| {
            memory.request_focus(id);
            memory.set_focus_lock_filter(id, super::keys::FIELD_FILTER);
        });
    }
    if out.escape {
        text.clear();
        ui.memory_mut(|memory| memory.surrender_focus(id));
    }

    // The chrome is decided from the text the frame ends with, so no frame
    // paints a placeholder over the character just typed, or a clear control
    // over the field it has just emptied.
    if text.is_empty() {
        let placeholder = recipe::spaced(
            ui,
            &placeholder.to_uppercase(),
            egui::FontId::new(text::XXS, fonts::mono(MonoWeight::Regular)),
            tokens.palette.text_subtle,
            recipe::tracking_pts(text::XXS, tracking::CAPS),
        );
        ui.painter().galley(
            egui::Pos2::new(
                rect.left() + Size::SM.side + space::S2,
                rect.center().y - placeholder.size().y / 2.0,
            ),
            placeholder,
            tokens.palette.text_subtle,
        );
    } else {
        if hit.hovered() {
            ui.painter()
                .rect_filled(clear, radius::XS, tokens.palette.surface_hover);
        }
        let ink = if hit.hovered() {
            tokens.palette.text
        } else {
            tokens.palette.text_subtle
        };
        let mark = Rect::from_center_size(clear.center(), vec2(Size::SM.side, Size::SM.side));
        icon::paint(ui.painter(), Icon::Close, mark, Size::SM, ink);
        hit.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, "Clear filter"));
    }

    // Border only, and the bottom hairline is the whole border: no fill, no
    // glow, and no width change. The colour is the focus the frame ends with, so
    // a clear click that took the keyboard, and an Escape that gave it back,
    // both show on their own frame. A field whose rule is focus-only paints
    // nothing at all at rest, and the row it sits in carries the edge instead.
    let focused = ui.memory(|memory| memory.has_focus(id));
    let line = match (rule, focused) {
        (_, true) => Some(tokens.palette.text),
        (Rule::Resting, false) => Some(tokens.palette.border),
        (Rule::FocusOnly, false) => None,
    };
    if let Some(ink) = line {
        ui.painter().rect_filled(
            Rect::from_min_size(
                egui::Pos2::new(rect.left(), rect.bottom() - stroke::HAIRLINE),
                vec2(rect.width(), stroke::HAIRLINE),
            ),
            0.0,
            ink,
        );
    }

    Edited {
        changed: before != *text,
    }
}

#[cfg(test)]
mod tests;
