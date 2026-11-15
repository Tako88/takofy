//! Discovery's cards, from the answers the card path's batched reads carry.
//!
//! Pure: the messages one batched `ARTIST_V4` and one batched `ALBUM_V4` answer
//! carry, and the labels for the artists' top tracks, in, a [`DiscoveryCard`]
//! out. The requests themselves are in `remote::read_discovery_cards`.
//!
//! Nothing here reads an artist one at a time. One batched artist answer carries
//! a name, a portrait group and one playable top track per artist; one batched
//! album answer carries the dates the artist payload's own album stubs leave
//! out.

use std::collections::HashMap;

use librespot::metadata::Artist;
use librespot::protocol::metadata::Album as AlbumMessage;
use librespot::protocol::metadata::Artist as ArtistMessage;
use protobuf::Message as _;
use takofy_model::home::{CARD_TRACKS, DiscoveryArtist, DiscoveryCard};
use takofy_model::{Release, TrackEntry};

use crate::artist::{portrait_of, release_of, release_uris, top_track_positions};

/// Why a discovery read did not answer.
///
/// A read that failed and a read that answered nothing are different answers: a
/// dead session must never look like a seed the service rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadFailed;

/// The artist one encoded `ARTIST_V4` answer carries.
///
/// Shaped as the seam `remote::request_parsed` takes. The echoed URI is not read:
/// an artist message names itself, and the country the card filters its top list
/// by is applied by [`assemble`], which the request does not know.
pub(crate) fn artist_from(bytes: &[u8], _echoed: &str) -> Option<Artist> {
    let message = ArtistMessage::parse_from_bytes(bytes).ok()?;
    Artist::try_from(&message).ok()
}

/// The release one encoded `ALBUM_V4` answer carries.
///
/// The echoed URI names the release, so an answer whose own gid is unusable
/// still carries the date and the cover it sent.
pub(crate) fn album_from(bytes: &[u8], echoed: &str) -> Option<Release> {
    let album = AlbumMessage::parse_from_bytes(bytes).ok()?;
    Some(release_of(echoed, &album))
}

/// The artist's own releases, from what the album batch answered.
///
/// Only the groups that are the artist's own discography are walked, so an album
/// the artist merely appears on is never dated or counted. A release nobody
/// answered is left out rather than dated from its neighbours.
pub(crate) fn own_releases(artist: &Artist, albums: &HashMap<String, Release>) -> Vec<Release> {
    release_uris(artist)
        .into_iter()
        .filter_map(|uri| albums.get(&uri).cloned())
        .collect()
}

/// The newest release year among `releases`, or `None` when none is dated.
///
/// A wire year of zero is the field saying nothing rather than a record, and an
/// undated release says the same, so both are skipped.
pub(crate) fn newest_year(releases: &[Release]) -> Option<u16> {
    releases
        .iter()
        .filter_map(|release| release.year)
        .filter(|year| *year != 0)
        .max()
}

/// One card, from the artist a batched answer carried and the labels for its top
/// tracks.
///
/// `years` is the newest release year per artist URI, from [`newest_year`], so a
/// card whose artist had no dated release draws no year. The card carries up to
/// [`CARD_TRACKS`] playable top positions; a position the labels did not reach is
/// kept as [`TrackEntry::unnamed`] so a playable position is never lost. A card
/// with no playable first position is `None`, because a recommendation that
/// cannot be played is not drawn.
pub(crate) fn assemble(
    uri: &str,
    artist: &Artist,
    country: &str,
    years: &HashMap<String, u16>,
    labelled: &HashMap<String, TrackEntry>,
) -> Option<DiscoveryCard> {
    let positions = top_track_positions(artist, country);
    if positions.is_empty() {
        return None;
    }
    let tracks = positions
        .into_iter()
        .take(CARD_TRACKS)
        .map(|position| {
            labelled
                .get(&position.uri)
                .cloned()
                .unwrap_or_else(|| TrackEntry::unnamed(&position.uri))
        })
        .collect();
    Some(DiscoveryCard {
        artist: DiscoveryArtist {
            uri: uri.to_owned(),
            name: name_of(uri, artist),
        },
        portrait: portrait_of(artist),
        tracks,
        latest_year: years.get(uri).copied(),
    })
}

/// The artist's name, or the URI when the wire sent none: a blank name is the
/// wire saying nothing rather than a name.
fn name_of(uri: &str, artist: &Artist) -> String {
    if artist.name.trim().is_empty() {
        uri.to_owned()
    } else {
        artist.name.clone()
    }
}

/// The artist's top-track titles for `country`, in the wire's own order.
///
/// A position the labels could not name is skipped rather than carried as its
/// URI: these titles are matched against another service's list, and a URI
/// matches nothing there.
pub(crate) fn top_titles_of(
    artist: &Artist,
    country: &str,
    labelled: &HashMap<String, TrackEntry>,
) -> Vec<String> {
    top_track_positions(artist, country)
        .into_iter()
        .filter_map(|position| labelled.get(&position.uri))
        .filter(|entry| entry.named)
        .map(|entry| entry.title.trim().to_owned())
        .filter(|title| !title.is_empty())
        .collect()
}

#[cfg(test)]
mod tests;
