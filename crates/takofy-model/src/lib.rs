//! Plain data shared by the engine and the window.
//!
//! Both `takofy-core` and `takofy-ui` depend on this crate, and it depends on
//! nothing. That is the point: the window can hold what it draws without
//! depending on the crate that talks to Spotify, so "a view cannot reach the
//! network" stays enforced by the compiler.

pub mod cover;
pub mod device;
pub mod remote;
pub mod uri;

pub use cover::{CoverSet, CoverVariant, SMALL_MAX_PX};

pub use device::{Device, DeviceKind, RemotePlayback, RemoteState};
pub use remote::{
    Landing, MAX_NEXT_TRACKS, MAX_PREV_TRACKS, Options, Order, Report, RequestId, Status,
};
pub use uri::{Uri, UriKind, parse};

/// An absolute URL for a cover image, restricted to the hosts Takofy fetches.
///
/// Only the two checked constructors build one, so a URL that reaches the
/// fetcher is one this crate chose rather than one a server supplied.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CoverUrl(String);

/// The only hosts a cover may be fetched from.
const COVER_HOSTS: [&str; 2] = ["i.scdn.co", "pickasso.spotifycdn.com"];

impl CoverUrl {
    /// Builds a CDN URL from a 20-byte image file id.
    ///
    /// Anything that is not exactly 20 bytes with at least one non-zero byte is
    /// `None`: a short id is zero-padded by the wire and is not a real image.
    pub fn from_file_id(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 20 || bytes.iter().all(|byte| *byte == 0) {
            return None;
        }
        use std::fmt::Write as _;
        let mut hex = String::with_capacity(40);
        for byte in bytes {
            let _ = write!(hex, "{byte:02x}");
        }
        Some(Self(format!("https://i.scdn.co/image/{hex}")))
    }

    /// Accepts only an absolute `https://` URL on a known image host.
    ///
    /// Characters that could make the fetching parser disagree about the host
    /// are rejected, so a server-supplied URL can never point somewhere else.
    pub fn from_wire(url: &str) -> Option<Self> {
        let rest = url.strip_prefix("https://")?;
        // A backslash or userinfo makes a lenient parser read a different host.
        if url
            .chars()
            .any(|c| c == '\\' || c == '@' || c.is_ascii_control() || c.is_whitespace())
        {
            return None;
        }
        let authority_end = rest.find(['/', '?', '#'])?;
        if !rest[authority_end..].starts_with('/') {
            return None;
        }
        let host_port = &rest[..authority_end];
        let host = match host_port.rsplit_once(':') {
            Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
                host
            }
            Some(_) => return None,
            None => host_port,
        };
        if !COVER_HOSTS.contains(&host) {
            return None;
        }
        Some(Self(url.to_owned()))
    }

    /// The URL as it will be fetched.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Decoded, tightly packed RGBA8 pixels for one cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl CoverImage {
    /// Builds an image, rejecting zero dimensions and a mismatched buffer.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))?;
        if rgba.len() != expected {
            return None;
        }
        Some(Self {
            width,
            height,
            rgba,
        })
    }
}

/// The longest playlist name the server accepts, in characters.
///
/// Counted in characters rather than bytes, because a name is what the user
/// typed and a multi-byte character is still one of them.
pub const PLAYLIST_NAME_MAX: usize = 100;

/// Where a playlist's cover came from.
///
/// A chosen cover is one a user set, so no row edit may replace it. A derived
/// cover is a copy of the playlist's first track, so an edit that changes that
/// track invalidates it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverOrigin {
    Chosen,
    Derived,
}

/// A playlist's cover together with where it came from.
///
/// The set is never absent: an origin with nothing to name cannot be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistCover {
    pub set: CoverSet,
    pub origin: CoverOrigin,
}

impl PlaylistCover {
    /// A cover a user set.
    pub fn chosen(cover: CoverSet) -> Self {
        Self {
            set: cover,
            origin: CoverOrigin::Chosen,
        }
    }

    /// A cover copied from the playlist's first track.
    pub fn derived(cover: CoverSet) -> Self {
        Self {
            set: cover,
            origin: CoverOrigin::Derived,
        }
    }
}

/// An owner's name as the wire spells it, checked once.
///
/// A blank name, or one made of digits alone, is an opaque account id rather
/// than a person, so it cannot be built and never reaches a screen.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OwnerName(String);

