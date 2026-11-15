//! The discovery band's switch: the full-width row that turns the band off and
//! back on.
//!
//! It is the reference's `.list-more` row without the chevron, because it asks
//! for a setting rather than for the rest of a list. Off draws it in place of the
//! band, so the setting always has a visible instance on the page it governs.

use egui::{Pos2, Role, Sense, Ui, vec2};

use crate::availability::Control;
use crate::format::discovery_switch_label;
use crate::theme::{Tokens, extent, radius, space};
use crate::view::{controls, text as recipe};

/// Draws the row and returns the setting it asks for.
pub(super) fn show(ui: &mut Ui, enabled: bool, tokens: &Tokens) -> Option<bool> {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, extent::DROP_HIT), Sense::hover());
    let label = discovery_switch_label(enabled);
    let slot = controls::slot(
        ui,
        Control::DiscoverySwitch,
        Role::Button,
        rect,
        label,
        tokens,
        true,
    );
    let over = slot.response.hovered() || ui.rect_contains_pointer(rect);
    if over {
        ui.painter()
            .rect_filled(rect, radius::SM, tokens.palette.row_hover);
    }

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
        (width - space::S2 * 2.0).max(0.0),
    );
    ui.painter().galley(
        Pos2::new(
            rect.left() + space::S1,
            rect.center().y - words.size().y / 2.0,
        ),
        words,
        color,
    );

    slot.response.clicked().then_some(!enabled)
}
