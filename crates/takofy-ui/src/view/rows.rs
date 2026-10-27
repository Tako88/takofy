//! The chrome every list row shares.
//!
//! A row's body is one click target that is also a drag source, it fades the
//! controls it reveals, and it is washed either because it is picked or because
//! the pointer is on it. None of that depends on what the row lists, so it lives
//! here rather than in each row module.

use egui::{Id, PointerButton, Rect, Response, Sense, Ui};

use crate::selection::Press;
use crate::theme::{Tokens, ease, motion, opacity, radius};
use crate::view::pin;

/// How one row stands in its list this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Look {
    /// The row is in the list's selection.
    pub selected: bool,
    /// The row is part of the block that is on its way somewhere else.
    pub lifted: bool,
    /// A block is carried at all, from this list or another.
    pub carrying: bool,
}

/// What interacting with a row's body reported.
pub struct Body {
    pub response: Response,
    /// The press a primary click on the row made.
    pub press: Option<Press>,
    /// Whether this frame began a drag out of the row.
    pub lift: bool,
}

/// Interacts with a row's body: the click target and the drag source both.
pub fn body(ui: &mut Ui, rect: Rect, id: Id, look: Look) -> Body {
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let lift = response.drag_started_by(PointerButton::Primary);
    if response.clicked() || lift {
        // The keyboard follows the pointer, so Tab order continues from the row
        // that was picked.
        response.request_focus();
    }
    if response.has_focus() && !look.carrying {
        // A row reachable by Tab has to come into view when it is reached. While
        // a block is carried the scroll is the pointer's, and revealing a row the
        // list is already leaving would fight it. The seam shifts the request
        // clear of whatever the page pins above the list.
        pin::reveal(ui, rect);
    }
    let press = response
        .clicked_by(PointerButton::Primary)
        .then(|| Press::from_modifiers(ui.input(|input| input.modifiers)));
    Body {
        response,
        press,
        lift,
    }
}

/// How far a row's revealed controls have come up this frame.
pub fn fade(ui: &Ui, id: Id, wanted: bool, look: Look) -> f32 {
    if look.carrying {
        // A carried row steps back whole: a control half up would read as one
        // still waiting to be clicked.
        ui.ctx().animate_bool_with_time(id, false, 0.0)
    } else {
        ui.ctx()
            .animate_bool_with_time_and_easing(id, wanted, motion::FAST, ease::out)
    }
}

/// Fills a row with the selection wash when it is picked, and with the hover wash
/// otherwise. A row never takes both.
pub fn wash(ui: &Ui, rect: Rect, hover: f32, look: Look, tokens: &Tokens) {
    let fill = if look.selected {
        Some(tokens.palette.row_selected)
    } else if hover > 0.0 {
        Some(tokens.palette.row_hover.gamma_multiply(hover))
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, radius::SM, fill);
    }
}

/// Steps a row that is being carried back, and returns the opacity the caller
/// restores with [`Ui::set_opacity`] before it returns.
pub fn dim(ui: &mut Ui, look: Look) -> f32 {
    let before = ui.opacity();
    if look.lifted {
        ui.multiply_opacity(opacity::LIFTED);
    }
    before
}

/// The block's own figures, drawn on a ghost in place of one row's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ghosting<'a> {
    /// How many rows the carried block holds.
    pub count: usize,
    /// The block's running time, where a single row carries its duration.
    pub total: Option<&'a str>,
    /// Whether the target under the pointer refuses the drop.
    pub flat: bool,
}

#[cfg(test)]
mod tests;
