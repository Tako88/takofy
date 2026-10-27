//! An album as its page draws it.
//!
//! Pure: a parsed [`Album`] and the labels for its track URIs in, an
//! [`AlbumDetail`] out. The request itself is in `remote::read_album`.
//!
//! An album's own order is not a playlist's: `Album::get` answers with discs,
//! each carrying its number and its tracks, so the positions are flattened by
//! disc number and numbered by their place in the result. A multi-disc album is
//! one continuous list, which is what the reference draws.

use librespot::core::SpotifyUri;
use librespot::metadata::Album;
use takofy_model::{AlbumDetail, TrackEntry};

use crate::context::PageTrack;
use crate::cover::covers_in_images;

/// Why an album read failed.
#[derive(Debug, thiserror::Error)]
pub enum AlbumError {
    /// The URI does not name an album, so there is nothing to ask for.
    #[error("not an album URI")]
    NotAnAlbum,
    /// The request failed; the message is the source's.
    #[error("{0}")]
    Source(String),
}

/// The album's playable positions, in its own order.
///
/// The discs are ordered by their own number, so a wire that lists disc two first
/// still reads one first; `sort_by_key` is stable, so discs sharing a number keep
/// the wire's order. Only `SpotifyUri::Track` positions are playable, and one
/// whose URI cannot be written is left out rather than guessed at.
///
/// No position carries an added date: an album is a record, not a membership.
pub(crate) fn positions(album: &Album) -> Vec<PageTrack> {
    let mut discs: Vec<_> = album.discs.iter().collect();
    discs.sort_by_key(|disc| disc.number);
    discs
        .into_iter()
        .flat_map(|disc| disc.tracks.iter())
        .filter(|uri| matches!(uri, SpotifyUri::Track { .. }))
        .filter_map(|uri| uri.to_uri().ok())
        .map(|uri| PageTrack {
            uri,
            added_at: None,
        })
        .collect()
}

/// The album's page data, from the album and its labelled tracks.
///
/// The name falls back to the URI exactly as a playlist's does, and a blank
/// artist is skipped rather than drawn as an empty name in the middle of a list.
/// When the first named artist has no usable artist id there is no link, and the
/// joined credit is drawn plain.
pub(crate) fn detail(uri: &str, album: &Album, tracks: Vec<TrackEntry>) -> AlbumDetail {
    let name = if album.name.is_empty() {
        uri.to_owned()
    } else {
        album.name.clone()
    };
    let artists = album
        .artists
        .iter()
        .map(|artist| artist.name.trim())
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    let artist_uri = album
        .artists
        .iter()
        .find(|artist| !artist.name.trim().is_empty())
        .filter(|artist| matches!(artist.id, SpotifyUri::Artist { .. }))
        .and_then(|artist| artist.id.to_uri().ok());
    AlbumDetail {
        uri: uri.to_owned(),
        name,
        artists,
        artist_uri,
        year: year_of(album),
        cover: covers_in_images(&album.cover_group),
        tracks,
    }
}

/// The album's release year, or `None` when it has none worth drawing.
///
/// A default date message reads as year zero, which is the wire saying nothing
/// rather than a record from year 1 BC. A negative year is representable in
/// `time::Date` and is refused here for the same reason. The year is a `u16` in
/// the model, so the calendar's own ceiling — `time::Date` rejects anything past
/// 9999 — is the only upper bound that can reach this.
fn year_of(album: &Album) -> Option<u16> {
    u16::try_from(album.date.year())
        .ok()
        .filter(|year| *year != 0)
}

#[cfg(test)]
mod tests;
