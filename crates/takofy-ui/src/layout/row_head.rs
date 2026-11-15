//! Where a row head's seed block and its controls sit.
//!
//! A head is one row: the re-roll, then the seed and its provenance, and on the
//! right whatever the row has to report — a ledger and the play. The re-roll
//! stands beside the seed because it replaces exactly that, and the about block
//! takes the room the rest leaves, so a long seed elides rather than pushing a
//! control off the head at any width.
//!
//! Every size is handed in already measured; this module decides only places.

use egui::{Pos2, Rect, Vec2, pos2, vec2};

use crate::theme::space;

/// The about block's measured size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct About {
    /// The width the block would use if the cluster left it room.
    pub width: f32,
    /// The seed line and the provenance under it, with their gap.
    pub height: f32,
    /// The seed line's own height, which the ledger sits on.
    pub seed: f32,
}

/// The head's measured controls.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cluster {
    /// The re-roll's square, which stands before the seed.
    pub reroll: Option<Vec2>,
    /// The ledger's measured line, when the row carries one.
    pub ledger: Option<Vec2>,
    /// The play's square.
    pub play: Option<Vec2>,
}

/// The placed parts of one head.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowHeadRects {
    /// The seed and its provenance, starting after the re-roll.
    pub about: Rect,
    /// The re-roll, at the head's left edge on the seed's line.
    pub reroll: Option<Rect>,
    pub ledger: Option<Rect>,
    pub play: Option<Rect>,
}

/// A head's height about its content: one step above, the taller of the words
/// and the controls, one step below.
pub fn height(lines: f32, control: f32) -> f32 {
    space::S1 + lines.max(control) + space::S2
}

/// Places the controls and the about block inside `head`.
///
/// The right cluster is flush right in the order it is drawn — ledger, play —
/// with `gap` between neighbours, so the play keeps the outer edge. The re-roll
/// takes the head's left edge, and the about block is centred on the head between
/// it and the cluster.
pub fn place(head: Rect, about: About, cluster: Cluster, gap: f32) -> RowHeadRects {
    let block_top = head.center().y - about.height / 2.0;
    // The whole cluster rides the seed's line, count and controls alike: a row
    // head is one line of accounting, and a control centred between the seed and
    // its provenance sits below the words it belongs to.
    let line = block_top + about.seed / 2.0;

    let reroll = cluster
        .reroll
        .map(|size| Rect::from_min_size(pos2(head.left(), line - size.y / 2.0), size));

    let mut right = head.right();
    let play = cluster.play.map(|size| {
        let rect = Rect::from_min_size(pos2(right - size.x, line - size.y / 2.0), size);
        right = rect.left() - gap;
        rect
    });
    let ledger = cluster.ledger.map(|size| {
        let rect = Rect::from_min_size(pos2(right - size.x, line - size.y / 2.0), size);
        right = rect.left() - gap;
        rect
    });

    let left = match reroll {
        Some(rect) => rect.right() + gap,
        None => head.left(),
    };
    let room = if play.is_none() && ledger.is_none() && reroll.is_none() {
        head.width()
    } else {
        (right.max(left) - gap - left).max(0.0)
    };
    let width = about.width.min(room).max(0.0);
    let about = Rect::from_min_size(pos2(left, block_top), vec2(width, about.height));

    RowHeadRects {
        about,
        ledger,
        reroll,
        play,
    }
}

/// A square control at [`crate::theme::extent::BUTTON_LG`]'s size, for a caller
/// that has no measured button yet.
pub fn square(side: f32) -> Vec2 {
    Vec2::splat(side)
}

/// A rect's own edge, from its top-left corner clockwise, cut where `progress`
/// has reached: the line a cooling button draws as it comes back.
///
/// The corners are not rounded here. The radius a control wears is two pixels
/// and this line is laid inside the border it retraces, so it lies on the edge
/// the border already covers.
pub fn edge(rect: Rect, progress: f32) -> Vec<Pos2> {
    let progress = progress.clamp(0.0, 1.0);
    if progress <= 0.0 {
        return Vec::new();
    }
    let mut walked = progress * 2.0 * (rect.width() + rect.height());
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    let mut points = vec![corners[0]];
    for pair in corners.windows(2) {
        if walked <= 0.0 {
            break;
        }
        let (from, to) = (pair[0], pair[1]);
        let side = (to - from).length();
        if walked >= side {
            points.push(to);
            walked -= side;
        } else {
            let reached = if side > 0.0 { walked / side } else { 0.0 };
            points.push(from + (to - from) * reached);
            walked = 0.0;
        }
    }
    points
}

#[cfg(test)]
mod tests;
