//! Where a section head's trailing cluster sits, and the line a list keeps at
//! its foot.
//!
//! A head is one line: the title at the left, and a cluster — the show page's
//! order chips — flush right. The cluster takes that line only when the title
//! leaves it room; when it does not, it takes a line of its own under the title.
//! Both rects are computed here, so the view measures the title once, draws into
//! the rect it is given, and decides nothing about the geometry.
//!
//! A list's foot is the other one-line reservation: a count or a failure line
//! below a list belongs to the viewport rather than the flow, so a long list
//! scrolls under it instead of carrying it out of sight.

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

/// The rect a list keeps for a line at its foot, and the list's own rect above
/// it.
///
/// `height` is the line's row height. A rect too short to hold it keeps its top
/// edge, so the list never gains a negative height and the line is clipped
/// rather than drawn above the list's first row.
pub fn foot(rect: Rect, height: f32) -> (Rect, Rect) {
    let top = (rect.bottom() - height).max(rect.top());
    (
        Rect::from_min_max(rect.min, pos2(rect.right(), top)),
        Rect::from_min_max(pos2(rect.left(), top), rect.max),
    )
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
}
