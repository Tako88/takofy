use super::*;
use crate::theme::{extent, space};

#[test]
fn the_chips_fit_beside_a_wide_head_and_wrap_under_a_narrow_one() {
    let size = Vec2::new(4.0 * extent::BUTTON, extent::BUTTON);
    let title_right = 2.0 * extent::BUTTON;
    let fitting = title_right + space::S4 + size.x;

    let wide = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(fitting, extent::ROW_H));
    let Placement::Beside(placed) = place(wide, title_right, size, space::S4) else {
        panic!("the cluster fits beside the title");
    };
    assert_eq!(placed.right(), wide.right(), "the cluster is flush right");
    assert_eq!(
        placed.size(),
        size,
        "the cluster keeps the size it asked for"
    );
    assert_eq!(placed.center().y, wide.center().y, "centred on the head");

    // The same head one point narrower leaves the title and the cluster
    // exactly nothing between them, so the cluster takes its own line.
    let narrow = Rect::from_min_size(
        egui::Pos2::ZERO,
        Vec2::new(fitting - space::PX, extent::ROW_H),
    );
    let Placement::Under(placed) = place(narrow, title_right, size, space::S4) else {
        panic!("the cluster takes its own line");
    };
    assert_eq!(placed.right(), narrow.right(), "and still flush right");
    assert_eq!(placed.size(), size);
    assert!(
        placed.top() > narrow.bottom(),
        "on a line of its own under the head"
    );
}

#[test]
fn a_list_keeps_the_line_at_its_foot_and_nothing_above_it() {
    let list = Rect::from_min_size(pos2(10.0, 20.0), Vec2::new(300.0, 400.0));

    let (rows, strip) = foot(list, 16.0);
    assert_eq!(strip.top(), list.bottom() - 16.0, "the line holds the foot");
    assert_eq!(strip.bottom(), list.bottom());
    assert_eq!(strip.width(), list.width(), "it is the list's own width");
    assert_eq!(rows.top(), list.top(), "the list keeps its top");
    assert_eq!(
        rows.bottom(),
        strip.top(),
        "and stops where the line starts"
    );

    // A line taller than the list is clipped at the top rather than given a
    // negative height.
    let (rows, strip) = foot(list, 1000.0);
    assert_eq!(rows.top(), list.top());
    assert_eq!(rows.bottom(), list.top(), "the list keeps no height");
    assert_eq!(strip.top(), list.top());
    assert_eq!(strip.bottom(), list.bottom());

    // A line exactly the list's height takes all of it, so the list keeps
    // no rows and the line is still the foot.
    let (rows, strip) = foot(list, list.height());
    assert_eq!(rows.top(), list.top());
    assert_eq!(rows.bottom(), list.top(), "the list keeps no height");
    assert_eq!(strip.top(), list.top(), "the line starts at the top");
    assert_eq!(strip.bottom(), list.bottom());
    assert_eq!(strip.height(), list.height(), "and holds the whole list");
}
