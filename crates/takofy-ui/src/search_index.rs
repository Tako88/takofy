//! What a search answer contains besides its tracks: the albums they sit on
//! and the artists they credit.
//!
//! Pure, so the derivation and its caps are testable without a window.

use takofy_model::{CoverSet, TrackEntry};

/// How many rows of each kind a surface shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    pub tracks: usize,
    pub albums: usize,
    pub artists: usize,
}

/// The drop-down's caps: a surface property, not the derivation's, so the
/// results page asks for its own from the same shape.
pub const MENU: Caps = Caps {
    tracks: 3,
    albums: 2,
    artists: 2,
};

/// The results page's caps: ten of each kind until a block's reveal is taken.
pub const PAGE: Caps = Caps {
    tracks: 10,
    albums: 10,
    artists: 10,
};

/// One album an answer's tracks sit on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Album {
    pub uri: String,
    pub name: String,
    /// The first crediting track's first artist: the payload names no album
    /// performer of its own.
    pub artist: String,
    pub cover: Option<CoverSet>,
}

/// One artist credited on an answer's tracks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artist {
    pub uri: String,
    pub name: String,
    /// How many answer tracks credit this artist; a track crediting them twice
    /// counts once.
    pub tracks: usize,
    /// The cover of the record the match sits on, for an artist with no portrait.
    pub record_cover: Option<CoverSet>,
}

/// The lists an answer derives, in first-appearance order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Index {
    /// The answer's length, which is not capped: a band's total is the truth.
    pub tracks: usize,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
}

impl Index {
    /// Derives the albums and artists of `answer`.
    pub fn derive(answer: &[TrackEntry]) -> Self {
        let mut index = Self {
            tracks: answer.len(),
            ..Self::default()
        };
        for entry in answer {
            if let Some(album) = album_of(entry) {
                match index.albums.iter_mut().find(|found| found.uri == album.uri) {
                    // A later track fills the cover only when the first had none.
                    Some(found) => {
                        if found.cover.is_none() {
                            found.cover = album.cover;
                        }
                    }
                    None => index.albums.push(album),
                }
            }
            for (position, (uri, name)) in entry
                .artist_uris
                .iter()
                .zip(&entry.artist_names)
                .enumerate()
            {
                // A track crediting the same artist twice is one credit.
                if entry.artist_uris[..position].contains(uri) {
                    continue;
                }
                match index.artists.iter_mut().find(|found| &found.uri == uri) {
                    Some(found) => {
                        found.tracks += 1;
                        if found.record_cover.is_none() {
                            found.record_cover = entry.cover.clone();
                        }
                    }
                    None => index.artists.push(Artist {
                        uri: uri.clone(),
                        name: name.clone(),
                        tracks: 1,
                        record_cover: entry.cover.clone(),
                    }),
                }
            }
        }
        index
    }

    /// The bands `caps` draws from this index, in band order, empty bands
    /// omitted.
    pub fn shape(&self, caps: Caps) -> Shape {
        let bands: Vec<Band> = [
            (Kind::Tracks, self.tracks, caps.tracks),
            (Kind::Albums, self.albums.len(), caps.albums),
            (Kind::Artists, self.artists.len(), caps.artists),
        ]
        .into_iter()
        .filter(|(_, total, _)| *total > 0)
        .map(|(kind, total, cap)| Band {
            kind,
            shown: cap.min(total),
            total,
        })
        .collect();
        let floor = bands.iter().any(|band| band.shown < band.total);
        Shape { bands, floor }
    }
}

/// The album one track contributes, or `None` when it names no album.
fn album_of(entry: &TrackEntry) -> Option<Album> {
    Some(Album {
        uri: entry.album_uri.clone()?,
        name: entry.album.clone()?,
        artist: entry
            .artist_names
            .first()
            .cloned()
            .unwrap_or_else(|| entry.artists.clone()),
        cover: entry.cover.clone(),
    })
}

