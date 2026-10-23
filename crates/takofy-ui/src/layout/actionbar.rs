//! The action row's arithmetic: where it pins, and where its children sit.
//!
//! The row holds its resting height in the page's flow whatever it is doing, so
//! the page's scroll extent, the scrollbar and the culling of the rows under it
//! never change. Once its flow slot has scrolled above the viewport's top edge it
//! is drawn at that edge instead, at the height the reference condenses to.
//!
//! Everything here is pure: the solver answers rects and the view paints them, so
//! the tests that check a place in the row never draw a frame.

use egui::{Pos2, Rect, Vec2};

use crate::theme::{actionbar, extent, hero, space, text};

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
    /// The shuffle, on the contexts the reference gives one.
    pub shuffle: Option<Rect>,
    /// Saving the context to the library.
    pub heart: Rect,
    /// The context's own menu.
    pub more: Rect,
}

impl Placed {
    /// The first tool at the row's right-hand end.
    ///
    /// The row is laid out from the right, so a context that owns fewer tools
    /// keeps every one it has still; the reference's DOM order puts the shuffle
    /// first among them.
    fn first_tool(&self) -> Rect {
        self.shuffle.unwrap_or(self.heart)
    }
}

/// Solves the row's children for a row of `bar`.
pub fn solve(bar: Rect, shuffle: bool) -> Placed {
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
    let heart = tool();
    let shuffle = shuffle.then(&mut tool);

    Placed {
        play,
        shuffle,
        heart,
        more,
    }
}

/// The box the context's name may use, between the play tile and the first tool.
///
/// `None` when nothing but an ellipsis would fit: the name is a memory aid, and a
/// stub of one beside the controls reads as debris rather than as a label.
pub fn title(bar: Rect, placed: &Placed) -> Option<Rect> {
    let left = placed.play.right() + actionbar::GAP;
    let right = placed.first_tool().left() - actionbar::GAP;
    (right - left >= text::MD)
        .then(|| Rect::from_min_max(Pos2::new(left, bar.top()), Pos2::new(right, bar.bottom())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(top: f32, width: f32, height: f32) -> Rect {
        Rect::from_min_size(Pos2::new(0.0, top), Vec2::new(width, height))
    }

    #[test]
    fn a_bar_at_or_below_the_clip_rests() {
        let flow = slot(100.0, 400.0, actionbar::REST_H);
        let (rect, stuck) = pin(flow, 12.0);
        assert!(!stuck, "a row below the clip top stays in its place");
        assert_eq!(rect, flow);

        // A row whose top is exactly the clip's top has not moved either: the
        // pin is for a row that has scrolled past it.
        let (rect, stuck) = pin(slot(12.0, 400.0, actionbar::REST_H), 12.0);
        assert!(!stuck, "a row at the clip top is not pinned");
        assert_eq!(rect.top(), 12.0);
    }

    #[test]
    fn a_bar_above_the_clip_pins_at_the_stuck_height() {
        let flow = slot(-30.0, 400.0, actionbar::REST_H);
        let (rect, stuck) = pin(flow, 12.0);
        assert!(stuck);
        assert_eq!(rect.top(), 12.0, "the row sits at the clip's top edge");
        assert_eq!(rect.height(), actionbar::STUCK_H);
        assert_eq!(rect.width(), flow.width(), "the row keeps the page's width");
    }

    #[test]
    fn play_is_a_gutter_in_and_centred_at_both_heights() {
        for height in [actionbar::REST_H, actionbar::STUCK_H] {
            let bar = slot(12.0, 900.0, height);
            let placed = solve(bar, true);
            assert_eq!(placed.play.left(), bar.left() + space::GUTTER);
            assert_eq!(placed.play.width(), hero::PLAY);
            assert_eq!(placed.play.height(), hero::PLAY);
            assert_eq!(placed.play.center().y, bar.center().y);
        }
    }

    #[test]
    fn tools_hug_the_right_gutter_in_reference_order() {
        let bar = slot(12.0, 900.0, actionbar::STUCK_H);
        let placed = solve(bar, true);
        assert_eq!(placed.more.right(), bar.right() - space::GUTTER);
        let shuffle = placed.shuffle.expect("a playlist's row has a shuffle");
        assert_eq!(shuffle.right(), placed.heart.left() - actionbar::TOOLS_GAP);
        assert_eq!(
            placed.heart.right(),
            placed.more.left() - actionbar::TOOLS_GAP
        );
        for tool in [shuffle, placed.heart, placed.more] {
            assert_eq!(tool.width(), extent::BUTTON_LG);
            assert_eq!(tool.height(), extent::BUTTON_LG);
            assert_eq!(tool.center().y, bar.center().y);
        }
    }

    #[test]
    fn no_shuffle_closes_the_gap_without_moving_the_rest() {
        let bar = slot(12.0, 900.0, actionbar::REST_H);
        let with = solve(bar, true);
        let without = solve(bar, false);
        assert!(without.shuffle.is_none(), "an album has no shuffle");
        assert_eq!(without.play, with.play);
        assert_eq!(without.heart, with.heart);
        assert_eq!(without.more, with.more);
    }

    #[test]
    fn the_title_runs_from_play_plus_gap_to_the_first_tool_minus_gap() {
        let bar = slot(12.0, 900.0, actionbar::STUCK_H);
        let placed = solve(bar, true);
        let label = title(bar, &placed).expect("a wide row has room for a name");
        assert_eq!(label.top(), bar.top());
        assert_eq!(label.bottom(), bar.bottom());
        assert_eq!(label.left(), placed.play.right() + actionbar::GAP);
        assert_eq!(
            label.right(),
            placed.shuffle.expect("a playlist has a shuffle").left() - actionbar::GAP
        );

        // The album's row has no shuffle, so its name may run up to the heart.
        let placed = solve(bar, false);
        assert_eq!(
            title(bar, &placed).expect("room").right(),
            placed.heart.left() - actionbar::GAP
        );
    }

    #[test]
    fn a_title_without_room_is_none() {
        // The box's left is the gutter, the play tile and the gap; its right is
        // the three tools, their gaps and the gutter, and the gap before them.
        let left = space::GUTTER + hero::PLAY + actionbar::GAP;
        let from_the_end =
            space::GUTTER + 3.0 * extent::BUTTON_LG + 2.0 * actionbar::TOOLS_GAP + actionbar::GAP;
        let fits = left + text::MD + from_the_end;

        let bar = slot(12.0, fits, actionbar::STUCK_H);
        assert!(
            title(bar, &solve(bar, true)).is_some(),
            "a box exactly one text tier wide still holds the name"
        );
        let bar = slot(12.0, fits - 1.0, actionbar::STUCK_H);
        assert!(
            title(bar, &solve(bar, true)).is_none(),
            "a box narrower than a name holds nothing"
        );
    }
}
