//! What a Spotify address names, decided in one place.
//!
//! An address is a kind segment and an id, and the id is whatever follows the
//! kind: `spotify:local:` spells the artist, album and title into it, and only
//! the code that acts on a local file splits them apart again. A caller that
//! needs an id without colons says so itself, because an address carrying them
//! still names the thing it names.

/// The kind of thing a Spotify address names.
///
/// Only the kinds this app draws: an address naming anything else is not a
/// [`Uri`], rather than a kind standing in for "other".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UriKind {
    Track,
    Episode,
    Local,
    Playlist,
    Album,
    Artist,
    Show,
}

/// A Spotify address split into the kind it names and the id it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Uri<'a> {
    pub kind: UriKind,
    /// Everything after the kind segment, colons and all.
    pub id: &'a str,
    /// The account name when the address is the older `spotify:user:<name>:playlist:<id>` shape.
    pub owner: Option<&'a str>,
}

/// Reads an address, or `None` when it names nothing this app draws.
///
/// A kind with an empty id is not an address: `spotify:track:` names no track.
pub fn parse(uri: &str) -> Option<Uri<'_>> {
    let rest = uri.strip_prefix("spotify:")?;

    // The account-named playlist is the one shape whose kind is not its second
    // segment. Every other `user:` address names no kind here and fails below.
    if let Some(named) = rest.strip_prefix("user:")
        && let Some((owner, tail)) = named.split_once(':')
        && let Some(id) = tail.strip_prefix("playlist:")
        && !owner.is_empty()
        && !id.is_empty()
    {
        return Some(Uri {
            kind: UriKind::Playlist,
            id,
            owner: Some(owner),
        });
    }

    let (kind, id) = rest.split_once(':')?;
    if id.is_empty() {
        return None;
    }
    Some(Uri {
        kind: kind_from_segment(kind)?,
        id,
        owner: None,
    })
}

/// The kind a segment names, or `None` for one this app does not draw.
fn kind_from_segment(segment: &str) -> Option<UriKind> {
    match segment {
        "track" => Some(UriKind::Track),
        "episode" => Some(UriKind::Episode),
        "local" => Some(UriKind::Local),
        "playlist" => Some(UriKind::Playlist),
        "album" => Some(UriKind::Album),
        "artist" => Some(UriKind::Artist),
        "show" => Some(UriKind::Show),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{UriKind, parse};

    fn raw(uri: &str) -> Option<(UriKind, &str, Option<&str>)> {
        parse(uri).map(|parsed| (parsed.kind, parsed.id, parsed.owner))
    }

    #[test]
    fn every_kind_parses_with_the_remainder_as_its_id() {
        assert_eq!(
            raw("spotify:track:4uLU6hMCjMI75M1A2tKUQC"),
            Some((UriKind::Track, "4uLU6hMCjMI75M1A2tKUQC", None))
        );
        assert_eq!(
            raw("spotify:episode:1"),
            Some((UriKind::Episode, "1", None))
        );
        assert_eq!(
            raw("spotify:playlist:abc"),
            Some((UriKind::Playlist, "abc", None))
        );
        assert_eq!(
            raw("spotify:album:abc"),
            Some((UriKind::Album, "abc", None))
        );
        assert_eq!(
            raw("spotify:artist:abc"),
            Some((UriKind::Artist, "abc", None))
        );
        assert_eq!(raw("spotify:show:abc"), Some((UriKind::Show, "abc", None)));
    }

    #[test]
    fn a_colon_in_the_id_is_part_of_the_id() {
        assert_eq!(
            raw("spotify:playlist:abc:extra"),
            Some((UriKind::Playlist, "abc:extra", None))
        );
        assert_eq!(
            raw("spotify:local:Artist:Album:Title:222"),
            Some((UriKind::Local, "Artist:Album:Title:222", None))
        );
    }

    #[test]
    fn an_account_named_playlist_keeps_the_name_and_the_id() {
        assert_eq!(
            raw("spotify:user:alice:playlist:abc"),
            Some((UriKind::Playlist, "abc", Some("alice")))
        );
        assert_eq!(
            raw("spotify:user:alice:playlist:abc:extra"),
            Some((UriKind::Playlist, "abc:extra", Some("alice")))
        );
    }

    #[test]
    fn a_kind_with_no_id_is_not_an_address() {
        for uri in [
            "spotify:track:",
            "spotify:playlist:",
            "spotify:user:alice:playlist:",
            "spotify:playlist",
        ] {
            assert_eq!(raw(uri), None, "must refuse {uri:?}");
        }
    }

    #[test]
    fn an_address_naming_no_drawn_kind_is_not_an_address() {
        for uri in [
            "spotify:user:alice:collection",
            "spotify:user:alice:folder:x",
            "spotify:user:alice:folder:playlist:abc",
            "spotify:user::playlist:abc",
            "spotify:user:",
            "spotify:",
            "spotify:track_extra:abc",
        ] {
            assert_eq!(raw(uri), None, "must refuse {uri:?}");
        }
    }

    #[test]
    fn a_foreign_scheme_is_not_an_address() {
        for uri in ["", "https://i.scdn.co/image/abc", "list:p0", "spotify"] {
            assert_eq!(raw(uri), None, "must refuse {uri:?}");
        }
    }
}
