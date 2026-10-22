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
    // egui stores a filter only for a widget that had focus last frame, so the
    // frame the field first asks for the keyboard cannot store it yet. The next
    // frame does, and still owns the Escape egui would otherwise swallow.
    let asked = asked_last_frame(ui.ctx(), id);
    let (enter_pressed, escape_pressed) = ui.input(|input| {
        (
            pressed_once(input, egui::Key::Enter),
            pressed_once(input, egui::Key::Escape),
        )
    });
    // The Escape that the missing filter surrendered is still the field's: it
    // takes the keyboard back in the same frame it reads that press.
    let repairs = !writing && asked && escape_pressed;
    let owns = !writing && (focused_before || repairs);
    let (enter, escape) = if owns {
        (enter_pressed, escape_pressed)
    } else {
        (false, false)
    };
    if owns {
        ui.input_mut(|input| {
            if enter {
                input.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
            }
            if escape {
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
            }
        });
    }
    if !writing && (wants_focus || repairs) {
        ui.memory_mut(|memory| memory.request_focus(id));
    }
    if focused_before || (!writing && (wants_focus || repairs)) {
        // The filter must exist before the widget has focus, or the first key is swallowed.
        ui.memory_mut(|memory| memory.set_focus_lock_filter(id, FIELD_FILTER));
    }
    if wants_focus && !ui.memory(|memory| memory.had_focus_last_frame(id)) {
        mark_asked(ui.ctx(), id);
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

/// The key under which a field records the frame it asked for the keyboard.
fn ask_key(id: Id) -> Id {
    id.with("focus-asked")
}

/// Whether this field asked for the keyboard on the frame before this one.
fn asked_last_frame(ctx: &egui::Context, id: Id) -> bool {
    let Some(previous) = ctx.cumulative_frame_nr().checked_sub(1) else {
        return false;
    };
    ctx.data(|data| data.get_temp::<u64>(ask_key(id))) == Some(previous)
}

/// Records that this field asked for the keyboard on this frame.
fn mark_asked(ctx: &egui::Context, id: Id) {
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|data| data.insert_temp(ask_key(id), frame));
}

fn submit_id() -> Id {
    Id::unique("library-rename-submit")
}

/// Posts a field's submit for the shell to apply this frame.
pub(crate) fn post_submit(ctx: &egui::Context) {
    ctx.data_mut(|data| data.insert_temp(submit_id(), true));
}

/// The submit posted this frame, read once so it cannot outlive it.
pub(crate) fn take_submit(ctx: &egui::Context) -> bool {
    ctx.data_mut(|data| data.remove_temp::<bool>(submit_id()).unwrap_or(false))
}
