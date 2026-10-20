//! Track-row column arithmetic.
//!
//! The reference composes a playlist row from eight top-level tracks — index,
//! title, artist, album, added, like, duration and actions — with the cover art
//! nested inside the title. Only a playlist carries the artist column; a library
//! or an album block leaves it out, and an album block leaves the album out too.
//! `columns` solves those tracks from the fixed extents and the growth weights;
//! `title_parts` then splits the title track into its art box and its text.
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
    pub artist: bool,
    pub album: bool,
    pub added: bool,
    pub like: bool,
    pub duration: bool,
    pub actions: bool,
}

impl Present {
    /// Drops the date column unless the rows are a playlist's membership.
    ///
    /// The width solver keeps its one job; which context the row belongs to is
    /// decided here, from the loaded context's own URI.
    pub fn for_playlist(self, is_playlist: bool) -> Self {
        Self {
            added: self.added && is_playlist,
            ..self
        }
    }

    /// Drops the columns an album's own tracklist does not carry.
    ///
    /// The band already names the record, so neither the artist nor the album
    /// takes a column, and an album position has no membership date. What is
    /// left is the reference's album table: the number, the title over its
    /// artist, the length, the like and the more.
    pub fn for_album(self) -> Self {
        Self {
            artist: false,
            album: false,
            added: false,
            ..self
        }
    }
}

/// Whether a context URI names a playlist: `spotify:playlist:<id>`, or the named
/// `spotify:user:<user>:playlist:<id>`. The id must be non-empty.
///
/// The first segment is matched exactly and the id is only checked for
/// emptiness: the loaded URI was already validated where it came from, so this
/// is a question of identity, not of well-formedness.
pub fn is_playlist(uri: &str) -> bool {
    let Some(rest) = uri.strip_prefix("spotify:") else {
        return false;
    };
    let mut parts = rest.split(':');
    match parts.next() {
        Some("playlist") => parts.next().is_some_and(|id| !id.is_empty()),
        Some("user") => {
            parts.next();
            parts.next() == Some("playlist") && parts.next().is_some_and(|id| !id.is_empty())
        }
        _ => false,
    }
}

/// The eight top-level rects of a track row, in the reference's order.
///
/// An absent column keeps a zero-width rect at the cursor rather than vanishing,
/// so every field is always readable.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Columns {
    pub index: Rect,
    pub title: Rect,
    pub artist: Rect,
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
            artist: self.artist.translate(shift),
            album: self.album.translate(shift),
            added: self.added.translate(shift),
            like: self.like.translate(shift),
            duration: self.duration.translate(shift),
            actions: self.actions.translate(shift),
        }
    }
}

// The eight top-level tracks, in the reference's order.
const INDEX: usize = 0;
const TITLE: usize = 1;
const ARTIST: usize = 2;
const ALBUM: usize = 3;
const ADDED: usize = 4;
const LIKE: usize = 5;
const DURATION: usize = 6;
const ACTIONS: usize = 7;
const TRACKS: usize = 8;

/// The track a column is solved from.
///
/// The playlist's denser weights apply when the artist column is present: the
/// artist and the album are the two flexible columns a playlist divides its room
/// between, and a library's album takes a smaller share.
fn track_for(index: usize, present: Present) -> Track {
    match index {
        INDEX => Track::fixed(extent::TRACK_INDEX_W),
        TITLE => Track {
            basis: 0.0,
            min: 0.0,
            max: f32::INFINITY,
            grow: if present.artist {
                grow::TITLE_PLAYLIST
            } else {
                grow::TITLE
            },
        },
        ARTIST => Track {
            basis: 0.0,
            min: 0.0,
            max: f32::INFINITY,
            grow: grow::ARTIST,
        },
        ALBUM => Track {
            basis: 0.0,
            min: 0.0,
            max: f32::INFINITY,
            grow: if present.artist {
                grow::ALBUM_PLAYLIST
            } else {
                grow::ALBUM
            },
        },
        ADDED => Track::fixed(extent::TRACK_ADDED_W),
        LIKE => Track::fixed(extent::TRACK_LIKE_W),
        DURATION => Track::fixed(extent::TRACK_DURATION_W),
        ACTIONS => Track::fixed(extent::TRACK_ACTIONS_W),
        _ => unreachable!("there are eight top-level tracks"),
    }
}

/// The gutter between two columns.
///
/// A playlist pays for its extra column with the tighter `space-3`; every other
/// list keeps `space-4`.
fn gap(present: Present) -> f32 {
    if present.artist { space::S3 } else { space::S4 }
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

/// Solves a row's eight columns inside a row `width` points wide.
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
        present.artist,
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
        .map(|(index, _)| track_for(index, present))
        .collect();
    let gutter = gap(present);
    let widths = solve(inner.width(), gutter, &tracks);

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
            cursor = right + gutter;
        } else {
            rects[index] = Rect::from_min_max(pos2(left, inner.top()), pos2(left, inner.bottom()));
        }
    }

    Columns {
        index: rects[INDEX],
        title: rects[TITLE],
        artist: rects[ARTIST],
        album: rects[ALBUM],
        added: rects[ADDED],
        like: rects[LIKE],
        duration: rects[DURATION],
        actions: rects[ACTIONS],
    }
}

