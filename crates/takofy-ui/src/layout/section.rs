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
mod tests;
