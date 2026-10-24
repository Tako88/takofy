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
mod tests {
    use egui::{Pos2, Rect, pos2, vec2};

    use super::*;
    use crate::menu::Anchor;
    use crate::theme::{extent, space};

    fn row() -> Rect {
        Rect::from_min_size(pos2(100.0, 200.0), vec2(extent::MENU_MIN_W, extent::ROW_H))
    }

    #[test]
    fn a_rows_parts_stay_inside_the_row() {
        let row = row();
        let parts = row_parts(row, 30.0);
        for part in [parts.icon, parts.label, parts.trailing] {
            assert!(
                row.contains_rect(part) || part.width() == 0.0,
                "{part:?} is not inside {row:?}"
            );
        }
        assert_eq!(parts.icon.left(), row.left() + space::S3);
        assert_eq!(parts.icon.width(), parts.icon.height());
        assert_eq!(parts.trailing.right(), row.right() - space::S3);
        assert_eq!(parts.trailing.height(), row.height() - 2.0 * space::S1);
        assert_eq!(parts.label.right(), parts.trailing.left() - space::S4);
        assert_eq!(parts.label.left(), parts.icon.right() + space::S3);
        assert!(
            parts.icon.right() <= parts.label.left()
                && parts.label.right() <= parts.trailing.left(),
            "the parts are not left to right: {parts:?}"
        );
    }

    #[test]
    fn a_row_narrower_than_its_parts_keeps_them_from_inverting() {
        for width in [64.0, 24.0, 4.0, 0.0] {
            let row = Rect::from_min_size(pos2(100.0, 200.0), vec2(width, extent::ROW_H));
            let parts = row_parts(row, 30.0);
            for part in [parts.icon, parts.label, parts.trailing] {
                assert!(part.width() >= 0.0, "{part:?} inverted at {width}");
                assert!(part.height() >= 0.0, "{part:?} inverted at {width}");
            }
            assert!(row.contains_rect(parts.icon) || parts.icon.width() == 0.0);
            assert!(row.contains_rect(parts.trailing) || parts.trailing.width() == 0.0);
        }
    }

    #[test]
    fn every_row_of_a_placed_card_lands_on_the_grid() {
        let window = Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0));
        for y in [0.0, 100.3, 421.77, 860.65] {
            let button = Rect::from_min_size(pos2(600.0, y), vec2(28.0, 28.0));
            let card = menu::place(
                Anchor::Button(button),
                vec2(extent::MENU_MIN_W, menu::rows_height(3)),
                window,
            );
            for edge in [card.left(), card.right(), card.top(), card.bottom()] {
                let grid = edge / (1.0 / 32.0);
                assert!((grid - grid.round()).abs() < 1e-3, "{edge} is off the grid");
            }
            for index in 0..3 {
                let row = menu::row_rect(card, index);
                assert_eq!(row.left(), card.left());
                assert_eq!(row.width(), card.width());
                assert!(window.contains_rect(row), "{row:?} left the window");
            }
        }
        // A button at the window's foot flips the card above it, which is the
        // whole reason the list uses the menu's placement.
        let button = Rect::from_min_size(pos2(600.0, 860.0), vec2(28.0, 28.0));
        let card = menu::place(
            Anchor::Button(button),
            vec2(extent::MENU_MIN_W, menu::rows_height(3)),
            window,
        );
        assert!(
            card.bottom() <= button.top() + 0.01,
            "{card:?} did not flip above {button:?}"
        );
    }
}