impl OwnerName {
    /// Reads a wire name, trimmed.
    ///
    /// Anything blank after trimming, or all ASCII digits, is not a name.
    pub fn from_wire(name: &str) -> Option<Self> {
        let name = name.trim();
        if name.is_empty() || name.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        Some(Self(name.to_owned()))
    }

    /// The name as it will be drawn.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Who owns a playlist, as the window draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    /// The account signed in, drawn without a name because the list is ours.
    You,
    /// Another account, named.
    Named(OwnerName),
}

impl Owner {
    /// The name to draw, when there is one to draw: our own list draws none.
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::You => None,
            Self::Named(name) => Some(name.as_str()),
        }
    }
}

/// How many covers a composed playlist cover tiles.
pub const COMPOSED_MAX: usize = 4;

/// The fewest covers that tile: one cover is a picture, not a mosaic.
pub const COMPOSED_MIN: usize = 2;

/// The first four distinct covers `head` names, in head order.
///
/// A playlist usually repeats an album, so distinctness is counted on the
/// covers, and an entry whose label did not arrive contributes nothing rather
/// than a hole. A chosen picture is the list's own and a mosaic may never
/// replace it; fewer than two covers have nothing to tile, so both give nothing.
pub fn composed_from<'a, I>(origin: Option<CoverOrigin>, head: I) -> Vec<CoverSet>
where
    I: IntoIterator<Item = Option<&'a CoverSet>>,
{
    if origin == Some(CoverOrigin::Chosen) {
        return Vec::new();
    }
    let mut picked: Vec<CoverSet> = Vec::new();
    for cover in head.into_iter().flatten() {
        if picked.len() == COMPOSED_MAX {
            break;
        }
        if !picked.contains(cover) {
            picked.push(cover.clone());
        }
    }
    if picked.len() < COMPOSED_MIN {
        return Vec::new();
    }
    picked
}

/// A playlist as listed in the rootlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistEntry {
    pub uri: String,
    pub name: String,
    /// `false` when `name` is the URI fallback.
    pub named: bool,
    /// The playlist's cover, once one of the three sources names it.
    pub cover: Option<PlaylistCover>,
    /// Up to four distinct album covers from the playlist's head, in track order.
    ///
    /// Empty when the playlist has a picture a user chose, or when its head
    /// names fewer than two distinct covers: a card then draws the single
    /// picture. A mosaic is only ever drawn in place of [`CoverOrigin::Derived`]
    /// or a missing cover.
    ///
    /// A derived cover arriving does not empty this, because the band goes on
    /// tiling the same head and the card must keep agreeing with it. Only a
    /// change to the rows does.
    pub composed: Vec<CoverSet>,
    /// The playlist's own description, collapsed to a line, when it has one.
    ///
    /// `None` is the wire giving nothing; an empty description is `None` rather
    /// than `Some("")`.
    pub description: Option<String>,
    /// The playlist's declared track count, when the wire named one.
    ///
    /// This is the playlist's length, not the number of tracks a truncated
    /// read returned.
    pub tracks: Option<usize>,
    /// Who owns the playlist, when the read named a person.
    ///
    /// Our own lists are [`Owner::You`] and draw no name; another account's
    /// numeric id is not a person, so it is no owner at all.
    pub owner: Option<Owner>,
    /// The revision the entry was read at and whether its items may be edited.
    ///
    /// `None` when the read named no revision: an entry with no access names no
    /// version of its own list, so nothing may be written through it.
    pub access: Option<PlaylistAccess>,
    /// When the playlist was saved to the library, when the rootlist dated it.
    pub saved_at: Option<SavedAt>,
}

impl PlaylistEntry {
    /// The playlist's cover image, whatever its origin.
    pub fn cover_set(&self) -> Option<&CoverSet> {
        self.cover.as_ref().map(|cover| &cover.set)
    }

    /// The revision and length an add would be based on, when one may be made.
    ///
    /// It needs an editable list read at a known revision and a known count: the
    /// count is the index an append names, so an unknown one appends nowhere.
    pub fn add_basis(&self) -> Option<(&Revision, usize)> {
        let access = self.access.as_ref()?;
        if !access.editable {
            return None;
        }
        Some((&access.revision, self.tracks?))
    }
}

