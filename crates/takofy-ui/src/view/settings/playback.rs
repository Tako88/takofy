//! The Playback section: the one preference the page carries as a switch.
//!
//! The switch is the queue's own state, so a press moves the queue and the
//! preferences file follows through the shell's write, not through this page.

use egui::{Pos2, Rect, Role, Sense, Stroke, StrokeKind, Ui, Vec2, vec2};

use crate::availability::Control;
use crate::theme::{Tokens, extent, radius, space, stroke};
use crate::view::{controls, section};

use super::{Presses, block, body_face, line, measure, paint, title_face};

/// The row's name, in the reference's own words.
const NAME: &str = "Keep playing after the queue ends";

/// The row's sentence: what turning it on does.
const LINE: &str = "When a playlist runs out, carry on with something near it.";

/// Draws the section's one row and the switch at its trailing edge.
pub(super) fn show(ui: &mut Ui, autoplay: bool, tokens: &Tokens, presses: &mut Presses) {
    section::head(ui, "Playback", "", tokens);

    let width = ui.available_width();
    let text_w = (width - extent::SWITCH_W - space::S4).max(0.0);
    let name = line(ui, NAME, title_face(), tokens.palette.text, 0.0, text_w);
    let font = body_face();
    let line_w = text_w.min(measure(ui, &font, extent::SETTINGS_TEXT_CH));
    let body = block(ui, LINE, font, tokens.palette.text_muted, line_w, 2);
    let height = (name.size().y + space::S1 + body.size().y).max(extent::SWITCH_H);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());

    let top = rect.center().y - (name.size().y + space::S1 + body.size().y) / 2.0;
    paint(ui, Pos2::new(rect.left(), top), &name, tokens.palette.text);
    paint(
        ui,
        Pos2::new(rect.left(), top + name.size().y + space::S1),
        &body,
        tokens.palette.text_muted,
    );

    let track = Rect::from_min_size(
        Pos2::new(
            rect.right() - extent::SWITCH_W,
            rect.center().y - extent::SWITCH_H / 2.0,
        ),
        vec2(extent::SWITCH_W, extent::SWITCH_H),
    );
    switch(ui, tokens, track, autoplay, presses);
}

/// The switch: a square thumb on a hairline track.
fn switch(ui: &mut Ui, tokens: &Tokens, rect: Rect, on: bool, presses: &mut Presses) {
    let label = if on {
        "Keep playing after the queue ends: on"
    } else {
        "Keep playing after the queue ends: off"
    };

    let border = if on {
        tokens.palette.text
    } else {
        tokens.palette.border_strong
    };
    if on {
        ui.painter()
            .rect_filled(rect, radius::XS, tokens.palette.text);
    }
    ui.painter().rect_stroke(
        rect,
        radius::XS,
        Stroke::new(stroke::HAIRLINE, border),
        StrokeKind::Inside,
    );

    let inset = (extent::SWITCH_H - extent::SWITCH_THUMB) / 2.0;
    let left = if on {
        rect.right() - inset - extent::SWITCH_THUMB
    } else {
        rect.left() + inset
    };
    let thumb = Rect::from_min_size(
        Pos2::new(left, rect.top() + inset),
        Vec2::splat(extent::SWITCH_THUMB),
    );
    let ink = if on {
        tokens.palette.text_inverse
    } else {
        tokens.palette.text_muted
    };
    ui.painter().rect_filled(thumb, radius::XS, ink);

    let slot = controls::slot(
        ui,
        Control::AutoplaySwitch,
        Role::Button,
        rect,
        label,
        tokens,
        true,
    );
    if slot.response.clicked() {
        presses.autoplay = Some(!on);
    }
}
