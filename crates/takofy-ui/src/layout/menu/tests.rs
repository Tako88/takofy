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
    let nan_window = Rect::from_min_size(pos2(0.0, f32::NAN), vec2(WINDOW_MIN[0], WINDOW_MIN[1]));

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