/// When a track joined the playlist it is drawn in, in epoch milliseconds.
///
/// A separate type from a duration, which is also a millisecond count, and from
/// [`PublishedAt`] and [`SavedAt`], so the two can never be passed for each
/// other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddedAt(u64);

/// The earliest plausible date, 2001-09-09: a value below it is a unit
/// mistake rather than a date, so it is read as no date at all.
const ADDED_AT_FLOOR_MS: u64 = 1_000_000_000_000;

impl AddedAt {
    /// Reads the wire's decimal millisecond count.
    ///
    /// Only digits are accepted, so a sign, a space or an exponent is not a
    /// count, and a value below [`ADDED_AT_FLOOR_MS`] is rejected as a
    /// seconds-vs-milliseconds mix-up rather than read as a date.
    pub fn from_wire(wire: &str) -> Option<Self> {
        if wire.is_empty() || !wire.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let millis: u64 = wire.parse().ok()?;
        (millis >= ADDED_AT_FLOOR_MS).then_some(Self(millis))
    }

    /// The instant as epoch milliseconds.
    pub fn as_millis(self) -> u64 {
        self.0
    }
}

/// When an episode was published, in epoch milliseconds.
///
/// A separate type from [`AddedAt`], which dates a playlist membership, so the
/// two can never be passed for each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishedAt(u64);

impl PublishedAt {
    /// Reads the wire's millisecond count.
    ///
    /// A failed `Date` conversion or non-positive millis gives `None`.
    pub fn from_millis(millis: i64) -> Option<Self> {
        let millis = u64::try_from(millis).ok()?;
        (millis > 0).then_some(Self(millis))
    }

    /// The instant as epoch milliseconds.
    pub fn as_millis(self) -> u64 {
        self.0
    }
}

/// When a playlist, album or liked song was saved, in epoch milliseconds.
///
/// A separate type from [`AddedAt`] and [`PublishedAt`], so a save date can
/// never be passed for a track's added date or an episode's publish date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SavedAt(u64);

impl SavedAt {
    /// Reads a millisecond count.
    ///
    /// Zero is no date, because the wire answers `0` for an entry it cannot
    /// date, and a negative count is not an instant at all.
    pub fn from_millis(millis: i64) -> Option<Self> {
        let millis = u64::try_from(millis).ok()?;
        (millis > 0).then_some(Self(millis))
    }

    /// Reads an epoch-seconds count, the unit the collection reads answer in.
    ///
    /// A count too large to scale to milliseconds is no date.
    pub fn from_secs(secs: i64) -> Option<Self> {
        Self::from_millis(secs.checked_mul(1000)?)
    }

    /// The instant as epoch milliseconds.
    pub fn as_millis(self) -> u64 {
        self.0
    }
}

/// A liked song and the date it was liked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LikedTrack {
    pub uri: String,
    /// `None` when the wire gave no usable date for the song.
    pub saved_at: Option<SavedAt>,
}

/// A track as a list row draws it.
///
/// A playlist position and an album position are both rows, so both use this.
/// An album position is not a membership, so its [`Self::added_at`] is always
/// `None`: an album is a record, not a date the track joined something.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackEntry {
    pub uri: String,
    pub title: String,
    pub artists: String,
    /// The album the track belongs to, when the metadata carried one.
    pub album: Option<String>,
    /// The album's URI, when the metadata carried a resolvable album id.
    ///
    /// It does not depend on [`Self::album`] being named: an empty album name
    /// still yields a URI, and the row decides whether it can draw a link.
    pub album_uri: Option<String>,
    /// The artists' URIs, in the same order as [`Self::artists`].
    ///
    /// The list stops at the first artist that contributes no name or whose id
    /// does not resolve, so a link never points at a different artist than the
    /// one named in the same place.
    pub artist_uris: Vec<String>,
    /// The artists' names, same length and order as [`Self::artist_uris`].
    ///
    /// The joined [`Self::artists`] cannot be split back into names, because a
    /// name may itself contain the joining comma.
    pub artist_names: Vec<String>,
    /// When the track joined *the playlist that drew it*; per position, not per
    /// track, and always `None` on an album's rows.
    pub added_at: Option<AddedAt>,
    /// The track's length in milliseconds, when the metadata carried a length.
    pub duration_ms: Option<u32>,
    /// `false` when `title` is the URI fallback.
    pub named: bool,
    /// The track's own album cover, when the metadata carried one.
    pub cover: Option<CoverSet>,
}

