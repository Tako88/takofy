//! Where a row head's seed block and trailing cluster sit.
//!
//! A head is one row: the seed and its provenance on the left, and on the right
//! whatever the row has to report — a ledger, the re-roll, the play. The about
//! block takes the room the cluster leaves, so a long seed elides rather than
//! pushing a control off the head at any width.
//!
//! Every size is handed in already measured; this module decides only places.

use egui::{Rect, Vec2, pos2, vec2};

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

/// The trailing cluster's measured sizes, right to left in drawing order.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cluster {
    /// The ledger's measured line, when the row carries one.
    pub ledger: Option<Vec2>,
    /// The re-roll's square.
    pub reroll: Option<Vec2>,
    /// The play's square.
    pub play: Option<Vec2>,
}

/// The placed parts of one head.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowHeadRects {
    pub about: Rect,
    pub ledger: Option<Rect>,
    pub reroll: Option<Rect>,
    pub play: Option<Rect>,
}

/// A head's height about its content: one step above, the taller of the words
/// and the controls, one step below.
pub fn height(lines: f32, control: f32) -> f32 {
    space::S1 + lines.max(control) + space::S2
}

/// Places the about block and the cluster inside `head`.
///
/// The cluster is flush right in the order it is drawn — ledger, re-roll, play —
/// with `gap` between neighbours, so the play keeps the outer edge. The about
/// block is centred on the head and its width is clamped to the room left of the
/// cluster.
pub fn place(head: Rect, about: About, cluster: Cluster, gap: f32) -> RowHeadRects {
    let block_top = head.center().y - about.height / 2.0;

    let mut right = head.right();
    let play = cluster.play.map(|size| {
        let rect = Rect::from_min_size(pos2(right - size.x, head.center().y - size.y / 2.0), size);
        right = rect.left() - gap;
        rect
    });
    let reroll = cluster.reroll.map(|size| {
        let rect = Rect::from_min_size(pos2(right - size.x, head.center().y - size.y / 2.0), size);
        right = rect.left() - gap;
        rect
    });
    let ledger = cluster.ledger.map(|size| {
        let rect = Rect::from_min_size(
            pos2(right - size.x, block_top + about.seed / 2.0 - size.y / 2.0),
            size,
        );
        right = rect.left() - gap;
        rect
    });

    let room = if play.is_none() && reroll.is_none() && ledger.is_none() {
        head.width()
    } else {
        (right - gap - head.left()).max(0.0)
    };
    let width = about.width.min(room).max(0.0);
    let about = Rect::from_min_size(pos2(head.left(), block_top), vec2(width, about.height));

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

#[cfg(test)]
mod tests;
