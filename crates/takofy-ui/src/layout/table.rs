//! Track-row column arithmetic.
//!
//! The reference composes a playlist row from seven top-level tracks — index,
//! title, album, added, like, duration and actions — with the cover art nested
//! inside the title. `columns` solves those tracks from the fixed extents and
//! the two growth weights; `title_parts` then splits the title track into its
//! art box and its text.
//!
//! Every returned rect is clamped inside the row's inner rect. The solver hands
//! back every track's minimum when the fixed columns and gaps do not fit, so
//! without the clamp a narrow window would place a column past the row's right
//! edge. The clamp is the contract: nothing here overflows.

use egui::{Pos2, Rect, pos2, vec2};

use crate::layout::tracks::{Track, solve};
use crate::theme::{extent, grow, space};

/// Which optional columns a row carries at the current width.
///
/// The index and the title are always present; this is the painter's per-width
/// choice for the rest. The table never decides which columns drop, it lays out
/// the ones it is told exist.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Present {
    pub album: bool,
    pub added: bool,
    pub like: bool,
    pub duration: bool,
    pub actions: bool,
}

/// The seven top-level rects of a track row, in the reference's order.
///
/// An absent column keeps a zero-width rect at the cursor rather than vanishing,
/// so every field is always readable.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Columns {
    pub index: Rect,
    pub title: Rect,
    pub album: Rect,
    pub added: Rect,
    pub like: Rect,
    pub duration: Rect,
    pub actions: Rect,
}

/// The title track split into its art box and its text.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TitleParts {
    pub art: Rect,
    pub text: Rect,
}

impl Columns {
    /// The same columns moved so the row's own origin is at `at`.
    ///
    /// The rects are solved row-local — the row's top-left is the origin and the
    /// content is inset by the row's own padding — so a painter that has the row's
    /// place on screen moves them once here rather than adding it to each rect.
    pub fn at(self, at: Pos2) -> Self {
        let shift = at.to_vec2();
        Self {
            index: self.index.translate(shift),
            title: self.title.translate(shift),
            album: self.album.translate(shift),
            added: self.added.translate(shift),
            like: self.like.translate(shift),
            duration: self.duration.translate(shift),
            actions: self.actions.translate(shift),
        }
    }
}

// The seven top-level tracks, in the reference's order.
const INDEX: usize = 0;
const TITLE: usize = 1;
const ALBUM: usize = 2;
const ADDED: usize = 3;
const LIKE: usize = 4;
const DURATION: usize = 5;
const ACTIONS: usize = 6;
const TRACKS: usize = 7;

/// The track a column is solved from.
fn track_for(index: usize) -> Track {
    match index {
        INDEX => Track::fixed(extent::TRACK_INDEX_W),
        TITLE => Track {
            basis: 0.0,
            min: 0.0,
            max: f32::INFINITY,
            grow: grow::TITLE,
        },
        ALBUM => Track {
            basis: 0.0,
            min: 0.0,
            max: f32::INFINITY,
            grow: grow::ALBUM,
        },
        ADDED => Track::fixed(extent::TRACK_ADDED_W),
        LIKE => Track::fixed(extent::TRACK_LIKE_W),
        DURATION => Track::fixed(extent::TRACK_DURATION_W),
        ACTIONS => Track::fixed(extent::TRACK_ACTIONS_W),
        _ => unreachable!("there are seven top-level tracks"),
    }
}

/// The content box of a row `width` points wide.
///
/// The row is `extent::ROW_H` tall with the reference's `padding: S1 S3`, so the
/// inner rect is the row less that padding. A width below the horizontal padding
/// collapses the inner box to zero width rather than inverting it.
fn inner_rect(width: f32) -> Rect {
    let left = space::S3;
    let right = (width - space::S3).max(left);
    let top = space::S1;
    let bottom = (extent::ROW_H - space::S1).max(top);
    Rect::from_min_max(pos2(left, top), pos2(right, bottom))
}