impl TrackEntry {
    /// Whether this entry is a track a playlist can hold.
    ///
    /// An episode and a local file are rows too, but neither may be added to a
    /// playlist, so the URI kind decides rather than the row's shape.
    pub fn is_track(&self) -> bool {
        uri::parse(&self.uri).is_some_and(|uri| uri.kind == UriKind::Track)
    }
}

/// An episode as a page or a list row draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Episode {
    pub uri: String,
    pub title: String,
    /// The show's name, or empty when the wire named none.
    pub show: String,
    /// The show's URI, when the metadata carried a resolvable show id.
    pub show_uri: Option<String>,
    /// The episode's own description, collapsed to a line, when it has one.
    pub description: Option<String>,
    /// When the episode was published, when the wire gave a usable date.
    pub published: Option<PublishedAt>,
    /// The episode's length in milliseconds, when the metadata carried one.
    pub duration_ms: Option<u32>,
    pub cover: Option<CoverSet>,
    /// `false` when `title` is the URI fallback.
    pub named: bool,
}

impl Episode {
    /// The episode as a queue or playlist row draws it.
    ///
    /// The show stands in for the artists, and an unnamed show contributes no
    /// credit URI because there is nothing to link to.
    pub fn entry(&self) -> TrackEntry {
        let mut artist_uris = Vec::new();
        let mut artist_names = Vec::new();
        if !self.show.is_empty()
            && let Some(uri) = self.show_uri.clone()
        {
            artist_uris.push(uri);
            artist_names.push(self.show.clone());
        }
        TrackEntry {
            uri: self.uri.clone(),
            title: self.title.clone(),
            artists: self.show.clone(),
            album: None,
            album_uri: None,
            artist_uris,
            artist_names,
            added_at: None,
            duration_ms: self.duration_ms,
            named: self.named,
            cover: self.cover.clone(),
        }
    }
}

/// A loaded album, as its page draws it.
///
/// Plain data by design: `takofy-ui` has no librespot dependency, so the year is
/// a number the engine already read from the wire's date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumDetail {
    pub uri: String,
    /// The album's name, or the URI when the wire gave no name.
    pub name: String,
    /// The album's artists joined with `, `; empty when the wire named none.
    pub artists: String,
    /// The URI of the first artist named in [`Self::artists`].
    ///
    /// A joined credit gets one link, and it goes to that first name. `None`
    /// when no artist is named or the first named one has no resolvable
    /// artist id, so a link never points at a name drawn somewhere else.
    pub artist_uri: Option<String>,
    /// The release year, or `None` for a wire year of zero or none at all.
    ///
    /// A `time::Date` cannot hold a year past 9999, so a wider integer is not a
    /// case this can see.
    pub year: Option<u16>,
    pub cover: Option<CoverSet>,
    /// Every track, in the album's own order. [`TrackEntry::added_at`] is always
    /// `None`: an album position carries no membership date.
    pub tracks: Vec<TrackEntry>,
}

/// What a release is, as its block labels it.
///
/// The wire distinguishes more kinds than the reference names; anything this
/// build does not draw a word for is [`Self::Other`] rather than a guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseKind {
    Album,
    Ep,
    Single,
    Compilation,
    /// An audiobook, a podcast, or a wire value this build does not name.
    Other,
}

/// One release in an artist's discography, as its block draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub uri: String,
    /// The release's name, or the URI when the wire gave no name.
    pub name: String,
    pub kind: ReleaseKind,
    /// The release year, or `None` for a wire year of zero or none at all.
    pub year: Option<u16>,
    pub cover: Option<CoverSet>,
    /// The release's track URIs in disc order.
    ///
    /// Names are fetched only while the block is on screen, so this is what a
    /// label request carries. It is not a [`TrackEntry`]: nothing has been
    /// labelled yet.
    pub tracks: Vec<String>,
}

/// A loaded artist, as the artist page draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistDetail {
    pub uri: String,
    /// The artist's name, or the URI when the wire gave no name.
    pub name: String,
    /// The portrait the hero draws, when the wire carried one.
    pub portrait: Option<CoverSet>,
    /// The artist's top tracks for the session's country.
    pub top_tracks: Vec<TrackEntry>,
    /// Every release the artist payload named: albums, then singles, then
    /// compilations, deduped, in the wire's own order.
    pub releases: Vec<Release>,
    /// The payload's likeness list, URIs only, capped by the read.
    pub related: Vec<String>,
}

