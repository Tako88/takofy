//! The discovery band's geometry: where a row's bodies and a card's corners go.
//!
//! Pure. The band chooses a body; this module turns that choice and a width into
//! the rects the painter and the hit-test share, so a click and the control under
//! it can never disagree.

use egui::{Rect, Vec2, pos2, vec2};

use crate::layout::cards::{self, Grid};
use crate::theme::{extent, space};

/// A sole face: card size, ending where it ends rather than stretching.
pub fn lone(available: Rect, face_w: f32, extra: f32) -> Rect {
    let width = face_w.min(available.width()).max(0.0);
    Rect::from_min_size(available.min, vec2(width, width + extra))
}

/// Two faces turned on their side: each card's picture takes a share of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pair {
    pub cards: [Rect; 2],
    /// Where each card's picture sits inside it.
    pub arts: [Rect; 2],
}

pub fn pair(available: Rect, gap: f32, share: f32, extra: f32) -> Pair {
    let width = ((available.width() - gap) / 2.0).max(0.0);
    let art_w = ((width - 2.0 * space::S2) * share).max(0.0);
    let height = art_w + extra;
    let cards = [
        Rect::from_min_size(available.min, vec2(width, height)),
        Rect::from_min_size(
            pos2(available.left() + width + gap, available.top()),
            vec2(width, height),
        ),
    ];
    let arts = cards.map(|card| pair_art(card, share));
    Pair { cards, arts }
}

/// One face and the tracks beside it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Split {
    pub card: Rect,
    /// The column the mini-rows take; its height is the card's.
    pub tracks: Rect,
}

pub fn split(available: Rect, face_w: f32, gap: f32, extra: f32) -> Split {
    let width = face_w.min(available.width()).max(0.0);
    let height = width + extra;
    let card = Rect::from_min_size(available.min, vec2(width, height));
    let left = card.right() + gap;
    let tracks = Rect::from_min_size(
        pos2(left, available.top()),
        vec2((available.right() - left).max(0.0), height),
    );
    Split { card, tracks }
}

/// The faces a row lays on the shared track width.
pub fn faces(available: Rect, columns: usize, gap: f32, extra: f32) -> Grid {
    cards::grid_fixed(available, columns, gap, extra)
}

/// A pair card's picture: the share of its inner width, square.
pub fn pair_art(card: Rect, share: f32) -> Rect {
    let inner = card.shrink(space::S2);
    Rect::from_min_size(inner.min, Vec2::splat((inner.width() * share).max(0.0)))
}

/// A card's picture: its inner box, square.
pub fn card_art(card: Rect) -> Rect {
    let inner = card.shrink(space::S2);
    Rect::from_min_size(inner.min, Vec2::splat(inner.width()))
}

/// One corner control, `space-2` in from the card's own corner.
///
/// The controls belong to the card's raised rectangle — the box that changes
/// colour under the pointer — not to the picture's box inside it, and not to the
/// circle the picture is drawn as: the picture's box has no visible corner to sit
/// against, so a control placed there reads as floating.
fn corner(card: Rect, size: f32, across: f32, down: f32) -> Rect {
    let x = if across > 0.0 {
        card.right() - space::S2 - size
    } else {
        card.left() + space::S2
    };
    let y = if down > 0.0 {
        card.bottom() - space::S2 - size
    } else {
        card.top() + space::S2
    };
    Rect::from_min_size(pos2(x, y), Vec2::splat(size))
}

/// The `⋯` tile on the card's upper right.
pub fn more(card: Rect) -> Rect {
    corner(card, extent::CARD_PLAY, 1.0, -1.0)
}

/// The Follow tile beside the `⋯`, one step to its left.
pub fn follow(card: Rect) -> Rect {
    more(card).translate(vec2(-(extent::CARD_PLAY + space::S1), 0.0))
}

/// The play tile on the card's lower right.
pub fn play(card: Rect) -> Rect {
    corner(card, extent::CARD_PLAY, 1.0, 1.0)
}

#[cfg(test)]
mod tests;
