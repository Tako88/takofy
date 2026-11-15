//! The artists the user never wants offered again, the positions a day's rows
//! start at, and the file that holds both.
//!
//! Hand-parsed `key=value` lines, like the preferences file: unknown keys are
//! skipped so a file from a later version still loads, and a line this version
//! cannot read refuses the whole file rather than half-applying it.
//!
//! The set is permanent and per artist. It is not a record of what was seen, so
//! no row ever writes to it after looking somebody up.

use std::collections::BTreeSet;
use std::fmt;

use takofy_model::home::Day;
use takofy_model::{UriKind, parse as parse_uri};

/// How many artists one file may hide.
pub const HIDE_MAX: usize = 10_000;

/// The only built-in key that may repeat.
const HIDE_KEY: &str = "hide";

/// A key the file holds once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    Format,
    Day,
    Playlist,
    Artist,
    Album,
}

impl Key {
    /// Every singular key, in the order the file lists them.
    const ALL: [Key; 5] = [
        Key::Format,
        Key::Day,
        Key::Playlist,
        Key::Artist,
        Key::Album,
    ];

    /// The spelling in the file, which is also the spelling a reader matches.
    fn name(self) -> &'static str {
        match self {
            Key::Format => "format",
            Key::Day => "day",
            Key::Playlist => "playlist",
            Key::Artist => "artist",
            Key::Album => "album",
        }
    }

    /// The key `name` spells, or `None` for any key this version does not know.
    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.name() == name)
    }

    /// Where the three row positions sit in the stored array.
    fn slot(self) -> Option<usize> {
        match self {
            Key::Playlist => Some(0),
            Key::Artist => Some(1),
            Key::Album => Some(2),
            Key::Format | Key::Day => None,
        }
    }
}

/// Why a line refused the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reason {
    /// The line is not `key=value`.
    NoEquals,
    /// A key this version knows is padded with space.
    SpaceAroundKey,
    /// A key holds a value outside the table.
    BadValue(Key),
    /// A key that may appear once appears on an earlier line.
    Duplicate(Key),
    /// The same artist is hidden on an earlier line.
    DuplicateHide,
    /// A hidden value is not an artist address.
    NotAnArtist,
    /// A row position was written without the day it belongs to.
    PositionsWithoutDay,
    /// The file hides more artists than [`HIDE_MAX`].
    TooManyHides,
}

/// A refused line.
///
/// The value text is never kept, so a warning cannot print a hidden address or
/// whatever else the user wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refusal {
    line: usize,
    reason: Reason,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let line = self.line;
        match self.reason {
            Reason::NoEquals => write!(f, "line {line}: no `=`"),
            Reason::SpaceAroundKey => write!(f, "line {line}: space around a key"),
            Reason::BadValue(key) => {
                write!(f, "line {line}: bad value for `{}`", key.name())
            }
            Reason::Duplicate(key) => {
                write!(f, "line {line}: `{}` appears twice", key.name())
            }
            Reason::DuplicateHide => write!(f, "line {line}: `{HIDE_KEY}` appears twice"),
            Reason::NotAnArtist => write!(f, "line {line}: not an artist address"),
            Reason::PositionsWithoutDay => write!(f, "line {line}: a position without `day`"),
            Reason::TooManyHides => {
                write!(f, "line {line}: more than {HIDE_MAX} hidden artists")
            }
        }
    }
}

/// The hidden artists and the positions a day's rows start at.
#[derive(Clone, Debug, Default)]
pub struct Hidden {
    day: Option<i32>,
    positions: [usize; 3],
    artists: BTreeSet<String>,
    revision: u64,
}

impl PartialEq for Hidden {
    fn eq(&self, other: &Self) -> bool {
        self.day == other.day && self.positions == other.positions && self.artists == other.artists
    }
}

impl Eq for Hidden {}

impl Hidden {
    /// Hides `artist`, or reports that the set could not take it.
    ///
    /// Nothing is written at the cap, when the artist is already hidden, or when
    /// the value is not an artist address: a set never writes a file it would
    /// refuse to read.
    pub fn hide(&mut self, artist: &str) -> bool {
        if self.artists.len() >= HIDE_MAX || self.artists.contains(artist) || !is_artist(artist) {
            return false;
        }
        self.artists.insert(artist.to_owned());
        self.bump();
        true
    }

