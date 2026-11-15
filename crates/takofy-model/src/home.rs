//! What the Home page asks the session for, and what the session answers with.
//!
//! Plain data: `takofy-core` fills it in from Spotify and the window draws it,
//! so neither side has to depend on the other to talk about a shelf.

use crate::{ArtistPortrait, CoverSet, FollowedArtist, ShowDetail, TrackEntry};

/// The credit a compilation carries, which names no artist to reach further.
pub const VARIOUS_ARTISTS: &str = "Various Artists";

/// The kind of thing a station was seeded from, in the order the page seeds
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedKind {
    Playlist,
    Artist,
    Album,
    Song,
}

/// The kind of thing a discovery row was seeded from, in the order the page
/// offers the rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryKind {
    Playlist,
    Artist,
    Album,
}

impl DiscoveryKind {
    /// Every kind, in the order a row is drawn.
    pub const ALL: [Self; 3] = [Self::Playlist, Self::Artist, Self::Album];

    /// Where this kind sits in [`Self::ALL`].
    pub fn index(self) -> usize {
        match self {
            Self::Playlist => 0,
            Self::Artist => 1,
            Self::Album => 2,
        }
    }
}

/// The artist a discovery row reaches the outside from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryArtist {
    pub uri: String,
    pub name: String,
}

/// The one item a discovery row was seeded from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverySeed {
    pub kind: DiscoveryKind,
    /// The seed's own URI, which is the row's identity on the page.
    pub uri: String,
    /// The seed's name, as the shelf drew it.
    pub name: String,
    /// The artist the row is identified through. A playlist names one only when
    /// its rows agree, so the absent answer is `None`.
    pub anchor: Option<DiscoveryArtist>,
}

/// One artist recommended beside a seed, as its card draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryCard {
    pub artist: DiscoveryArtist,
    /// `None` when the wire offered no picture.
    pub portrait: Option<CoverSet>,
    /// The artist's top tracks, in the wire's order, up to [`CARD_TRACKS`]. The
    /// first is what playing the card hands to the queue; the rest are the rows
    /// a split card draws.
    pub tracks: Vec<TrackEntry>,
    /// The year of the artist's newest release, when the wire dated it.
    pub latest_year: Option<u16>,
}

/// How many of an artist's top tracks one card carries.
///
/// Three is what a split card draws as mini-rows and the most a card's play can
/// name; the wire's own top list is longer, so it is cut here.
pub const CARD_TRACKS: usize = 3;

/// What a discovery row came back with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryOutcome {
    /// The setting is off, so nothing was read.
    Off,
    /// The outside did not answer, which is not the same as finding nobody.
    Unreachable,
    /// The seed could not be identified outside, so the row is not drawn.
    Rejected,
    /// The neighbours the lookup found, with the genre they vote for when the
    /// vote was not split.
    Neighbours {
        cards: Vec<DiscoveryCard>,
        genre: Option<String>,
    },
}

/// The answer to a discovery ask, carrying the seed so a late answer lands on
/// the row that asked for it rather than on whichever is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryAnswer {
    pub seed: DiscoverySeed,
    pub outcome: DiscoveryOutcome,
}

/// A day number, which the binary computes from the local date.
///
/// Plain data that crosses the seam: it is a count, not a date, so no calendar
/// arithmetic lives here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Day(i32);

impl Day {
    pub fn from_days(days: i32) -> Self {
        Self(days)
    }

    pub fn days(self) -> i32 {
        self.0
    }
}

/// A release's date, in the parts the wire gave.
///
/// The month and the day are optional because the wire dates a release by as
/// little as its year, and a part it left out is not one the page can draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseDate {
    pub year: u16,
    pub month: Option<u8>,
    pub day: Option<u8>,
}

/// One release of a followed artist, as the release shelf draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRelease {
    /// The artist's own name, as the wire gave it.
    pub artist: String,
    /// The release's URI, which is what its card plays and opens.
    pub uri: String,
    pub name: String,
    pub date: ReleaseDate,
    /// The release's date as a day number, in the same count [`Day`] uses, so the
    /// shelf compares whole days without calendar arithmetic of its own.
    pub day: Day,
    pub cover: Option<CoverSet>,
}

/// One followed artist's releases, in the order the read answered them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistReleases {
    /// The artist's URI, which is what the ask named.
    pub artist: String,
    /// Empty when the artist has nothing new, which is an answer rather than a
    /// missing one.
    pub releases: Vec<NewRelease>,
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
    /// The newest episodes of every followed show the shelf draws, read
    /// together so the pool costs one label read rather than one per show.
    Episodes { shows: Vec<String> },
    /// One station, read from its seed.
    Station { seed: StationSeed },
    /// Names for these tracks, in the order they were asked for.
    Labels { tracks: Vec<String> },
    /// The faces of these artists, read together: the counted shelf draws a
    /// handful of tiles whose faces cost one read, not one each.
    Portraits { artists: Vec<String> },
    /// The neighbours one discovery row draws, looked up outside.
    Discovery { seed: DiscoverySeed },
    /// The newest releases of these followed artists, from `since` onwards.
    Releases { artists: Vec<String>, since: Day },
}

/// The answer to a [`HomeAsk`], matched back to it by what the ask named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeAnswer {
    /// The related artists, which may be empty when the artist has none.
    Related {
        artist: String,
        cards: Vec<FollowedArtist>,
    },
    /// One detail per show that answered, in the order the shows were read. A
    /// show the read could not get is missing rather than answered empty.
    Episodes(Vec<ShowDetail>),
    Station(Station),
    /// The tracks that could be named, paired with the URIs that were asked
    /// for: an unreadable track is missing rather than answered with a blank.
    Labelled {
        asked: Vec<String>,
        named: Vec<TrackEntry>,
    },
    /// The faces the read found, paired with the artists that were asked for.
    /// An artist it found no face for is missing rather than answered blank, so
    /// a late answer lands on the shelf that asked rather than on whichever is
    /// open.
    Portraits {
        asked: Vec<String>,
        found: Vec<ArtistPortrait>,
    },
    /// The neighbours a discovery row found, answered for its seed.
    Discovery(DiscoveryAnswer),
    /// One entry per artist the read answered, including an artist with nothing
    /// new. An artist the read could not get is missing rather than answered
    /// empty, so the shelf neither holds it nor waits on it again this open.
    Releases {
        asked: Vec<String>,
        found: Vec<ArtistReleases>,
    },
}
