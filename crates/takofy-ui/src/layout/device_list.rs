//! The device list's geometry: one device row's glyph, its name and its kind.
//!
//! The card is a menu's: it takes the menu's width, its `extent::ROW_H` rows and
//! its placement, so a device list lands and sizes exactly where a command menu
//! below the same button would. Only the row grammar is its own — a glyph, a
//! name, and the kind word where a command row puts a key hint.
//!
//! A row loses its glyph rather than its name: on a row too narrow for both, the
//! slots are clamped into the row and the parts stay zero-width instead of
//! inverting.

use egui::{Rect, pos2, vec2};

use super::menu;
use crate::theme::{icon, space};

/// One device row's parts, left to right.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Parts {
    /// The devices glyph, in the accent for the holder and `text_subtle` for
    /// the rest.
    pub icon: Rect,
    /// The device's name.
    pub label: Rect,
    /// The kind word, in the slot a command row gives its key hint.
    pub trailing: Rect,
}

/// Splits one device row into its glyph, its name and its kind word.
///
/// The trailing slot takes its width from the row's right edge inward, exactly
/// as a key hint does, and the glyph then takes its own square from the label's
/// left; the name keeps what is left of the middle.
pub fn row_parts(row: Rect, trailing_width: f32) -> Parts {
    let parts = menu::command_parts(row, Some(trailing_width), false);
    // A glyph that does not fit the row it is on would paint over the card's
    // edge, so it shrinks to the row's own room.
    let side = icon::Size::SM
        .side
        .min(parts.label.width())
        .min(parts.label.height());
    let glyph = Rect::from_min_size(
        pos2(parts.label.left(), parts.label.center().y - side / 2.0),
        vec2(side, side),
    );
    let label_left = (glyph.right() + space::S3).min(parts.label.right());
    Parts {
        icon: glyph,
        label: Rect::from_min_max(pos2(label_left, parts.label.top()), parts.label.max),
        trailing: parts.key.unwrap_or(Rect::ZERO),
    }
}

#[cfg(test)]
mod tests;
