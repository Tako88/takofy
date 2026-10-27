//! Track-row column arithmetic.
//!
//! The reference composes a playlist row from eight top-level tracks — index,
//! title, artist, album, added, like, duration and actions — with the cover art
//! nested inside the title. A row is one of three shapes: the playlist's dense
//! row, which carries the artist and the album; the plays row, which is tight and
//! carries the artist alone; and the slim row, which carries neither credit.
//! `columns` solves those tracks from the fixed extents and the growth weights;
//! `title_parts` then splits the title track into its art box and its text.
//!
//! Every returned rect is clamped inside the row's inner rect. The solver hands
//! back every track's minimum when the fixed columns and gaps do not fit, so
//! without the clamp a narrow window would place a column past the row's right
//! edge. The clamp is the contract: nothing here overflows.

use egui::{Pos2, Rect, pos2, vec2};
use takofy_model::uri::{UriKind, parse};

use crate::layout::tracks::{Track, solve};
use crate::theme::{extent, grow, space};

/// The row template a table draws: which credit columns it carries, how their
/// room is shared, and how wide a gutter sits between them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    /// A playlist's dense row: the artist and the album, `space-3` apart.
    Playlist,
    /// The plays row: one credit in a tight share of the title, `space-4` apart.
    /// The credit is the artist on an album's table and the album on an
    /// artist's.
    Plays,
    /// A row with no credit column at all.
    Slim,
}

/// Which optional columns a row carries at the current width.
///
/// The index and the title are always present; this is the painter's per-width
/// choice for the rest. The table never decides which columns drop, it lays out
/// the ones it is told exist.
///
/// The credit constructors produce three combinations: `for_plays` keeps the
/// artist and clears the album and the date, `for_plays_album` keeps the album
/// in that one cell instead, and `for_slim` clears all three. A hand-built
/// `Present` may set `album` on a `Slim` shape — the only state that reads
/// `grow::ALBUM` — but the app never builds one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Present {
    pub shape: Shape,
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

    /// Drops the columns a plays row does not carry.
    ///
    /// A plays row names the track's own artist, so it keeps the artist column,
    /// but the list it sits in already says which record the track is on and a
    /// play has no membership date.
    pub fn for_plays(self) -> Self {
        Self {
            shape: Shape::Plays,
            artist: true,
            album: false,
            added: false,
            ..self
        }
    }

    /// The plays row that credits the album instead of the artist.
    ///
    /// An artist's own table names that artist on every row, so the column
    /// repeats what the page already says; the album is the fact that changes
    /// from row to row.
    pub fn for_plays_album(self) -> Self {
        Self {
            shape: Shape::Plays,
            artist: false,
            album: true,
            added: false,
            ..self
        }
    }

    /// Drops every credit column, as a release block's rows do.
    pub fn for_slim(self) -> Self {
        Self {
            shape: Shape::Slim,
            artist: false,
            album: false,
            added: false,
            ..self
        }
    }
}

/// Whether a context URI names a playlist: `spotify:playlist:<id>`, or the named
/// `spotify:user:<user>:playlist:<id>`.
///
/// The id is not inspected: a URI that names a playlist still names it whatever
/// it spells into the id, so this is a question of identity.
pub fn is_playlist(uri: &str) -> bool {
    parse(uri).is_some_and(|parsed| parsed.kind == UriKind::Playlist)
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
/// The shape decides the weights: a playlist is dense, with a wide title to pay
/// for the two credit columns it carries, while a plays row keeps a `1fr` title
/// and spends a narrow share on the one thing it credits. Only the playlist
/// carries both credits, and the plays row's credit column reads
/// `grow::CREDIT_PLAYS` whichever of the two it is.
fn track_for(index: usize, present: Present) -> Track {
    match index {
        INDEX => Track::fixed(extent::TRACK_INDEX_W),
        TITLE => Track {
            basis: 0.0,
            min: 0.0,
            max: f32::INFINITY,
            grow: if present.shape == Shape::Playlist {
                grow::TITLE_PLAYLIST
            } else {
                grow::TITLE
            },
        },
        ARTIST => Track {
            basis: 0.0,
            min: 0.0,
            max: f32::INFINITY,
            grow: if present.shape == Shape::Plays {
                grow::CREDIT_PLAYS
            } else {
                grow::ARTIST
            },
        },
        ALBUM => Track {
            basis: 0.0,
            min: 0.0,
            max: f32::INFINITY,
            grow: match present.shape {
                Shape::Playlist => grow::ALBUM_PLAYLIST,
                Shape::Plays => grow::CREDIT_PLAYS,
                Shape::Slim => grow::ALBUM,
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
/// A playlist pays for its extra columns with the tighter `space-3`; every other
/// shape keeps `space-4`.
fn gap(present: Present) -> f32 {
    if present.shape == Shape::Playlist {
        space::S3
    } else {
        space::S4
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
/// With `art`, the reference nests a `2.25rem` art box and a `space-3` gutter
/// inside the title track, so the split is `extent::LIBRARY_ART` then `space::S3`.
/// The art is held to the title's own width and the text is clamped to a
/// non-negative width, so neither can spill past the title. Without `art` there
/// is no box at all: the art is a zero-width rect at the title's left edge and the
/// whole track is the text.
pub fn title_parts(title: Rect, art: bool) -> TitleParts {
    if !art {
        return TitleParts {
            art: Rect::from_min_size(title.min, vec2(0.0, title.height())),
            text: title,
        };
    }
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

/// Whether any of `rect` lies inside `visible`.
///
/// `Rect::intersects` counts a shared edge as an overlap, which would lay out an
/// item just past the fold and fetch its cover for nothing.
pub fn on_screen(visible: Rect, rect: Rect) -> bool {
    rect.left() < visible.right()
        && visible.left() < rect.right()
        && rect.top() < visible.bottom()
        && visible.top() < rect.bottom()
}

/// The boundary nearest `y`: 0 is before the first row, `count` after the last.
///
/// The inverse of [`row_rect`], so a pointer's height and the row it landed on
/// are two readings of one layout. A degenerate pitch or a non-finite pointer
/// reports the first boundary rather than panicking.
pub fn boundary_at(list: Rect, y: f32, row_h: f32, count: usize) -> usize {
    if !y.is_finite() || !row_h.is_finite() || row_h <= 0.0 {
        return 0;
    }
    let boundary = ((y - list.top()) / row_h).round();
    boundary.clamp(0.0, count as f32) as usize
}

#[cfg(test)]
mod tests;