/// Solves a row's seven columns inside a row `width` points wide.
///
/// The row's outer box is `width` by `extent::ROW_H`, and its content is inset
/// by the reference's padding. A column that is not `present` takes no width and
/// no gap, so the present columns tile the whole inner width when it is large
/// enough and are clamped to it when it is not.
pub fn columns(width: f32, present: Present) -> Columns {
    let width = if width.is_finite() && width > 0.0 {
        width
    } else {
        0.0
    };
    let inner = inner_rect(width);

    let present_flags = [
        true,
        true,
        present.album,
        present.added,
        present.like,
        present.duration,
        present.actions,
    ];
    let tracks: Vec<Track> = present_flags
        .iter()
        .enumerate()
        .filter(|(_, present)| **present)
        .map(|(index, _)| track_for(index))
        .collect();
    let widths = solve(inner.width(), space::S4, &tracks);

    let mut rects = [inner; TRACKS];
    let mut cursor = inner.left();
    let mut solved = 0;
    for (index, present) in present_flags.iter().enumerate() {
        let left = cursor.min(inner.right());
        if *present {
            let width = widths[solved].max(0.0);
            solved += 1;
            let right = (left + width).min(inner.right());
            rects[index] = Rect::from_min_max(pos2(left, inner.top()), pos2(right, inner.bottom()));
            cursor = right + space::S4;
        } else {
            rects[index] = Rect::from_min_max(pos2(left, inner.top()), pos2(left, inner.bottom()));
        }
    }

    Columns {
        index: rects[INDEX],
        title: rects[TITLE],
        album: rects[ALBUM],
        added: rects[ADDED],
        like: rects[LIKE],
        duration: rects[DURATION],
        actions: rects[ACTIONS],
    }
}

/// Splits the title track into its art box and its text.
///
/// The reference nests a `2.25rem` art box and a `space-3` gutter inside the
/// title track, so the split is `extent::LIBRARY_ART` then `space::S3`. The art
/// is held to the title's own width and the text is clamped to a non-negative
/// width, so neither can spill past the title.
pub fn title_parts(title: Rect) -> TitleParts {
    let art_width = extent::LIBRARY_ART.min(title.width().max(0.0));
    let art = Rect::from_min_size(title.min, vec2(art_width, title.height()));
    let text_left = (art.right() + space::S3).min(title.right());
    let text = Rect::from_min_max(
        pos2(text_left, title.top()),
        pos2(title.right().max(text_left), title.bottom()),
    );
    TitleParts { art, text }
}