    /// Unhides `artist`, or reports that it was not hidden.
    pub fn unhide(&mut self, artist: &str) -> bool {
        if !self.artists.remove(artist) {
            return false;
        }
        self.bump();
        true
    }

    /// Whether `artist` is hidden.
    pub fn contains(&self, artist: &str) -> bool {
        self.artists.contains(artist)
    }

    /// Where `day`'s rows start, or `None` when the stored positions belong to
    /// another day.
    pub fn positions_for(&self, day: Day) -> Option<[usize; 3]> {
        (self.day == Some(day.days())).then_some(self.positions)
    }

    /// Records where `day`'s rows start.
    pub fn set_positions(&mut self, day: Day, positions: [usize; 3]) {
        if self.day == Some(day.days()) && self.positions == positions {
            return;
        }
        self.day = Some(day.days());
        self.positions = positions;
        self.bump();
    }

    /// How many real changes the running set has seen.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// The file this set writes.
    pub fn render(&self) -> String {
        let mut text = String::from("format=1\n");
        if let Some(day) = self.day {
            text.push_str(&format!(
                "day={day}\nplaylist={}\nartist={}\nalbum={}\n",
                self.positions[0], self.positions[1], self.positions[2]
            ));
        }
        for artist in &self.artists {
            text.push_str(&format!("{HIDE_KEY}={artist}\n"));
        }
        text
    }

    /// Reads the file.
    ///
    /// An absent `format` means 1, and any other value refuses the whole text:
    /// nothing is half-applied.
    pub fn parse(text: &str) -> Result<Self, Refusal> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut hidden = Self::default();
        let mut seen: Vec<Key> = Vec::new();
        let mut position_line: Option<usize> = None;

        for (index, raw) in text.split('\n').enumerate() {
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            let refuse = |reason| Refusal {
                line: index + 1,
                reason,
            };
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| refuse(Reason::NoEquals))?;
            let name = key.trim();
            if name == HIDE_KEY {
                if key != name {
                    return Err(refuse(Reason::SpaceAroundKey));
                }
                if !is_artist(value) {
                    return Err(refuse(Reason::NotAnArtist));
                }
                if hidden.artists.contains(value) {
                    return Err(refuse(Reason::DuplicateHide));
                }
                if hidden.artists.len() >= HIDE_MAX {
                    return Err(refuse(Reason::TooManyHides));
                }
                hidden.artists.insert(value.to_owned());
                continue;
            }
            let Some(known) = Key::from_name(name) else {
                continue;
            };
            if key != name {
                return Err(refuse(Reason::SpaceAroundKey));
            }
            if seen.contains(&known) {
                return Err(refuse(Reason::Duplicate(known)));
            }
            seen.push(known);
            match known {
                Key::Format => {
                    if value != "1" {
                        return Err(refuse(Reason::BadValue(known)));
                    }
                }
                Key::Day => {
                    let day = value
                        .parse::<i32>()
                        .map_err(|_| refuse(Reason::BadValue(known)))?;
                    hidden.day = Some(day);
                }
                Key::Playlist | Key::Artist | Key::Album => {
                    let position = value
                        .parse::<usize>()
                        .map_err(|_| refuse(Reason::BadValue(known)))?;
                    if let Some(slot) = known.slot() {
                        hidden.positions[slot] = position;
                    }
                    if position_line.is_none() {
                        position_line = Some(index + 1);
                    }
                }
            }
        }

        if hidden.day.is_none()
            && let Some(line) = position_line
        {
            return Err(Refusal {
                line,
                reason: Reason::PositionsWithoutDay,
            });
        }
        Ok(hidden)
    }
}

/// Whether `text` is a Spotify artist address.
fn is_artist(text: &str) -> bool {
    parse_uri(text).is_some_and(|uri| uri.kind == UriKind::Artist)
}

#[cfg(test)]
mod tests;
