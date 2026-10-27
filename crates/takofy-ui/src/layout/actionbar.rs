//! The action row's arithmetic: where it pins, and where its children sit.
//!
//! The row holds its resting height in the page's flow whatever it is doing, so
//! the page's scroll extent, the scrollbar and the culling of the rows under it
//! never change. Once its flow slot has scrolled above the viewport's top edge it
//! is drawn at that edge instead, at the height the reference condenses to.
//!
//! The row is solved in two steps because the count beside the field is not the
//! row's to measure: the field is anchored to the tools, which fixes every box
//! that never moves; the caller then hands back the count's width and the name
//! elides to whatever is left. Nothing drawn in the first step moves in the
//! second, so a keystroke that brings the count in does not shift the field.
//!
//! Everything here is pure: the solver answers rects and the view paints them, so
//! the tests that check a place in the row never draw a widget. One measures the
//! artist's word through the control that will draw it, because a sweep at the
//! size the word actually takes is worth more than a guessed number.

use egui::{Pos2, Rect, Vec2};

use crate::theme::{actionbar, extent, hero, space, text};

use super::{Sidebar, Table, regions_for};

/// Which page the row heads, and what a playlist's row leaves room for.
///
/// A playlist's row carries the page's filter field. When its table has
/// condensed and it has a field to draw, the field takes the row's spare width
/// and the heart and name make way for it. Every other page's row is the same at
/// every window width.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Playlist(Form),
    Other,
}

/// What a playlist's row leaves room for at the width it was given.
///
/// The page is handed a [`Table`] by the shell and is not told which half of the
/// window it sits in, so the two halves of the form come from two places: the
/// table says whether the page condensed, the window says whether the rail took
/// the count's room away.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Form {
    /// Whether the count left of the field is drawn.
    pub count: bool,
    /// Whether the page condensed: a field it draws takes the row, and then its
    /// heart and pinned name make way for it.
    pub narrow: bool,
}

impl Form {
    /// The form of a page handed `table` in a window this wide.
    ///
    /// "Narrow" is the page's own table, so a page given the full table is never
    /// condensed by a width its own table did not take.
    pub fn at(width: f32, table: Table) -> Form {
        Form {
            count: regions_for(width).sidebar != Sidebar::Rail,
            narrow: table == Table::Narrow,
        }
    }
}

/// What the page puts in the row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Parts {
    /// Whether the page gives this context a shuffle.
    pub shuffle: bool,
    /// Whether the row carries the page's filter field.
    pub field: bool,
    /// The size of the text control a page puts where the heart would be.
    ///
    /// A text slot means no heart: a page that follows an artist offers
    /// `Follow` rather than a saved state, and the two never share the row.
    pub follow: Option<Vec2>,
}

/// Where the row is drawn this frame, and whether it pinned.
///
/// `flow` is the slot the page allocated for the row; `clip_top` is the top of
/// the viewport it scrolls in.
pub fn pin(flow: Rect, clip_top: f32) -> (Rect, bool) {
    if flow.top() >= clip_top {
        return (flow, false);
    }
    let at = Rect::from_min_size(
        Pos2::new(flow.left(), clip_top),
        Vec2::new(flow.width(), actionbar::STUCK_H),
    );
    (at, true)
}

/// The row's children, once [`pin`] has said where the row is.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Placed {
    /// The play tile: the context's whole queue from its first track.
    pub play: Rect,
    /// The filter field, when the page has one and the row has room for it.
    pub field: Option<Rect>,
    /// The page's text control, in the heart's place.
    pub follow: Option<Rect>,
    /// The shuffle, on the contexts the reference gives one.
    pub shuffle: Option<Rect>,
    /// Saving the context to the library. A condensed playlist gives it up when
    /// it draws its field, so the field can have its room.
    pub heart: Option<Rect>,
    /// The context's own menu.
    pub more: Rect,
}

impl Placed {
    /// The leftmost tool, which is what the field and the name stop short of.
    ///
    /// The row is laid out from the right, so a context that owns fewer tools
    /// keeps every one it has still; the reference's DOM order puts the shuffle
    /// first among them.
    fn first_tool(&self) -> Rect {
        self.follow
            .or(self.shuffle)
            .or(self.heart)
            .unwrap_or(self.more)
    }
}

