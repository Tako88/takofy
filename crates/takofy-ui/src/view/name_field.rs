//! The one text field the window draws for a playlist name.
//!
//! It owns the keyboard rules every name field needs: Enter and Escape are read
//! and consumed before egui's own handling, focus is taken on request, and a
//! write in flight is read-only. The caller paints the surface and decides what
//! a key means.

use egui::{Align2, Color32, FontId, Id, Margin, Rect, Stroke, Ui, UiBuilder};
use takofy_model::PLAYLIST_NAME_MAX;

use super::keys::{FIELD_FILTER, pressed_once};

/// What one field draws and which keys it should hand back.
pub(crate) struct Spec<'a> {
    pub id: Id,
    pub rect: Rect,
    pub text: &'a mut String,
    pub writing: bool,
    pub wants_focus: bool,
    pub font: FontId,
    pub hint: &'a str,
    pub inset: Margin,
    pub text_color: Color32,
    pub weak_color: Color32,
    pub cursor: Stroke,
    pub align: Align2,
}

/// What the keyboard and focus did this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Out {
    pub enter: bool,
    pub escape: bool,
    pub focused: bool,
    pub lost_focus: bool,
}

/// Draws the field and reports the keys it took.
pub(crate) fn edit(ui: &mut Ui, spec: Spec<'_>) -> Out {
    let Spec {
        id,
        rect,
        text,
        writing,
        wants_focus,
        font,
        hint,
        inset,
        text_color,
        weak_color,
        cursor,
        align,
    } = spec;
    if rect.width() <= 0.0 {
        return Out::default();
    }

    let focused_before = ui.memory(|memory| memory.has_focus(id));
    let (enter, escape) = if focused_before && !writing {
        ui.input(|input| {
            (
                pressed_once(input, egui::Key::Enter),
                pressed_once(input, egui::Key::Escape),
            )
        })
    } else {
        (false, false)
    };
    if focused_before {
        ui.input_mut(|input| {
            if enter {
                input.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
            }
            if escape {
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
            }
        });
    }
    if wants_focus {
        ui.memory_mut(|memory| memory.request_focus(id));
    }
    if focused_before {
        // egui stores a filter only for a widget that had focus last frame, so
        // this takes effect on the frame after the field first holds the
        // keyboard. Without it the field would lose Escape to egui's own focus
        // handling, and `request_focus` stores a default filter.
        ui.memory_mut(|memory| memory.set_focus_lock_filter(id, FIELD_FILTER));
    }

    let mut focused = focused_before;
    let lost_focus;
    if writing {
        let at = egui::pos2(rect.left() + inset.left as f32, rect.center().y);
        ui.painter()
            .text(at, Align2::LEFT_CENTER, text, font, weak_color);
        lost_focus = false;
    } else {
        let mut child = ui.new_child(
            UiBuilder::new()
                .id_salt(id.with("text"))
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.set_clip_rect(rect);
        child.visuals_mut().weak_text_color = Some(weak_color);
        child.visuals_mut().text_cursor.stroke = cursor;

        let output = egui::TextEdit::singleline(text)
            .id(id)
            .frame(egui::Frame::NONE.inner_margin(inset))
            .min_size(rect.size())
            .desired_width(rect.width())
            .align(align)
            .font(font)
            .text_color(text_color)
            .event_filter(FIELD_FILTER)
            .hint_text(egui::RichText::new(hint))
            .char_limit(PLAYLIST_NAME_MAX)
            .show(&mut child);
        focused = output.response.has_focus();
        lost_focus = output.response.lost_focus();
    }

    Out {
        enter,
        escape,
        focused,
        lost_focus,
    }
}
