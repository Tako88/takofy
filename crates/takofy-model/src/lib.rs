//! Plain data shared by the engine and the window.
//!
//! Both `takofy-core` and `takofy-ui` depend on this crate, and it depends on
//! nothing. That is the point: the window can hold what it draws without
//! depending on the crate that talks to Spotify, so "a view cannot reach the
//! network" stays enforced by the compiler.

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
/// The url is never absent: an origin with nothing to name cannot be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistCover {
    pub url: CoverUrl,
    pub origin: CoverOrigin,
}

impl PlaylistCover {
    /// A cover a user set.
    pub fn chosen(url: CoverUrl) -> Self {
        Self {
            url,
            origin: CoverOrigin::Chosen,
        }
    }

    /// A cover copied from the playlist's first track.
    pub fn derived(url: CoverUrl) -> Self {
        Self {
            url,
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

/// A playlist as listed in the rootlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistEntry {
    pub uri: String,
    pub name: String,
    /// `false` when `name` is the URI fallback.
    pub named: bool,
    /// The playlist's cover, once one of the three sources names it.
    pub cover: Option<PlaylistCover>,
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
}

impl PlaylistEntry {
    /// The playlist's cover image url, whatever its origin.
    pub fn cover_url(&self) -> Option<&CoverUrl> {
        self.cover.as_ref().map(|cover| &cover.url)
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
/// A separate type from a duration, which is also a millisecond count, so the
/// two can never be passed for each other.
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
    pub cover: Option<CoverUrl>,
}

impl TrackEntry {
    /// Whether this entry is a track a playlist can hold.
    ///
    /// An episode and a local file are rows too, but neither may be added to a
    /// playlist, so the URI kind decides rather than the row's shape.
    pub fn is_track(&self) -> bool {
        self.uri.starts_with("spotify:track:")
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
    pub cover: Option<CoverUrl>,
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
    /// The release year, or `None` for a wire year of zero or none at all.
    ///
    /// A `time::Date` cannot hold a year past 9999, so a wider integer is not a
    /// case this can see.
    pub year: Option<u16>,
    pub cover: Option<CoverUrl>,
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
    pub cover: Option<CoverUrl>,
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
    pub portrait: Option<CoverUrl>,
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
    pub cover: Option<CoverUrl>,
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
    pub cover: Option<CoverUrl>,
}

/// One artist in the followed-artists list, as its card draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowedArtist {
    pub uri: String,
    /// The artist's name, or the URI when the metadata did not answer.
    pub name: String,
    /// `false` when `name` is the URI fallback.
    pub named: bool,
    pub portrait: Option<CoverUrl>,
    /// How many current releases the artist has, when they could be read.
    pub releases: Option<u32>,
}

/// One artist's portrait, as a search menu's artist row draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistPortrait {
    pub artist: String,
    /// `None` when the artist's record carries no portrait group.
    pub portrait: Option<CoverUrl>,
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
    pub cover: Option<CoverUrl>,
    /// The album's track count, when its discs could be read.
    pub tracks: Option<u32>,
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
mod tests {
    use super::{
        AddedAt, CoverImage, CoverOrigin, CoverUrl, Episode, Move, Owner, OwnerName,
        PlaylistAccess, PlaylistCover, PlaylistEntry, PublishedAt, Removal, Revision, TrackEntry,
        TrackLyrics,
    };

    fn revision(byte: u8) -> Revision {
        Revision::from_wire(&[byte]).expect("a revision")
    }

    fn playlist(tracks: Option<usize>, access: Option<PlaylistAccess>) -> PlaylistEntry {
        PlaylistEntry {
            uri: "spotify:playlist:a".to_owned(),
            name: "A".to_owned(),
            named: true,
            cover: None,
            description: None,
            tracks,
            owner: None,
            access,
        }
    }

    fn access(editable: bool) -> Option<PlaylistAccess> {
        Some(PlaylistAccess {
            revision: revision(1),
            editable,
        })
    }

    fn track(uri: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: "A Title".to_owned(),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            artist_names: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
        }
    }

    #[test]
    fn add_basis_needs_an_editable_revision_and_a_count() {
        assert_eq!(playlist(Some(4), None).add_basis(), None, "no access");
        assert_eq!(
            playlist(Some(4), access(false)).add_basis(),
            None,
            "not editable"
        );
        assert_eq!(playlist(None, access(true)).add_basis(), None, "no count");
        assert_eq!(playlist(None, None).add_basis(), None, "neither");

        assert_eq!(
            playlist(Some(4), access(true)).add_basis(),
            Some((&revision(1), 4))
        );

        // A count of zero is a real count: an append to an empty list names it.
        assert_eq!(
            playlist(Some(0), access(true)).add_basis(),
            Some((&revision(1), 0))
        );
    }

    #[test]
    fn a_chosen_cover_and_a_derived_cover_differ_only_in_origin() {
        let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url");
        assert_eq!(
            PlaylistCover::chosen(url.clone()),
            PlaylistCover {
                url: url.clone(),
                origin: CoverOrigin::Chosen,
            }
        );
        assert_eq!(
            PlaylistCover::derived(url.clone()),
            PlaylistCover {
                url,
                origin: CoverOrigin::Derived,
            }
        );
    }

    #[test]
    fn a_playlist_entry_hands_out_its_cover_url() {
        let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url");
        let mut entry = playlist(Some(1), access(true));
        assert_eq!(entry.cover_url(), None, "no cover named");
        entry.cover = Some(PlaylistCover::derived(url.clone()));
        assert_eq!(entry.cover_url(), Some(&url));
    }

    #[test]
    fn an_episode_or_local_uri_is_not_a_track() {
        for uri in ["spotify:episode:1", "spotify:local:a:b:c:1", "", "list:p0"] {
            assert!(!track(uri).is_track(), "must refuse {uri:?}");
        }
        assert!(track("spotify:track:4uLU6hMCjMI75M1A2tKUQC").is_track());
    }

    #[test]
    fn an_empty_revision_is_none() {
        assert_eq!(Revision::from_wire(&[]), None);
        assert!(
            Revision::from_wire(&[0]).is_some(),
            "a zero byte is still a byte"
        );
    }

    #[test]
    fn a_revision_keeps_its_bytes() {
        let bytes = [0x00, 0x00, 0x00, 0x06, 0xc1, 0x52];
        let revision = Revision::from_wire(&bytes).expect("a revision");
        assert_eq!(revision.as_bytes(), &bytes);
    }

    #[test]
    fn a_revision_never_prints_its_bytes() {
        let secret = format!(
            "{:?}",
            Revision::from_wire(&[0xde, 0xad]).expect("a revision")
        );
        assert_eq!(secret, "Revision(..)");

        let access = PlaylistAccess {
            revision: Revision::from_wire(&[0xde, 0xad]).expect("a revision"),
            editable: true,
        };
        let printed = format!("{access:?}");
        assert!(printed.contains("Revision(..)"), "{printed}");
        assert!(
            !printed.contains("222"),
            "the decimal bytes leaked: {printed}"
        );
    }

    fn episode() -> Episode {
        Episode {
            uri: "spotify:episode:1".to_owned(),
            title: "An episode".to_owned(),
            show: "Lateral with Tom Scott".to_owned(),
            show_uri: Some("spotify:show:abc".to_owned()),
            description: Some("A description".to_owned()),
            published: PublishedAt::from_millis(1_761_134_400_000),
            duration_ms: Some(2_773_461),
            cover: Some(CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url")),
            named: true,
        }
    }

    #[test]
    fn default_lyrics_are_untimed_and_empty() {
        let lyrics = TrackLyrics::default();
        assert!(lyrics.lines.is_empty());
        assert!(!lyrics.synced);
    }

    #[test]
    fn published_at_rejects_zero_and_negative_and_keeps_positive() {
        assert_eq!(PublishedAt::from_millis(0), None);
        assert_eq!(PublishedAt::from_millis(-1), None);
        let published = PublishedAt::from_millis(1_761_134_400_000).expect("a positive instant");
        assert_eq!(published.as_millis(), 1_761_134_400_000);
        assert_eq!(PublishedAt::from_millis(i64::MIN), None);
        let widest = PublishedAt::from_millis(i64::MAX).expect("the widest instant");
        assert_eq!(widest.as_millis(), i64::MAX as u64);
    }

    #[test]
    fn episode_entry_puts_the_show_name_in_artists_and_the_show_uri_in_artist_uris() {
        let entry = episode().entry();
        assert_eq!(entry.artists, "Lateral with Tom Scott");
        assert_eq!(entry.artist_uris, vec!["spotify:show:abc".to_owned()]);
        assert_eq!(
            entry.artist_names,
            vec!["Lateral with Tom Scott".to_owned()]
        );
    }

    #[test]
    fn episode_entry_has_no_artist_uris_when_the_show_name_is_empty() {
        let mut value = episode();
        value.show = String::new();
        assert!(value.entry().artist_uris.is_empty());
        assert!(value.entry().artist_names.is_empty());
    }

    #[test]
    fn episode_entry_keeps_the_show_name_without_artist_uris_when_the_show_uri_is_missing() {
        let mut value = episode();
        value.show_uri = None;
        let entry = value.entry();
        assert_eq!(entry.artists, "Lateral with Tom Scott");
        assert!(entry.artist_uris.is_empty());
        assert!(entry.artist_names.is_empty());
    }

    #[test]
    fn episode_entry_carries_named_duration_and_cover_and_no_album_or_added_at() {
        let entry = episode().entry();
        assert_eq!(entry.uri, "spotify:episode:1");
        assert_eq!(entry.title, "An episode");
        assert_eq!(entry.album, None);
        assert_eq!(entry.album_uri, None);
        assert_eq!(entry.added_at, None);
        assert_eq!(entry.duration_ms, Some(2_773_461));
        assert_eq!(entry.cover, episode().cover);
        assert!(entry.named);

        // An unnamed episode must not come out of the entry named.
        let mut unnamed = episode();
        unnamed.named = false;
        assert!(!unnamed.entry().named);
    }

    #[test]
    fn added_at_from_wire_reads_milliseconds_in_a_string() {
        let added = AddedAt::from_wire("1792357200000").expect("the issue's sample");
        assert_eq!(added.as_millis(), 1_792_357_200_000);
    }

    #[test]
    fn added_at_from_wire_rejects_what_is_not_a_plain_count() {
        for wire in [
            "",
            " 1792357200000",
            "+1792357200000",
            "-1",
            "1.7e12",
            "abc",
            "1792357200000ms",
            "99999999999999999999999",
        ] {
            assert_eq!(AddedAt::from_wire(wire), None, "must reject {wire:?}");
        }
    }

    #[test]
    fn added_at_from_wire_rejects_a_value_in_seconds() {
        assert_eq!(AddedAt::from_wire("1792357200"), None);
    }

    #[test]
    fn added_at_floor_is_the_inclusive_boundary() {
        assert_eq!(AddedAt::from_wire("999999999999"), None);
        assert!(AddedAt::from_wire("1000000000000").is_some());
    }

    #[test]
    fn a_20_byte_file_id_becomes_a_cdn_url() {
        let id: Vec<u8> = (1..=20).collect();
        let url = CoverUrl::from_file_id(&id).expect("a valid id");
        assert_eq!(
            &url.as_str()["https://i.scdn.co/image/".len()..],
            "0102030405060708090a0b0c0d0e0f1011121314"
        );
        assert_eq!(
            url.as_str().len(),
            "https://i.scdn.co/image/".len() + 40,
            "the hex is forty characters"
        );
    }

    #[test]
    fn a_file_id_containing_a_zero_byte_is_still_a_file_id() {
        // Only an all-zero id is padding; a real id may hold a zero byte.
        let mut id = [1u8; 20];
        id[3] = 0;
        assert!(CoverUrl::from_file_id(&id).is_some());
    }

    #[test]
    fn a_file_id_of_the_wrong_length_is_rejected() {
        assert_eq!(CoverUrl::from_file_id(&[]), None);
        assert_eq!(CoverUrl::from_file_id(&[1; 16]), None);
        assert_eq!(CoverUrl::from_file_id(&[1; 21]), None);
    }

    #[test]
    fn an_all_zero_file_id_is_rejected() {
        assert_eq!(CoverUrl::from_file_id(&[0; 20]), None);
    }

    #[test]
    fn from_wire_accepts_the_known_hosts() {
        for url in [
            "https://i.scdn.co/image/abc",
            "https://pickasso.spotifycdn.com/image/ab67c0de",
        ] {
            assert_eq!(
                CoverUrl::from_wire(url).map(|cover| cover.as_str().to_owned()),
                Some(url.to_owned())
            );
        }
    }

    #[test]
    fn from_wire_rejects_everything_else() {
        for url in [
            "http://i.scdn.co/image/abc",
            "/image/abc",
            "https://i.scdn.co.example.com/image/abc",
            "https://i.scdn.co@evil.example/image/abc",
            "https://evil.example\\@i.scdn.co/x",
            "https://i.scdn.co\\@evil.example/x",
            "https://i.scdn.co\t.evil/x",
            "https://evil.example/i.scdn.co.png",
            "https://i.scdn.co",
        ] {
            assert_eq!(CoverUrl::from_wire(url), None, "must reject {url}");
        }
    }

    #[test]
    fn cover_image_requires_a_positive_shape() {
        assert!(CoverImage::new(2, 2, vec![0; 16]).is_some());
        assert_eq!(CoverImage::new(0, 2, vec![0; 0]), None);
        assert_eq!(CoverImage::new(2, 0, vec![0; 0]), None);
    }

    #[test]
    fn cover_image_rejects_a_mismatched_buffer() {
        assert_eq!(CoverImage::new(2, 2, vec![0; 15]), None);
        assert_eq!(CoverImage::new(2, 2, vec![0; 17]), None);
    }

    #[test]
    fn a_move_is_a_value_the_whole_write_can_share() {
        let mv = Move {
            from: 1,
            length: 3,
            to: 6,
        };
        let copy = mv;
        assert_eq!(copy, mv);
        assert_ne!(mv, Move { length: 1, ..mv });
    }

    #[test]
    fn a_removal_is_a_value_the_chain_can_share() {
        let removal = Removal { from: 5, length: 2 };
        let copy = removal;
        assert_eq!(copy, removal);
        assert_ne!(removal, Removal { from: 4, ..removal });
    }

    #[test]
    fn owner_name_rejects_blank_and_whitespace() {
        assert_eq!(OwnerName::from_wire(""), None);
        assert_eq!(OwnerName::from_wire("   "), None);
        assert_eq!(OwnerName::from_wire(" \n\t "), None);
    }

    #[test]
    fn owner_name_rejects_a_bare_number() {
        assert_eq!(OwnerName::from_wire("31"), None);
        assert_eq!(OwnerName::from_wire("31415926535"), None);
        assert_eq!(OwnerName::from_wire("  42  "), None);
    }

    #[test]
    fn owner_name_keeps_a_name_with_digits_and_letters() {
        assert!(OwnerName::from_wire("31l6").is_some());
        assert!(OwnerName::from_wire("alice123").is_some());
        assert!(OwnerName::from_wire("Vela Moreno").is_some());
    }

    #[test]
    fn owner_name_is_trimmed() {
        let name = OwnerName::from_wire("  Vela Moreno \n").expect("a name");
        assert_eq!(name.as_str(), "Vela Moreno");
    }

    #[test]
    fn owner_name_reads_back() {
        let name = OwnerName::from_wire("Vela Moreno").expect("a name");
        assert_eq!(name.as_str(), "Vela Moreno");
    }

    #[test]
    fn owner_name_method_is_none_for_you() {
        assert_eq!(Owner::You.name(), None);
        let named = Owner::Named(OwnerName::from_wire("Vela Moreno").expect("a name"));
        assert_eq!(named.name(), Some("Vela Moreno"));
    }
}
