//! Where a section head's trailing cluster sits.
//!
//! A head is one line: the title at the left, and a cluster — the show page's
//! order chips — flush right. The cluster takes that line only when the title
//! leaves it room; when it does not, it takes a line of its own under the title.
//! Both rects are computed here, so the view measures the title once, draws into
//! the rect it is given, and decides nothing about the geometry.

use egui::{Rect, Vec2, pos2};

/// Where a head's trailing cluster sits, and the rect it takes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    /// Flush right on the head's own line, beside the title.
    Beside(Rect),
    /// Flush right on its own line, a step under the title.
    Under(Rect),
}

/// The cluster's place, decided by the room the title leaves beside it.
///
/// `head` is the title's line, `title_right` the title's own right edge and
/// `gap` the step the head keeps beside the title, and above a cluster that
/// does not fit. A cluster that would meet the title is given a line of its own
/// under `head` instead.
pub fn place(head: Rect, title_right: f32, size: Vec2, gap: f32) -> Placement {
    let right = head.right() - size.x;
    if title_right + gap + size.x <= head.right() {
        return Placement::Beside(Rect::from_min_size(
            pos2(right, head.center().y - size.y / 2.0),
            size,
        ));
    }
    Placement::Under(Rect::from_min_size(pos2(right, head.bottom() + gap), size))
}

#[cfg(test)]
mod tests {
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
}
