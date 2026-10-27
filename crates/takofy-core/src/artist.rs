//! An artist as its page draws it.
//!
//! Pure: a parsed [`Artist`] and the protocol albums one batched request
//! answered with in, an [`ArtistDetail`] out. The request itself is in
//! `remote::read_artist`.
//!
//! The artist payload names its releases by URI only, so everything a release
//! block says — its name, its kind, its year, its cover, its track list — comes
//! from the batched album metadata, not from the artist. Those answers arrive in
//! no particular order, so they are mapped back by `entity_uri` and then read in
//! the order the release list asked for.

use std::collections::HashMap;

use librespot::core::{SpotifyId, SpotifyUri};
use librespot::metadata::Artist;
use librespot::protocol::extended_metadata::BatchedExtensionResponse;
use librespot::protocol::metadata::Album as AlbumMessage;
use librespot::protocol::metadata::album::Type as AlbumType;
use protobuf::Message as _;
use takofy_model::{ArtistDetail, CoverSet, Release, ReleaseKind};

use crate::context::PageTrack;
use crate::cover::{covers_in_group, covers_in_images};

/// Why an artist read failed.
#[derive(Debug, thiserror::Error)]
pub enum ArtistError {
    /// The URI does not name an artist, so there is nothing to ask for.
    #[error("not an artist URI")]
    NotAnArtist,
    /// The request failed; the message is the source's.
    #[error("{0}")]
    Source(String),
}

/// How many rows the Popular list draws.
///
/// The wire's top list is already ten entries, but a payload that carries more
/// is cut here: the reference's Popular list is a preview, not the discography.
pub(crate) const TOP_TRACKS: usize = 10;

/// How many related artists one page shows.
///
/// Each URI costs a full artist read, and the row is a preview rather than a
/// directory, so a payload that names more is cut here.
pub(crate) const RELATED_MAX: usize = 12;

/// How many release URIs one `ALBUM_V4` request asks for.
///
/// An artist payload can name a few hundred releases, and one request over all
/// of them can answer only part of the set. Fifty keeps each request inside what
/// the provider answers in full, so a short answer costs its own chunk rather
/// than the discography.
pub(crate) const RELEASE_CHUNK: usize = 50;

/// The release URIs split into the requests one read sends.
pub(crate) fn release_chunks(order: &[String]) -> impl Iterator<Item = &[String]> + '_ {
    order.chunks(RELEASE_CHUNK)
}

/// Every release URI the payload names, in the order the page draws them.
///
/// Albums, then singles, then compilations, each group's current variant only,
/// so a re-release does not appear beside the release it replaces. `appears_on`
/// is not the artist's own discography and is left out. A URI that turns up in
/// two groups is kept where it was first seen.
pub(crate) fn release_uris(artist: &Artist) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    artist
        .albums_current()
        .chain(artist.singles_current())
        .chain(artist.compilations_current())
        .filter_map(|uri| uri.to_uri().ok())
        .filter(|uri| seen.insert(uri.clone()))
        .collect()
}

/// Every related artist URI the payload names, in the order the page shows them.
///
/// Deduped by first sight, the artist's own URI dropped, and stopped at
/// [`RELATED_MAX`]. An entry that does not name an artist, or whose id does not
/// resolve to a URI, is dropped: a card that cannot open an artist is not one.
pub(crate) fn related_uris(artist: &Artist) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    artist
        .related
        .iter()
        .filter(|related| related.id != artist.id)
        .filter(|related| matches!(related.id, SpotifyUri::Artist { .. }))
        .filter_map(|related| related.id.to_uri().ok())
        .filter(|uri| seen.insert(uri.clone()))
        .take(RELATED_MAX)
        .collect()
}

/// The artist's top tracks for `country`, as playable positions.
///
/// The wire's country entry wins, the empty-country entry is the fallback, and
/// nothing is the answer when neither exists. An artist's own top list is not a
/// membership, so no position carries an added date.
pub(crate) fn top_track_positions(artist: &Artist, country: &str) -> Vec<PageTrack> {
    artist
        .top_tracks
        .for_country(country)
        .iter()
        .filter_map(|uri| match uri {
            SpotifyUri::Track { .. } => uri.to_uri().ok(),
            _ => None,
        })
        .take(TOP_TRACKS)
        .map(|uri| PageTrack {
            uri,
            added_at: None,
        })
        .collect()
}