/// `"1 track"`, or `"N tracks"` for any other count.
pub fn tracks_phrase(tracks: usize) -> String {
    if tracks == 1 {
        "1 track".to_owned()
    } else {
        format!("{tracks} tracks")
    }
}

/// The kinds of row the menu walks, in the order their bands appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Tracks,
    Albums,
    Artists,
}

/// One band: a kind, how many of its rows a surface shows, and how many exist.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Band {
    pub kind: Kind,
    pub shown: usize,
    pub total: usize,
}

impl Kind {
    /// The kind's heading: what a menu band or a page block is called.
    pub fn heading(self) -> &'static str {
        match self {
            Kind::Tracks => "Tracks",
            Kind::Albums => "Albums",
            Kind::Artists => "Artists",
        }
    }
}

impl Band {
    /// The band's heading, in the menu's head grammar.
    pub fn heading(&self) -> &'static str {
        self.kind.heading()
    }

    /// The showing-versus-total phrase beside the heading.
    pub fn share(&self) -> String {
        if self.shown < self.total {
            format!("{} of {}", self.shown, self.total)
        } else {
            self.total.to_string()
        }
    }

    /// The heading and share as one phrase, for a screen reader: `Tracks, 10 of 20`.
    pub fn spoken(&self) -> String {
        format!("{}, {}", self.heading(), self.share())
    }

    /// The reveal row's words, or `None` when nothing is hidden.
    pub fn more(&self) -> Option<String> {
        let hidden = self
            .total
            .checked_sub(self.shown)
            .filter(|hidden| *hidden > 0)?;
        let noun = match (self.kind, hidden) {
            (Kind::Tracks, 1) => "track",
            (Kind::Tracks, _) => "tracks",
            (Kind::Albums, 1) => "album",
            (Kind::Albums, _) => "albums",
            (Kind::Artists, 1) => "artist",
            (Kind::Artists, _) => "artists",
        };
        Some(format!("Show {hidden} more {noun}"))
    }
}

/// The bands a surface draws, and whether a floor row sits under them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shape {
    pub bands: Vec<Band>,
    pub floor: bool,
}

impl Shape {
    /// How many rows the keyboard walks: every shown row, then the floor.
    pub fn len(&self) -> usize {
        self.bands.iter().map(|band| band.shown).sum::<usize>() + usize::from(self.floor)
    }

    /// Whether there is nothing to walk.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The row at `ordinal`, walking bands in order and the floor last.
    pub fn row(&self, ordinal: usize) -> Option<Row> {
        let mut walked = 0;
        for band in &self.bands {
            if ordinal < walked + band.shown {
                let index = ordinal - walked;
                return Some(match band.kind {
                    Kind::Tracks => Row::Track(index),
                    Kind::Albums => Row::Album(index),
                    Kind::Artists => Row::Artist(index),
                });
            }
            walked += band.shown;
        }
        (self.floor && ordinal == walked).then_some(Row::Floor)
    }

    /// The ordinal `row` sits at, or `None` when the shape does not draw it.
    pub fn ordinal(&self, row: Row) -> Option<usize> {
        let (kind, index) = match row {
            Row::Track(index) => (Kind::Tracks, index),
            Row::Album(index) => (Kind::Albums, index),
            Row::Artist(index) => (Kind::Artists, index),
            Row::Floor => return self.floor.then(|| self.len().saturating_sub(1)),
        };
        let mut walked = 0;
        for band in &self.bands {
            if band.kind == kind {
                return (index < band.shown).then_some(walked + index);
            }
            walked += band.shown;
        }
        None
    }

    /// The shown rows per band, as the layout stacks them.
    pub fn rows_per_band(&self) -> Vec<usize> {
        self.bands.iter().map(|band| band.shown).collect()
    }
}

/// One row of the walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Row {
    /// An index into the whole answer, which is a prefix of it.
    Track(usize),
    /// An index into [`Index::albums`].
    Album(usize),
    /// An index into [`Index::artists`].
    Artist(usize),
    Floor,
}

#[cfg(test)]
mod tests;
