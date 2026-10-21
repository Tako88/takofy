//! A command menu's geometry: where the box lands, and how wide one row is.
//!
//! A menu opens from the button that asked for it — never from the pointer, so
//! there is one position for all three doors. The box right-aligns to that
//! button, sits `extent::MENU_GAP` below it, flips above when the window's
//! bottom would cut it, and is clamped into the window in both axes.
//!
//! The width is the longest row's, within the sheet's 12 rem floor and 20 rem
//! cap. A row is the reference's `space-3`/`space-1` padding, its label, and the
//! trailing slot a key hint or a chevron takes.

use egui::{Rect, Vec2, pos2, vec2};

use super::search_menu::{is_sane, sane};
use crate::menu::Anchor;
use crate::theme::{extent, icon, space};

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

/// The height of a menu of `rows` rows.
pub fn rows_height(rows: usize) -> f32 {
    rows as f32 * extent::ROW_H
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
    let Anchor::Button(button) = anchor;
    let height = room(size.y);
    if !is_sane(button) || !is_sane(window) || !size.x.is_finite() || !size.y.is_finite() {
        return Rect::from_min_size(pos2(0.0, extent::MENU_GAP), vec2(0.0, height));
    }

    let button = sane(button);
    let window = sane(window);
    let width = room(size.x).min(window.width());

    let below = button.bottom() + extent::MENU_GAP;
    let above = button.top() - extent::MENU_GAP - height;
    let top = if below + height <= window.bottom() {
        below
    } else {
        above
    };

    let top = top.clamp(window.top(), (window.bottom() - height).max(window.top()));
    let left =
        (button.right() - width).clamp(window.left(), (window.right() - width).max(window.left()));
    Rect::from_min_size(pos2(left, top), vec2(width, height))
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
mod tests {
    use egui::{Pos2, Rect, pos2, vec2};

    use super::*;
    use crate::layout::WINDOW_MIN;
    use crate::menu::Anchor;
    use crate::theme::{extent, icon, space};

    fn window() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))
    }

    fn button() -> Rect {
        Rect::from_min_size(pos2(600.0, 400.0), vec2(28.0, 28.0))
    }

    fn size() -> Vec2 {
        vec2(200.0, 4.0 * extent::ROW_H)
    }

    fn row_of(width: f32) -> Rect {
        Rect::from_min_size(pos2(10.0, 20.0), vec2(width, extent::ROW_H))
    }

    fn label_only(width: f32) -> RowWidth {
        RowWidth {
            label: width,
            key: None,
            chevron: false,
        }
    }

    #[test]
    fn a_button_menu_right_aligns_one_gap_below_the_button() {
        let menu = place(Anchor::Button(button()), size(), window());
        assert_eq!(
            menu.right(),
            button().right(),
            "the menu is not right-aligned"
        );
        assert_eq!(menu.top(), button().bottom() + extent::MENU_GAP);
        assert_eq!(menu.size(), size());
    }

    #[test]
    fn a_button_at_the_right_edge_stays_inside_the_window() {
        let button = Rect::from_min_size(pos2(1400.0, 400.0), vec2(28.0, 28.0));
        let menu = place(Anchor::Button(button), size(), window());
        assert!(menu.right() <= window().right() + 0.01, "{menu:?}");
        assert!(menu.left() >= window().left() - 0.01, "{menu:?}");
    }

    #[test]
    fn a_button_near_the_bottom_flips_above_it() {
        let button = Rect::from_min_size(pos2(600.0, 860.0), vec2(28.0, 28.0));
        let menu = place(Anchor::Button(button), size(), window());
        assert!(
            menu.bottom() <= button.top() + 0.01,
            "the menu at {menu:?} did not flip above {button:?}"
        );
        assert_eq!(menu.bottom(), button.top() - extent::MENU_GAP);
        assert!(menu.top() >= window().top() - 0.01, "{menu:?}");
    }

    #[test]
    fn a_menu_taller_than_either_side_pins_to_the_top() {
        let window = Rect::from_min_size(Pos2::ZERO, vec2(640.0, 480.0));
        let button = Rect::from_min_size(pos2(300.0, 200.0), vec2(28.0, 28.0));
        let menu = place(Anchor::Button(button), vec2(200.0, 600.0), window);
        assert_eq!(menu.top(), window.top(), "the menu did not pin to the top");
        assert_eq!(menu.height(), 600.0);
    }

    #[test]
    fn every_placement_inside_a_grid_of_anchors_stays_in_the_window() {
        let window = Rect::from_min_size(Pos2::ZERO, vec2(WINDOW_MIN[0], WINDOW_MIN[1]));
        let sizes = [
            vec2(extent::MENU_MIN_W, extent::ROW_H),
            vec2(extent::MENU_MAX_W, 4.0 * extent::ROW_H),
        ];
        for size in sizes {
            for x in (0..=WINDOW_MIN[0] as i32).step_by(40) {
                for y in (0..=WINDOW_MIN[1] as i32).step_by(40) {
                    let button = Rect::from_min_size(pos2(x as f32, y as f32), vec2(28.0, 28.0));
                    let menu = place(Anchor::Button(button), size, window);
                    assert!(
                        menu.left() >= window.left() - 0.01
                            && menu.right() <= window.right() + 0.01
                            && menu.top() >= window.top() - 0.01
                            && menu.bottom() <= window.bottom() + 0.01,
                        "the menu at {button:?} left the window: {menu:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn non_finite_inputs_fall_back_to_the_documented_rect() {
        let height = 4.0 * extent::ROW_H;
        let nan_button = Rect::from_min_size(pos2(f32::NAN, 0.0), vec2(28.0, 28.0));
        let nan_window = Rect::from_min_size(pos2(0.0, f32::NAN), vec2(640.0, 480.0));

        for (anchor, size, window, expected_height) in [
            (Anchor::Button(nan_button), size(), window(), height),
            (Anchor::Button(button()), size(), nan_window, height),
            (
                Anchor::Button(button()),
                vec2(f32::NAN, height),
                window(),
                height,
            ),
            (
                Anchor::Button(button()),
                vec2(200.0, f32::NAN),
                window(),
                0.0,
            ),
        ] {
            let menu = place(anchor, size, window);
            assert_eq!(menu.min, pos2(0.0, extent::MENU_GAP));
            assert_eq!(menu.width(), 0.0);
            assert_eq!(menu.height(), expected_height);
        }
    }

    #[test]
    fn width_follows_the_longest_row_inside_the_floor_and_the_cap() {
        assert_eq!(
            width(&[label_only(10.0), label_only(20.0)]),
            extent::MENU_MIN_W,
            "a short menu is the sheet's floor"
        );

        let middle = 2.0 * space::S3 + 200.0;
        assert_eq!(width(&[label_only(20.0), label_only(200.0)]), middle);

        assert_eq!(
            width(&[label_only(extent::MENU_MAX_W * 2.0)]),
            extent::MENU_MAX_W,
            "a long menu is the sheet's cap"
        );
    }

    #[test]
    fn a_key_hint_and_a_chevron_widen_the_row() {
        // Above the floor, so the widening is the trailing slot's own and not
        // the floor's.
        let bare = width(&[label_only(200.0)]);
        assert_eq!(bare, 2.0 * space::S3 + 200.0);
        let keyed = width(&[RowWidth {
            label: 200.0,
            key: Some(24.0),
            chevron: false,
        }]);
        assert_eq!(keyed, bare + 24.0 + space::S4);

        let chevroned = width(&[RowWidth {
            label: 200.0,
            key: None,
            chevron: true,
        }]);
        assert_eq!(chevroned, bare + icon::Size::SM.side + space::S4);

        let both = width(&[RowWidth {
            label: 200.0,
            key: Some(24.0),
            chevron: true,
        }]);
        assert_eq!(
            both,
            bare + 24.0 + icon::Size::SM.side + 2.0 * space::S4,
            "each trailing slot takes its own gap"
        );
    }

    #[test]
    fn the_label_takes_what_the_key_and_chevron_leave() {
        let row = row_of(extent::MENU_MAX_W);
        let inner = extent::MENU_MAX_W - 2.0 * space::S3;
        let parts = command_parts(row, Some(30.0), true);
        assert_eq!(
            parts.label.width(),
            inner - 30.0 - icon::Size::SM.side - 2.0 * space::S4
        );
        assert_eq!(parts.label.left(), row.left() + space::S3);
        assert_eq!(
            parts.key.expect("a key").right(),
            parts.chevron.expect("a chevron").left() - space::S4
        );
        assert_eq!(
            parts.chevron.expect("a chevron").right(),
            row.right() - space::S3
        );

        let plain = command_parts(row, None, false);
        assert_eq!(plain.label.width(), inner);
        assert_eq!(plain.key, None);
        assert_eq!(plain.chevron, None);
    }

    #[test]
    fn command_parts_stay_inside_the_row_and_never_invert() {
        for width in [
            0.0,
            1.0,
            space::S3,
            60.0,
            extent::MENU_MIN_W,
            extent::MENU_MAX_W,
        ] {
            for key in [None, Some(0.0), Some(30.0), Some(400.0)] {
                for chevron in [false, true] {
                    let row = row_of(width);
                    let parts = command_parts(row, key, chevron);
                    let rects = [Some(parts.label), parts.key, parts.chevron];
                    for rect in rects.into_iter().flatten() {
                        assert!(
                            rect.width() >= 0.0 && rect.height() >= 0.0,
                            "{width}/{key:?}/{chevron}: {rect:?}"
                        );
                        assert!(
                            rect.left() >= row.left() - 0.01 && rect.right() <= row.right() + 0.01,
                            "{width}/{key:?}/{chevron}: {rect:?} left {row:?}"
                        );
                    }
                    assert!(
                        parts.label.right() <= row.right() + 0.01,
                        "{width}/{key:?}/{chevron}: {:?}",
                        parts.label
                    );
                }
            }
        }
    }

    #[test]
    fn the_minimum_window_holds_the_tallest_track_menu() {
        assert!(
            rows_height(4) <= WINDOW_MIN[1],
            "four rows need {} but the minimum window is {}",
            rows_height(4),
            WINDOW_MIN[1]
        );
        assert_eq!(rows_height(4), 4.0 * extent::ROW_H);
    }
}