/// The square a row's number or glyph sits in, right-justified inside the index
/// column.
///
/// The reference draws the number, the play glyph and the equaliser in a `1.5rem`
/// box at the column's right edge rather than across the whole `2.25rem` column,
/// and the header's `#` uses the same box so the two line up.
pub fn number_box(index: Rect) -> Rect {
    let side = extent::TRACK_NUMBER_W;
    let width = side.min(index.width().max(0.0));
    let height = side.min(index.height().max(0.0));
    Rect::from_min_size(
        pos2(index.right() - width, index.center().y - height / 2.0),
        vec2(width, height),
    )
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
    use crate::layout::{Table, WINDOW_MIN};

    fn all() -> Present {
        Present {
            artist: true,
            album: true,
            added: true,
            like: true,
            duration: true,
            actions: true,
        }
    }

    /// A library row: the list already says whose record it is, so no artist
    /// column, but it does carry the album.
    fn library() -> Present {
        Present {
            artist: false,
            ..all()
        }
    }

    /// An album block: the block header names the record, so neither the artist
    /// nor the album takes a column, and its rows carry no added date.
    fn album_block() -> Present {
        all().for_album()
    }

    fn rects(columns: Columns) -> [(&'static str, Rect); 8] {
        [
            ("index", columns.index),
            ("title", columns.title),
            ("artist", columns.artist),
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
    fn only_a_playlist_uri_is_a_playlist() {
        for uri in [
            "spotify:playlist:abc",
            "spotify:user:u:playlist:abc",
            "spotify:playlist:abc:extra",
        ] {
            assert!(is_playlist(uri), "must accept {uri}");
        }
        for uri in [
            "spotify:album:abc",
            "spotify:artist:playlist-fan",
            "spotify:track:abc",
            "spotify:local:Artist:Album:Title:222",
            "spotify:user:u:collection",
            "spotify:user:someone:folder:abc",
            "spotify:playlist",
            "spotify:playlist:",
            "spotify:user:u:playlist",
            "spotify:user:u:playlist:",
            "",
            "https://i.scdn.co/image/abc",
        ] {
            assert!(!is_playlist(uri), "must reject {uri}");
        }
    }

    #[test]
    fn only_a_playlist_presents_the_date_added() {
        for table in [Table::Full, Table::Reduced, Table::Narrow] {
            let present = table.columns();
            assert_eq!(
                present.for_playlist(true),
                present,
                "{table:?} must keep its own columns for a playlist"
            );
            assert_eq!(
                present.for_playlist(false),
                Present {
                    added: false,
                    ..present
                },
                "{table:?} must drop only the date for a non-playlist"
            );
        }
    }

    #[test]
    fn the_columns_sum_to_the_available_width_when_it_is_large_enough() {
        let width = 1000.0;
        let columns = columns(width, all());
        let sum = columns.index.width()
            + columns.title.width()
            + columns.artist.width()
            + columns.album.width()
            + columns.added.width()
            + columns.like.width()
            + columns.duration.width()
            + columns.actions.width()
            + 7.0 * space::S3;
        assert_close(sum, width - 2.0 * space::S3);
    }

    #[test]
    fn an_absent_column_takes_no_width_and_no_gap() {
        let width = 1000.0;
        let hidden = columns(width, album_block());
        assert_eq!(hidden.artist.width(), 0.0);
        assert_eq!(hidden.album.width(), 0.0);
        assert_eq!(hidden.added.width(), 0.0);

        // An album block keeps the title, the like, the duration and the
        // actions, with `space-4` between them.
        let sum = hidden.index.width()
            + hidden.title.width()
            + hidden.like.width()
            + hidden.duration.width()
            + hidden.actions.width()
            + 4.0 * space::S4;
        assert_close(sum, width - 2.0 * space::S3);

        // The title takes the dropped columns' share of the growth.
        assert!(hidden.title.width() > columns(width, all()).title.width());
    }

    #[test]
    fn an_album_block_clears_the_artist_album_and_added_columns() {
        let width = 1000.0;
        let present = all().for_album();
        assert!(!present.artist, "an album block names no artist column");
        assert!(!present.album, "an album block names no album column");
        assert!(!present.added, "an album position has no membership date");

        let columns = columns(width, present);
        assert_eq!(columns.artist.width(), 0.0);
        assert_eq!(columns.album.width(), 0.0);
        assert_eq!(columns.added.width(), 0.0);

        // The controls the album table keeps: the like, the length, the more.
        assert!(columns.like.width() > 0.0, "the album keeps the like");
        assert!(columns.duration.width() > 0.0);
        assert!(columns.actions.width() > 0.0);

        // The title starts one space-4 after the index, with no artist between.
        assert_close(columns.title.left(), columns.index.right() + space::S4);

        // Five present columns tile the inner width with four space-4 gutters.
        let sum = columns.index.width()
            + columns.title.width()
            + columns.like.width()
            + columns.duration.width()
            + columns.actions.width()
            + 4.0 * space::S4;
        assert_close(sum, width - 2.0 * space::S3);
    }

    #[test]
    fn the_album_title_takes_all_the_growth_the_dropped_columns_had() {
        // The title is the only grower left, so its width is exactly what the
        // fixed columns and the four gutters leave: this pins `grow::TITLE` as
        // the album block's weight rather than a second flexible column.
        let width = 1000.0;
        let columns = columns(width, all().for_album());
        let fixed = extent::TRACK_INDEX_W
            + extent::TRACK_LIKE_W
            + extent::TRACK_DURATION_W
            + extent::TRACK_ACTIONS_W;
        assert_close(
            columns.title.width(),
            width - 2.0 * space::S3 - fixed - 4.0 * space::S4,
        );
    }

    #[test]
    fn only_the_widest_album_block_keeps_the_like() {
        for table in [Table::Full, Table::Reduced, Table::Narrow] {
            let present = table.columns().for_album();
            assert!(
                !present.artist && !present.album && !present.added,
                "{table:?} must clear the same three columns"
            );
            assert_eq!(
                present.like,
                table != Table::Narrow,
                "{table:?} keeps the like exactly until its breakpoint drops it"
            );
            assert!(
                present.duration && present.actions,
                "{table:?} always keeps the length and the more"
            );
        }
    }

    #[test]
    fn only_a_playlist_carries_the_artist_column() {
        let width = 900.0;
        let playlist = columns(width, all());
        let library = columns(width, library());
        let album_block = columns(width, album_block());

        assert!(playlist.artist.width() > 0.0);
        assert_eq!(library.artist.width(), 0.0);
        assert_eq!(album_block.artist.width(), 0.0);

        assert!(library.album.width() > 0.0);
        assert_eq!(album_block.album.width(), 0.0);

        // The absent artist leaves no gap behind: the album starts where the
        // title ends plus one gutter.
        assert_close(library.album.left(), library.title.right() + space::S4);
    }

    #[test]
    fn the_title_starts_after_the_index_and_its_gutter() {
        for present in [all(), library(), album_block()] {
            let columns = columns(900.0, present);
            let gutter = if present.artist { space::S3 } else { space::S4 };
            assert_close(columns.title.left(), columns.index.right() + gutter);
        }
    }

    #[test]
    fn the_number_box_sits_at_the_right_of_the_index_column() {
        let columns = columns(900.0, all());
        let box_rect = number_box(columns.index);

        assert_eq!(box_rect.width(), extent::TRACK_NUMBER_W);
        assert_eq!(box_rect.height(), extent::TRACK_NUMBER_W);
        assert_close(box_rect.right(), columns.index.right());
        assert_close(box_rect.center().y, columns.index.center().y);

        // A column narrower than the box never lets it spill.
        let thin = Rect::from_min_size(pos2(10.0, 0.0), vec2(10.0, 10.0));
        let box_rect = number_box(thin);
        assert_eq!(box_rect.width(), 10.0);
        assert_close(box_rect.right(), thin.right());
    }

    #[test]
    fn a_playlist_rows_columns_never_overlap_at_its_narrowest_width() {
        // The narrowest content area the shell hands the tracklist at WINDOW_MIN,
        // then every width up to a full playlist. A playlist keeps the artist by
        // dropping the album; below 760 it also drops the date and the like.
        let narrowest = WINDOW_MIN[0];
        for present in [
            Present {
                album: false,
                ..all()
            },
            Present {
                album: false,
                added: false,
                like: false,
                ..all()
            },
        ] {
            for width in [narrowest, 400.0, 700.0, 900.0, 1440.0] {
                let columns = columns(width, present);
                let mut previous: Option<(&str, Rect)> = None;
                for (name, rect) in rects(columns) {
                    if rect.width() <= 0.0 {
                        continue;
                    }
                    if let Some((before, left)) = previous {
                        assert!(
                            rect.left() >= left.right() - 0.01,
                            "{name} at {width} starts at {} before {before} ends at {}",
                            rect.left(),
                            left.right()
                        );
                    }
                    previous = Some((name, rect));
                }
            }
        }
    }

    #[test]
    fn every_column_stays_inside_the_inner_rect_at_the_minimum_window() {
        // The narrowest content area the shell can hand the tracklist at
        // WINDOW_MIN, with every column present, then widths below it.
        for width in [
            WINDOW_MIN[0] - extent::SIDEBAR_W,
            WINDOW_MIN[0],
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