/// A show, as its page and the followed-shows list draw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Show {
    pub uri: String,
    /// The show's name, or the URI when the wire gave no name.
    pub name: String,
    /// The publisher, or empty when the wire named none.
    pub publisher: String,
    /// The show's own description as the wire sends it, empty when it has none.
    pub description: String,
    pub cover: Option<CoverSet>,
}

/// A loaded show, as its page draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShowDetail {
    pub show: Show,
    /// Every episode, in the show's own order.
    pub episodes: Vec<Episode>,
}

/// One show in the followed-shows list, as its card draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowedShow {
    pub uri: String,
    /// The show's name, or the URI when the metadata did not answer.
    pub name: String,
    /// `false` when `name` is the URI fallback.
    pub named: bool,
    pub publisher: Option<String>,
    /// How many episodes the show lists, when its metadata answered.
    pub episodes: Option<usize>,
    pub cover: Option<CoverSet>,
}

/// One artist in the followed-artists list, as its card draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowedArtist {
    pub uri: String,
    /// The artist's name, or the URI when the metadata did not answer.
    pub name: String,
    /// `false` when `name` is the URI fallback.
    pub named: bool,
    pub portrait: Option<CoverSet>,
    /// How many current releases the artist has, when they could be read.
    pub releases: Option<u32>,
}

/// One artist's portrait, as a search menu's artist row draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistPortrait {
    pub artist: String,
    /// `None` when the artist's record carries no portrait group.
    pub portrait: Option<CoverSet>,
}

/// One album in the saved-albums list, as its card draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedAlbum {
    pub uri: String,
    /// The album's name, or the URI when the metadata did not answer.
    pub name: String,
    /// `false` when `name` is the URI fallback.
    pub named: bool,
    /// The album's artists, joined with ", ". Empty when none were named.
    pub artists: String,
    pub year: Option<u16>,
    pub cover: Option<CoverSet>,
    /// The album's track count, when its discs could be read.
    pub tracks: Option<u32>,
    /// When the album was saved to the library, when the read dated it.
    pub saved_at: Option<SavedAt>,
}

/// One line of a track's lyrics, as the lyrics pane draws it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LyricLine {
    /// When the line starts singing, in milliseconds, when the provider timed it.
    pub start_ms: Option<u32>,
    /// When the line stops singing, in milliseconds, when the provider timed it.
    pub end_ms: Option<u32>,
    /// The words themselves, empty when the provider sent a blank line.
    pub words: String,
}

/// A track's lyrics, as the lyrics pane draws them.
///
/// Empty [`Self::lines`] is the same word as a provider that has none: the pane
/// says so either way.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrackLyrics {
    /// Every line, in the provider's own order, never sorted.
    pub lines: Vec<LyricLine>,
    /// `true` when the provider timed the lines, so one of them can be lit.
    pub synced: bool,
}

/// A playlist's revision, as the wire carries it.
///
/// Opaque bytes that identify one version of a playlist. It is never printed:
/// its `Debug` says only that it is one, so a revision cannot reach a log line.
#[derive(Clone, PartialEq, Eq)]
pub struct Revision(Vec<u8>);

impl Revision {
    /// Reads a revision from the wire, rejecting an empty one.
    ///
    /// An empty revision is no revision at all, and a write based on it would
    /// name no version of the list.
    pub fn from_wire(bytes: &[u8]) -> Option<Self> {
        (!bytes.is_empty()).then(|| Self(bytes.to_vec()))
    }

    /// The revision's bytes, as a write sends them back.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for Revision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Revision(..)")
    }
}

/// One `MOV` operation: `length` rows lifted out of `from` and inserted before
/// whatever the list held at `to` before the move, so `to` is not a row index
/// after the lift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Move {
    pub from: usize,
    pub length: usize,
    pub to: usize,
}

/// One `REM` operation: `length` rows dropped at `from`.
///
/// A chain is sent highest `from` first, so each index reads against the list
/// the removal before it left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Removal {
    pub from: usize,
    pub length: usize,
}

/// What a playlist page may change, and the version it was read at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistAccess {
    pub revision: Revision,
    pub editable: bool,
}

#[cfg(test)]
mod tests;
