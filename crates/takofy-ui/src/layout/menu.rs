//! A command menu's geometry: where the box lands, and how wide one row is.
//!
//! A button's menu right-aligns to that button, sits `extent::MENU_GAP` below
//! it, and flips above when the window's bottom would cut it. A right-click's
//! menu opens at the pointer instead, and flips left or up when it would run off
//! an edge. Both are clamped into the window in both axes afterward.
//!
//! The width is the longest row's, within the sheet's 12 rem floor and 20 rem
//! cap. A row is the reference's `space-3`/`space-1` padding, its label, and the
//! trailing slot a key hint or a chevron takes.

use egui::emath::GuiRounding as _;
use egui::{Rect, Vec2, pos2, vec2};

use super::search_menu::{is_sane, sane};
use crate::menu::Anchor;
use crate::theme::{extent, icon, space, stroke};

/// One row's measured parts, in points, as the view measured them.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct RowWidth {
    /// The label's own width.
    pub label: f32,
    /// The key hint's width, when the row has one.
    pub key: Option<f32>,
    /// Whether the row opens a submenu, which takes a chevron's box.
    pub chevron: bool,
}

/// The rows tile down from the menu's top at the shared `extent::ROW_H`.
pub use super::search_menu::row_rect;

/// The height of a menu of `rows` rows.
pub fn rows_height(rows: usize) -> f32 {
    super::search_menu::rows_height(rows, false)
}

/// The box a menu wants: its longest row, within the sheet's floor and cap.
pub fn width(rows: &[RowWidth]) -> f32 {
    let widest = rows.iter().map(row_width).fold(0.0, f32::max);
    if widest.is_finite() {
        widest.clamp(extent::MENU_MIN_W, extent::MENU_MAX_W)
    } else {
        extent::MENU_MIN_W
    }
}

/// The rect a menu of `size` takes for `anchor`, inside `window`.
///
/// Right-aligned to the button and one gap below it, flipped above when the
/// window's bottom would cut it, then clamped into the window: a menu taller
/// than the window pins to its top rather than hanging off either edge. A
/// non-finite anchor, window or size falls back to the documented rect, so a bad
/// read draws nothing instead of putting the menu somewhere arbitrary.
pub fn place(anchor: Anchor, size: Vec2, window: Rect) -> Rect {
    let height = room(size.y);
    let (origin, sane_origin) = match anchor {
        Anchor::Button(button) => (button, is_sane(button)),
        Anchor::Pointer(point) => (
            Rect::from_min_size(point, Vec2::ZERO),
            point.x.is_finite() && point.y.is_finite(),
        ),
    };
    if !sane_origin || !is_sane(window) || !size.x.is_finite() || !size.y.is_finite() {
        return Rect::from_min_size(pos2(0.0, extent::MENU_GAP), vec2(0.0, height));
    }

    let window = sane(window);
    let width = room(size.x).min(window.width());

    let (left, top) = match anchor {
        Anchor::Button(_) => {
            let button = origin;
            let below = button.bottom() + extent::MENU_GAP;
            let above = button.top() - extent::MENU_GAP - height;
            let top = if below + height <= window.bottom() {
                below
            } else {
                above
            };
            (button.right() - width, top)
        }
        Anchor::Pointer(point) => {
            // Under the cursor, unless that would hang off an edge, in which
            // case the box goes the other way and the cursor lands outside it.
            let left = if point.x + width <= window.right() {
                point.x
            } else {
                point.x - width
            };
            let top = if point.y + height <= window.bottom() {
                point.y
            } else {
                point.y - height
            };
            (left, top)
        }
    };

    let top = top.clamp(window.top(), (window.bottom() - height).max(window.top()));
    let left = left.clamp(window.left(), (window.right() - width).max(window.left()));
    // On egui's own grid, like any widget: a card whose edges sit a third of a
    // point off draws its hairline and its rule fuzzy, and egui's debug build
    // flags every `Ui` that lands there with an orange "Unaligned".
    let rect = Rect::from_min_size(pos2(left, top), vec2(width, height));
    Rect::from_min_max(rect.min.round_ui(), rect.max.round_ui())
}

/// One command row's parts: the label, and the trailing slot it leaves room for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CommandParts {
    pub label: Rect,
    pub key: Option<Rect>,
    pub chevron: Option<Rect>,
}

