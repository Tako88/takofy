//! What the Home page asks the session for, and what the session answers with.
//!
//! Plain data: `takofy-core` fills it in from Spotify and the window draws it,
//! so neither side has to depend on the other to talk about a shelf.

use crate::{CoverSet, FollowedArtist, ShowDetail, TrackEntry};

/// The kind of thing a station was seeded from, in the order the page seeds
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedKind {
    Playlist,
    Artist,
    Album,
    Song,
}

/// The one item a station was seeded from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StationSeed {
    pub kind: SeedKind,
    /// The seed's own URI, which is the station's identity on the page.
    pub uri: String,
}

/// The picture a station's head draws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StationArt {
    /// One cover, for a station whose seed names a single picture.
    Single(CoverSet),
    /// The parts of a mosaic, between [`crate::COMPOSED_MIN`] and
    /// [`crate::COMPOSED_MAX`] of them: fewer tile nothing, so a single valid
    /// part is [`Self::Single`].
    Mosaic(Vec<CoverSet>),
}

/// A station read from its seed, as its head and its rows draw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Station {
    /// The seed the station was read for, so a late answer is matched to the
    /// section that asked for it rather than to whichever is open.
    pub seed: StationSeed,
    /// The station's own URI, which is what playing it hands to the queue.
    pub uri: String,
    pub title: String,
    /// The station's own subtitle, when the wire gave one.
    ///
    /// A playlist or artist station names no subtitle, so the absent answer is
    /// `None` rather than an empty string the head would have to test for.
    pub subtitle: Option<String>,
    /// `None` when no part of the wire's picture decoded.
    pub art: Option<StationArt>,
    /// The station's tracks, in the wire's order. Empty when every row it sent
    /// was something other than a track.
    pub rows: Vec<TrackEntry>,
}

/// One read a Home shelf wants the session to perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeAsk {
    /// The artists related to `artist`, for the shelf that follows it.
    Related { artist: String },
    /// One followed show's episodes.
    Show { show: String },
    /// One station, read from its seed.
    Station { seed: StationSeed },
    /// Names for these tracks, in the order they were asked for.
    Labels { tracks: Vec<String> },
}

/// The answer to a [`HomeAsk`], matched back to it by what the ask named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeAnswer {
    /// The related artists, which may be empty when the artist has none.
    Related {
        artist: String,
        cards: Vec<FollowedArtist>,
    },
    Show(ShowDetail),
    Station(Station),
    /// The tracks that could be named, paired with the URIs that were asked
    /// for: an unreadable track is missing rather than answered with a blank.
    Labelled {
        asked: Vec<String>,
        named: Vec<TrackEntry>,
    },
}