/// Solves the row's children for a row of `bar`.
pub fn solve(bar: Rect, kind: Kind, parts: Parts) -> Placed {
    let centre = bar.center().y;
    let play = Rect::from_min_size(
        Pos2::new(bar.left() + space::GUTTER, centre - hero::PLAY / 2.0),
        Vec2::splat(hero::PLAY),
    );

    let mut right = bar.right() - space::GUTTER;
    let mut tool = || {
        let rect = Rect::from_min_size(
            Pos2::new(right - extent::BUTTON_LG, centre - extent::BUTTON_LG / 2.0),
            Vec2::splat(extent::BUTTON_LG),
        );
        right = rect.left() - actionbar::TOOLS_GAP;
        rect
    };
    let more = tool();
    // A text slot takes the heart's place: a page that offers a word offers no
    // saved state, and the two would be two answers to the same question. A
    // condensed playlist gives the heart's room to the field, but only when the
    // page actually has a field this frame: a page whose tracks are still
    // loading keeps its tools where they are, so they do not shift when the
    // list arrives.
    let narrow_field = matches!(kind, Kind::Playlist(form) if form.narrow) && parts.field;
    let heart = (parts.follow.is_none() && (kind == Kind::Other || !narrow_field)).then(&mut tool);
    let shuffle = parts.shuffle.then(&mut tool);

    // The text slot is anchored to the tools rather than to the play tile: our
    // name is painted only once the row pins, and a slot sitting beside the play
    // tile would jump sideways at that moment. It is a control of its own, so it
    // stands a row gap off the glyphs rather than the tools' own gap, and a
    // wider label grows leftward without moving anything.
    let first_tool = shuffle.or(heart).unwrap_or(more);
    let follow = parts.follow.map(|size| {
        Rect::from_min_size(
            Pos2::new(
                first_tool.left() - actionbar::GAP - size.x,
                centre - size.y / 2.0,
            ),
            size,
        )
    });

    let mut placed = Placed {
        play,
        field: None,
        follow,
        shuffle,
        heart,
        more,
    };
    if parts.field {
        placed.field = field_in(bar, kind, play, placed.first_tool());
    }
    placed
}

/// The field's box, between the play tile and the first tool.
///
/// It asks for [`actionbar::FIELD_W`] and keeps it whenever the row can spare
/// it, because a field that grew with the window would push the count out on the
/// widths that have one. A condensed playlist gives it everything between the
/// play tile and the tools instead.
fn field_in(bar: Rect, kind: Kind, play: Rect, first_tool: Rect) -> Option<Rect> {
    let left = play.right() + actionbar::GAP;
    let right = first_tool.left() - actionbar::TOOLS_GAP;
    let narrow = matches!(kind, Kind::Playlist(form) if form.narrow);
    let width = if narrow {
        right - left
    } else {
        actionbar::FIELD_W.min(right - left)
    };
    // Below the floor there is no field at all: a stub too narrow to read would
    // still take the keyboard, and a query nobody can see would narrow the list.
    (width >= actionbar::FIELD_MIN_W).then(|| {
        Rect::from_min_size(
            Pos2::new(right - width, bar.center().y - extent::BUTTON_LG / 2.0),
            Vec2::new(width, extent::BUTTON_LG),
        )
    })
}

/// The count's box, one [`actionbar::TOOLS_GAP`] left of the field.
///
/// `None` when the page's form hides it, when the row has no field, or when the
/// box would reach back into the play tile: the count is the first thing the row
/// sheds when it runs out of room.
pub fn count(placed: &Placed, kind: Kind, label_w: f32) -> Option<Rect> {
    let Kind::Playlist(form) = kind else {
        return None;
    };
    if !form.count {
        return None;
    }
    let field = placed.field?;
    let right = field.left() - actionbar::TOOLS_GAP;
    let left = right - label_w;
    (left >= placed.play.right() + actionbar::GAP).then(|| {
        Rect::from_min_max(
            Pos2::new(left, field.top()),
            Pos2::new(right, field.bottom()),
        )
    })
}

/// The box the context's name may use, between the play tile and its neighbour.
///
/// `field` is whether the page carries a filter field this frame. `None` when a
/// condensed playlist has given the room to its field, and `None` when nothing
/// but an ellipsis would fit: the name is a memory aid, and a stub of one beside
/// the controls reads as debris rather than as a label.
pub fn title(
    bar: Rect,
    placed: &Placed,
    count: Option<Rect>,
    kind: Kind,
    field: bool,
) -> Option<Rect> {
    if field && matches!(kind, Kind::Playlist(form) if form.narrow) {
        return None;
    }
    let left = placed.play.right() + actionbar::GAP;
    let right = count
        .or(placed.field)
        .map(|next| next.left())
        .unwrap_or_else(|| placed.first_tool().left())
        - actionbar::GAP;
    (right - left >= text::MD)
        .then(|| Rect::from_min_max(Pos2::new(left, bar.top()), Pos2::new(right, bar.bottom())))
}

#[cfg(test)]
mod tests;
