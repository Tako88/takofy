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
    fn a_submenu_lands_beside_the_menu_at_its_rows_top() {
        let parent = Rect::from_min_size(pos2(400.0, 300.0), vec2(240.0, 200.0));
        let row = row_rect(parent, 2);
        let size = submenu_size(3, 220.0);
        let sub = submenu_place(parent, row, size, window());

        assert_eq!(sub.left(), parent.right() + extent::MENU_GAP);
        assert_eq!(sub.top(), row.top());
        assert_eq!(sub.size(), size);
        assert_eq!(
            sub.height(),
            extent::SEARCH_H + 3.0 * extent::ROW_H,
            "the card is the field and its rows"
        );
    }

    #[test]
    fn a_submenu_flips_to_the_menus_left_when_the_window_would_cut_it() {
        // A menu right-aligned to a button near the right edge, so there is no
        // room for a card beside it.
        let parent = Rect::from_min_size(pos2(1100.0, 300.0), vec2(320.0, 200.0));
        let row = row_rect(parent, 1);
        let sub = submenu_place(parent, row, submenu_size(3, 220.0), window());

        assert_eq!(
            sub.right(),
            parent.left() - extent::MENU_GAP,
            "the card did not flip to the menu's left"
        );
        assert!(sub.left() >= window().left(), "{sub:?}");
    }

    #[test]
    fn a_submenu_lower_than_the_window_pins_into_it() {
        let parent = Rect::from_min_size(pos2(400.0, 700.0), vec2(240.0, 20.0));
        let row = row_rect(parent, 0);
        let sub = submenu_place(parent, row, submenu_size(8, 220.0), window());
        assert_eq!(sub.bottom(), window().bottom());
        assert_eq!(sub.height(), extent::SEARCH_H + 8.0 * extent::ROW_H);
    }

    #[test]
    fn a_submenu_from_a_bad_parent_draws_the_documented_rect() {
        let bad = Rect::from_min_size(pos2(f32::NAN, 0.0), vec2(240.0, 200.0));
        let sub = submenu_place(bad, bad, submenu_size(2, 220.0), window());
        assert_eq!(sub.min, pos2(0.0, extent::MENU_GAP));
        assert_eq!(sub.width(), 0.0);
    }

    #[test]
    fn a_submenus_field_and_rows_tile_from_its_top_and_never_invert() {
        let card = Rect::from_min_size(pos2(100.0, 200.0), vec2(220.0, 300.0));
        let field = submenu_field(card);
        assert_eq!(field.left(), card.left() + space::S2);
        assert_eq!(field.right(), card.right() - space::S2);
        assert!(field.top() > card.top(), "the field is flush with the card");
        assert!(field.bottom() <= card.top() + extent::SEARCH_H);

        let first = submenu_row(card, 0);
        assert_eq!(first.top(), card.top() + extent::SEARCH_H);
        assert_eq!(first.height(), extent::ROW_H);
        assert_eq!(first.width(), card.width());
        assert_eq!(
            submenu_row(card, 2).top(),
            first.top() + 2.0 * extent::ROW_H
        );

        assert_eq!(submenu_rule(card).top(), card.top() + extent::SEARCH_H);

        // A card narrower than its own padding yields an empty field, not a
        // mirrored one.
        let tiny = Rect::from_min_size(pos2(0.0, 0.0), vec2(4.0, 4.0));
        let field = submenu_field(tiny);
        assert!(field.width() >= 0.0 && field.height() >= 0.0, "{field:?}");
    }

    #[test]
    fn a_menu_taller_than_either_side_pins_to_the_top() {
        let window = Rect::from_min_size(Pos2::ZERO, vec2(WINDOW_MIN[0], WINDOW_MIN[1]));
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
    fn a_pointer_menu_opens_at_the_pointer() {
        let at = pos2(300.0, 200.0);
        let menu = place(Anchor::Pointer(at), size(), window());
        assert_eq!(menu.min, at);
    }

    #[test]
    fn a_pointer_menu_flips_left_and_up_at_the_edges() {
        let window = window();
        let corner = pos2(window.right() - 10.0, window.bottom() - 10.0);
        let menu = place(Anchor::Pointer(corner), size(), window);
        assert!(window.contains_rect(menu), "{menu:?} left the window");
        assert!(menu.right() <= corner.x, "it should have flipped left");
        assert!(menu.bottom() <= corner.y, "it should have flipped up");
    }

    #[test]
    fn every_placement_lands_on_egui_s_own_grid() {
        let window = window();
        for x in [100.0, 100.3, 421.77, 900.51] {
            for y in [80.0, 80.65, 313.3] {
                let button = Rect::from_min_size(pos2(x, y), vec2(28.0, 28.0));
                for anchor in [Anchor::Button(button), Anchor::Pointer(pos2(x, y))] {
                    let menu = place(anchor, size(), window);
                    for edge in [menu.left(), menu.right(), menu.top(), menu.bottom()] {
                        let grid = edge / (1.0 / 32.0);
                        assert!(
                            (grid - grid.round()).abs() < 1e-3,
                            "{edge} for {anchor:?} is off the grid"
                        );
                    }
                    assert!(window.contains_rect(menu), "{menu:?} left the window");
                }
            }
        }
    }

    #[test]
    fn non_finite_inputs_fall_back_to_the_documented_rect() {
        let height = 4.0 * extent::ROW_H;
        let nan_button = Rect::from_min_size(pos2(f32::NAN, 0.0), vec2(28.0, 28.0));
        let nan_window =
            Rect::from_min_size(pos2(0.0, f32::NAN), vec2(WINDOW_MIN[0], WINDOW_MIN[1]));

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
