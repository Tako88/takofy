//! The discovery band's geometry: where a row's bodies and a card's corners go.
//!
//! Pure. The band chooses a body; this module turns that choice and a width into
//! the rects the painter and the hit-test share, so a click and the control under
//! it can never disagree.

use egui::{Rect, Vec2, pos2, vec2};

use crate::layout::cards::{self, Grid};
use crate::theme::space;

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

/// The size of the corner tiles a picture of this width carries.
pub fn tile(art: Rect) -> f32 {
    cards::corner_tile(art)
}

/// The `⋯` tile, outermost on the picture's upper right.
pub fn more(art: Rect) -> Rect {
    cards::corner_more(art)
}

/// The Follow tile, one step in from the `⋯`.
pub fn follow(art: Rect) -> Rect {
    cards::corner(art, tile(art), 1.0)
}

/// The play tile, innermost of the three: it is the one the eye looks for last.
pub fn play(art: Rect) -> Rect {
    cards::corner(art, tile(art), 2.0)
}

#[cfg(test)]
mod tests;