/// The `index`-th row inside `list`, the rows tiling with no gap.
///
/// The hit-test and the painter both call this, so a click and the row it paints
/// can never disagree. A zero or non-finite pitch yields a zero-height row
/// rather than a panicking one.
pub fn row_rect(list: Rect, index: usize, row_h: f32) -> Rect {
    let row_h = if row_h.is_finite() && row_h > 0.0 {
        row_h
    } else {
        0.0
    };
    let top = list.top() + index as f32 * row_h;
    Rect::from_min_size(pos2(list.left(), top), vec2(list.width(), row_h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::WINDOW_MIN;

    fn all() -> Present {
        Present {
            album: true,
            added: true,
            like: true,
            duration: true,
            actions: true,
        }
    }

    fn rects(columns: Columns) -> [(&'static str, Rect); 7] {
        [
            ("index", columns.index),
            ("title", columns.title),
            ("album", columns.album),
            ("added", columns.added),
            ("like", columns.like),
            ("duration", columns.duration),
            ("actions", columns.actions),
        ]
    }

    fn assert_close(left: f32, right: f32) {
        assert!((left - right).abs() < 0.01, "{left} is not {right}");
    }

    #[test]
    fn the_columns_sum_to_the_available_width_when_it_is_large_enough() {
        let width = 1000.0;
        let columns = columns(width, all());
        let sum = columns.index.width()
            + columns.title.width()
            + columns.album.width()
            + columns.added.width()
            + columns.like.width()
            + columns.duration.width()
            + columns.actions.width()
            + 6.0 * space::S4;
        assert_close(sum, width - 2.0 * space::S3);
    }

    #[test]
    fn an_absent_column_takes_no_width_and_no_gap() {
        let present = Present {
            album: false,
            ..all()
        };
        let width = 1000.0;
        let hidden = columns(width, present);
        assert_eq!(hidden.album.width(), 0.0);

        let sum = hidden.index.width()
            + hidden.title.width()
            + hidden.added.width()
            + hidden.like.width()
            + hidden.duration.width()
            + hidden.actions.width()
            + 5.0 * space::S4;
        assert_close(sum, width - 2.0 * space::S3);

        // The title takes the album's share of the growth.
        assert!(hidden.title.width() > columns(width, all()).title.width());
    }

    #[test]
    fn every_column_stays_inside_the_inner_rect_at_the_minimum_window() {
        // The narrowest content area the shell can hand the tracklist at
        // WINDOW_MIN, with every column present, then widths below it.
        for width in [
            WINDOW_MIN[0] - extent::SIDEBAR_W,
            WINDOW_MIN[0] - extent::SIDEBAR_RAIL_W,
            200.0,
            100.0,
            24.0,
            0.0,
        ] {
            let columns = columns(width, all());
            let inner = inner_rect(width);
            for (name, rect) in rects(columns) {
                assert!(
                    rect.left() >= inner.left() - 0.01,
                    "{name} at {width}: {rect:?} spills left of {inner:?}"
                );
                assert!(
                    rect.right() <= inner.right() + 0.01,
                    "{name} at {width}: {rect:?} spills right of {inner:?}"
                );
                assert!(
                    rect.top() >= inner.top() - 0.01,
                    "{name} at {width}: {rect:?} spills above {inner:?}"
                );
                assert!(
                    rect.bottom() <= inner.bottom() + 0.01,
                    "{name} at {width}: {rect:?} spills below {inner:?}"
                );
                assert!(rect.width() >= 0.0, "{name} at {width} is negative");
                assert!(rect.height() >= 0.0, "{name} at {width} is negative");
            }
        }
    }

    #[test]
    fn a_zero_or_non_finite_width_yields_zero_width_rects() {
        for width in [0.0, -5.0, f32::NAN, f32::INFINITY] {
            let columns = columns(width, all());
            for (name, rect) in rects(columns) {
                assert_eq!(rect.width(), 0.0, "{name} at {width}: {rect:?}");
                assert!(rect.height() >= 0.0, "{name} at {width}: {rect:?}");
            }
        }
    }

    #[test]
    fn title_parts_splits_at_the_art_and_the_gutter() {
        let title = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, extent::LIBRARY_ART));
        let parts = title_parts(title);
        assert_eq!(parts.art.width(), extent::LIBRARY_ART);
        assert_eq!(parts.art.height(), title.height());
        assert_eq!(parts.text.left(), extent::LIBRARY_ART + space::S3);
        assert_eq!(parts.text.right(), title.right());
    }

    #[test]
    fn title_parts_never_goes_negative() {
        for width in [
            0.0,
            1.0,
            extent::LIBRARY_ART,
            extent::LIBRARY_ART + space::S3,
            extent::LIBRARY_ART + space::S3 + 100.0,
        ] {
            let title = Rect::from_min_size(pos2(10.0, 0.0), vec2(width, extent::LIBRARY_ART));
            let parts = title_parts(title);
            assert!(parts.art.width() >= 0.0, "{width}");
            assert!(parts.text.width() >= 0.0, "{width}");
            assert!(parts.art.right() <= title.right() + 0.01, "{width}");
            assert!(parts.text.right() <= title.right() + 0.01, "{width}");
        }
    }

    #[test]
    fn rows_tile_with_no_gap() {
        let list = Rect::from_min_size(pos2(4.0, 8.0), vec2(300.0, 200.0));
        let row_h = extent::ROW_H;
        let first = row_rect(list, 0, row_h);
        let second = row_rect(list, 1, row_h);
        let third = row_rect(list, 2, row_h);

        assert_eq!(first.min, list.min);
        assert_eq!(first.width(), list.width());
        assert_eq!(first.height(), row_h);
        assert_eq!(first.bottom(), second.top());
        assert_eq!(second.bottom(), third.top());

        assert_eq!(row_rect(list, 0, 0.0).height(), 0.0);
        assert_eq!(row_rect(list, 0, f32::NAN).height(), 0.0);
        assert_eq!(row_rect(list, 0, -10.0).height(), 0.0);
    }
}