/// Splits one row into its label and the key hint and chevron after it.
///
/// The trailing slots take their width from the right edge inward, and the label
/// takes what is left: a row too narrow for them yields zero-width rects rather
/// than inverted ones.
pub fn command_parts(row: Rect, key: Option<f32>, chevron: bool) -> CommandParts {
    let inner = inner_rect(sane(row));
    let mut right = inner.right();

    let chevron_rect = chevron.then(|| {
        let side = icon::Size::SM.side.min(inner.width()).min(inner.height());
        let rect = Rect::from_min_size(
            pos2(
                (right - side).max(inner.left()),
                inner.center().y - side / 2.0,
            ),
            vec2(side, side),
        );
        right = rect.left() - space::S4;
        rect
    });

    let key_rect = key.map(|width| {
        let width = room(width).min((right - inner.left()).max(0.0));
        let rect = Rect::from_min_size(
            pos2((right - width).max(inner.left()), inner.top()),
            vec2(width, inner.height()),
        );
        right = rect.left() - space::S4;
        rect
    });

    let label_right = right.max(inner.left()).min(inner.right());
    CommandParts {
        label: Rect::from_min_max(inner.min, pos2(label_right, inner.bottom())),
        key: key_rect,
        chevron: chevron_rect,
    }
}

/// The box a submenu wants: its longest list row, within the sheet's floor and
/// cap. A list name is measured like any label.
pub fn submenu_width(labels: &[RowWidth]) -> f32 {
    width(labels)
}

/// The card a submenu takes: its field's row plus one row per list shown.
///
/// The card is the field and the list, nothing else: a submenu that reserved
/// room for an empty result would be a card with a hole in it.
pub fn submenu_size(rows: usize, width: f32) -> Vec2 {
    vec2(width, extent::SEARCH_H + rows_height(rows))
}

/// Where a submenu lands: beside the parent card at the row's top, flipped to
/// the parent's left when the window's right edge would cut it, then clamped
/// into the window in both axes.
///
/// Flipping is the sheet's own rule for a menu that would hang off an edge: the
/// submenu goes the other way and the pointer lands outside it, which is the
/// same behaviour a right-click menu has.
pub fn submenu_place(parent: Rect, row: Rect, size: Vec2, window: Rect) -> Rect {
    let height = room(size.y);
    if !is_sane(parent) || !is_sane(row) || !is_sane(window) || !size.x.is_finite() {
        return Rect::from_min_size(pos2(0.0, extent::MENU_GAP), vec2(0.0, height));
    }

    let window = sane(window);
    let width = room(size.x).min(window.width());
    let beside = parent.right() + extent::MENU_GAP;
    let left = if beside + width <= window.right() {
        beside
    } else {
        parent.left() - extent::MENU_GAP - width
    };

    let top = row
        .top()
        .clamp(window.top(), (window.bottom() - height).max(window.top()));
    let left = left.clamp(window.left(), (window.right() - width).max(window.left()));
    let rect = Rect::from_min_size(pos2(left, top), vec2(width, height));
    Rect::from_min_max(rect.min.round_ui(), rect.max.round_ui())
}

/// The submenu's field, inset in the card's top strip.
pub fn submenu_field(card: Rect) -> Rect {
    let card = sane(card);
    let left = (card.left() + space::S2).min(card.right());
    let right = (card.right() - space::S2).max(left).min(card.right());
    let top = (card.top() + space::S1).min(card.bottom());
    let bottom = (card.top() + extent::SEARCH_H - space::S1)
        .max(top)
        .min(card.bottom());
    Rect::from_min_max(pos2(left, top), pos2(right, bottom))
}

/// The rect one list row of a submenu takes, below the field's strip.
pub fn submenu_row(card: Rect, index: usize) -> Rect {
    let card = sane(card);
    let top = card.top() + extent::SEARCH_H + index as f32 * extent::ROW_H;
    Rect::from_min_size(pos2(card.left(), top), vec2(card.width(), extent::ROW_H))
}

/// The hairline that separates the field from the list it filters.
pub fn submenu_rule(card: Rect) -> Rect {
    let card = sane(card);
    Rect::from_min_size(
        pos2(card.left(), card.top() + extent::SEARCH_H),
        vec2(card.width(), stroke::HAIRLINE),
    )
}

/// The row's content box: the row less its own `space-3`/`space-1` padding.
fn inner_rect(row: Rect) -> Rect {
    let left = (row.left() + space::S3).min(row.right());
    let right = (row.right() - space::S3).max(left).min(row.right());
    let top = (row.top() + space::S1).min(row.bottom());
    let bottom = (row.bottom() - space::S1).max(top).min(row.bottom());
    Rect::from_min_max(pos2(left, top), pos2(right, bottom))
}

/// What one row needs: its padding, its label and its trailing slots.
fn row_width(row: &RowWidth) -> f32 {
    let mut trailing = 0.0;
    let mut slots = 0.0;
    if let Some(key) = row.key {
        trailing += room(key);
        slots += 1.0;
    }
    if row.chevron {
        trailing += icon::Size::SM.side;
        slots += 1.0;
    }
    2.0 * space::S3 + room(row.label) + trailing + slots * space::S4
}

/// A length that takes room: anything zero, negative or non-finite takes none.
fn room(length: f32) -> f32 {
    if length.is_finite() && length > 0.0 {
        length
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests;
