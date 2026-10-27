//! The collection set's items, as the saved-artists and saved-albums lists draw them.
//!
//! Pure: the collection's paging items and the album metadata one batched read
//! answered with in, the ordered lists out. The request itself is not here.
//!
//! The set holds liked songs and saved albums together, so an order keeps only
//! the kind it names: an artist order keeps artist items, an album order keeps
//! album items. Removed items are dropped first, so a save undone and redone is
//! read once; among the remaining occurrences the first wire occurrence wins,
//! and items without a date come last, exactly as the followed shows' order.

use std::collections::HashMap;

use librespot::core::SpotifyUri;
use librespot::metadata::Artist;
use librespot::protocol::extended_metadata::BatchedExtensionResponse;
use librespot::protocol::metadata::Album as AlbumMessage;
use takofy_model::{CoverSet, FollowedArtist, SavedAlbum};

use crate::artist::{answered_albums, portrait_of, release_chunks, release_of, release_uris};
use crate::collection::Item;
use crate::followed::newest_first;

/// The saved artists' URIs, newest save first.
pub(crate) fn artist_order(items: Vec<Item>) -> Vec<String> {
    newest_first(items, |uri| matches!(uri, SpotifyUri::Artist { .. }))
}

/// The saved albums' URIs, newest save first.
///
/// Liked songs, episodes and everything else the set carries are dropped, so
/// only album items are kept.
pub(crate) fn album_order(items: Vec<Item>) -> Vec<String> {
    newest_first(items, |uri| matches!(uri, SpotifyUri::Album { .. }))
}

/// The liked tracks' URIs, newest save first.
///
/// The same set holds saved albums and episodes, so only track items are kept.
pub(crate) fn track_order(items: Vec<Item>) -> Vec<String> {
    newest_first(items, |uri| matches!(uri, SpotifyUri::Track { .. }))
}

/// An artist as its card draws it, before it is joined to the saved order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistCard {
    pub name: String,
    pub portrait: Option<CoverSet>,
    pub releases: u32,
}

/// The card the artist's payload answers with, when it names an artist.
///
/// `None` for a blank name: a card that cannot say who it is would draw a list
/// row the artist page cannot open. The count is the payload's own deduped
/// current releases.
pub(crate) fn artist_card(artist: &Artist) -> Option<ArtistCard> {
    if artist.name.trim().is_empty() {
        return None;
    }
    let releases = u32::try_from(release_uris(artist).len()).ok()?;
    Some(ArtistCard {
        name: artist.name.clone(),
        portrait: portrait_of(artist),
        releases,
    })
}

/// The saved artists the grid draws, in the order given.
///
/// An artist whose card is missing falls back to its URI and reads as unnamed
/// with no release count, so its card still opens the artist.
pub(crate) fn artist_entries(
    order: &[String],
    cards: &HashMap<String, ArtistCard>,
) -> Vec<FollowedArtist> {
    order
        .iter()
        .map(|uri| {
            let card = cards.get(uri);
            FollowedArtist {
                uri: uri.clone(),
                name: card.map_or_else(|| uri.clone(), |card| card.name.clone()),
                named: card.is_some(),
                portrait: card.and_then(|card| card.portrait.clone()),
                releases: card.map(|card| card.releases),
            }
        })
        .collect()
}

/// The saved albums the grid draws, in the order given.
///
/// Each chunk's answer is mapped against its own URIs, so a chunk that failed
/// or came back short costs only its albums: its URIs stay in the list, unnamed
/// and uncounted, while an answered album reads its name, its artists, its
/// year, its cover and its track count from the wire's message. A blank wire
/// name is unnamed, and the track count is the parseable track gids the wire's
/// discs carried: an album with no tracks, or whose discs failed to parse,
/// reads zero while an unanswered one reads none.
pub(crate) fn saved_album_entries(
    order: &[String],
    answers: Vec<Option<BatchedExtensionResponse>>,
) -> Vec<SavedAlbum> {
    let mut out = Vec::new();
    for (chunk, answer) in
        release_chunks(order).zip(answers.into_iter().chain(std::iter::repeat_with(|| None)))
    {
        let answered = answer.as_ref().map(answered_albums);
        for uri in chunk {
            match answered.as_ref().and_then(|albums| albums.get(uri)) {
                Some(album) => {
                    let release = release_of(uri, album);
                    out.push(SavedAlbum {
                        uri: uri.clone(),
                        name: release.name,
                        named: album.name.as_deref().is_some_and(|name| !name.is_empty()),
                        artists: join_artist_names(album),
                        year: release.year,
                        cover: release.cover,
                        tracks: u32::try_from(release.tracks.len()).ok(),
                    });
                }
                None => out.push(SavedAlbum {
                    uri: uri.clone(),
                    name: uri.clone(),
                    named: false,
                    artists: String::new(),
                    year: None,
                    cover: None,
                    tracks: None,
                }),
            }
        }
    }
    out
}

/// The album's artists, the album page's own rule: each name trimmed, blanks
/// dropped, the rest joined with ", ". An album that named no artists joins to
/// the empty string rather than an invention.
fn join_artist_names(album: &AlbumMessage) -> String {
    album
        .artist
        .iter()
        .filter_map(|artist| {
            artist
                .name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests;