/// The artist's page data, from the artist and its labelled top tracks.
///
/// The name falls back to the URI exactly as an album's does. A blank name is
/// the wire saying nothing rather than a name.
pub(crate) fn detail(
    uri: &str,
    artist: &Artist,
    top_tracks: Vec<takofy_model::TrackEntry>,
    releases: Vec<Release>,
) -> ArtistDetail {
    let name = if artist.name.trim().is_empty() {
        uri.to_owned()
    } else {
        artist.name.clone()
    };
    ArtistDetail {
        uri: uri.to_owned(),
        name,
        portrait: portrait_of(artist),
        top_tracks,
        releases,
        related: related_uris(artist),
    }
}

/// The portrait the hero draws.
///
/// Only `portrait_group` is read: `portraits` came back empty on every artist
/// the probe asked, and `is_portrait_album_cover` is a hint about one of the
/// group's tiles rather than a second source.
pub(crate) fn portrait_of(artist: &Artist) -> Option<CoverSet> {
    covers_in_images(&artist.portrait_group)
}

/// One release, from the URI the artist named and the album the batched answer
/// carried.
///
/// The name falls back to the URI; the kind is the wire's own, with the kinds
/// the reference has no word for collapsed into [`ReleaseKind::Other`]; the year
/// is refused when it is zero or negative, exactly as the album band does.
pub(crate) fn release_of(uri: &str, album: &AlbumMessage) -> Release {
    Release {
        uri: uri.to_owned(),
        name: album
            .name
            .clone()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| uri.to_owned()),
        kind: kind_of(album.type_()),
        year: u16::try_from(album.date.as_ref().map_or(0, |date| date.year()))
            .ok()
            .filter(|year| *year != 0),
        cover: covers_in_group(album.cover_group.as_ref()),
        tracks: track_uris(album),
    }
}

/// The wire's album type as the block labels it.
fn kind_of(kind: AlbumType) -> ReleaseKind {
    match kind {
        AlbumType::ALBUM => ReleaseKind::Album,
        AlbumType::EP => ReleaseKind::Ep,
        AlbumType::SINGLE => ReleaseKind::Single,
        AlbumType::COMPILATION => ReleaseKind::Compilation,
        AlbumType::AUDIOBOOK | AlbumType::PODCAST => ReleaseKind::Other,
    }
}

/// The release's track URIs, flattened by disc number.
///
/// The discs are ordered by their own number with a stable sort, so a wire that
/// lists disc two first still reads one first, and the positions are numbered by
/// their place in the result. A gid that is not a track id is left out rather
/// than guessed at.
fn track_uris(album: &AlbumMessage) -> Vec<String> {
    let mut discs: Vec<_> = album.disc.iter().collect();
    discs.sort_by_key(|disc| disc.number);
    discs
        .into_iter()
        .flat_map(|disc| disc.track.iter())
        .filter_map(|track| {
            let id = SpotifyId::from_raw(track.gid.as_deref()?).ok()?;
            SpotifyUri::Track { id }.to_uri().ok()
        })
        .collect()
}

/// The albums a batched answer carries, keyed by the URI the answer echoes.
///
/// An entity that does not parse is skipped, so one unreadable album does not
/// cost the page the others in the same answer.
pub(crate) fn answered_albums(
    response: &BatchedExtensionResponse,
) -> HashMap<String, AlbumMessage> {
    let mut answered: HashMap<String, AlbumMessage> = HashMap::new();
    for array in &response.extended_metadata {
        for data in &array.extension_data {
            let Some(any) = data.extension_data.as_ref() else {
                continue;
            };
            let Ok(album) = AlbumMessage::parse_from_bytes(&any.value) else {
                continue;
            };
            answered.insert(data.entity_uri.clone(), album);
        }
    }
    answered
}

/// The releases the batched answer carries, in the request's own order.
///
/// The answer's order is not the request's, so each entity is keyed by the URI
/// the answer echoes and the request's order is walked. An entity that does not
/// parse, and a URI nobody answered, are both skipped: one bad album must not
/// cost the page the other forty-seven.
pub(crate) fn releases_from(response: &BatchedExtensionResponse, order: &[String]) -> Vec<Release> {
    let mut answered = answered_albums(response);
    order
        .iter()
        .filter_map(|uri| answered.remove(uri).map(|album| release_of(uri, &album)))
        .collect()
}

/// The releases a chunked read answered, in the request's own order.
///
/// Each answer is mapped against its own chunk, so a chunk that failed or came
/// back short costs only its releases rather than the whole discography.
pub(crate) fn releases_of(
    order: &[String],
    answers: Vec<Option<BatchedExtensionResponse>>,
) -> Vec<Release> {
    release_chunks(order)
        .zip(answers)
        .flat_map(|(chunk, answer)| match answer {
            Some(response) => releases_from(&response, chunk),
            None => Vec::new(),
        })
        .collect()
}

#[cfg(test)]
mod tests;
