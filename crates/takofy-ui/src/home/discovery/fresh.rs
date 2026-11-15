//! What a row keeps of the neighbours it found: the artists the library does
//! not already have, and the user has not hidden.
//!
//! Owned means followed, or credited on a saved album. That is narrower than the
//! library: a liked song carries no artist link, and a playlist carries none
//! either, so an artist known only through one of those can still be offered.
//!
//! Hidden is a permanent per-artist set and never a "seen" list, so unfollowing
//! an artist makes it eligible again and only hiding it keeps it out.

use std::collections::HashSet;

use takofy_model::home::{DiscoveryArtist, DiscoveryCard};
use takofy_model::{FollowedArtist, SavedAlbum};

use super::hidden::Hidden;

/// The artists the library already holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Owned {
    artists: HashSet<String>,
}

impl Owned {
    /// Every followed artist's URI, plus every artist credited on a saved album.
    pub fn new(followed: &[FollowedArtist], saved: &[SavedAlbum]) -> Self {
        let mut artists = HashSet::with_capacity(followed.len());
        for artist in followed {
            artists.insert(artist.uri.clone());
        }
        for album in saved {
            artists.extend(album.artist_uris.iter().cloned());
        }
        Self { artists }
    }

    /// Whether the library already holds `artist`.
    pub fn contains(&self, artist: &str) -> bool {
        self.artists.contains(artist)
    }
}

/// The cards a row may draw, in the order the lookup found them.
pub fn fresh<'a>(
    cards: &'a [DiscoveryCard],
    anchor: Option<&DiscoveryArtist>,
    owned: &Owned,
    hidden: &Hidden,
) -> Vec<&'a DiscoveryCard> {
    cards
        .iter()
        .filter(|card| {
            !owned.contains(&card.artist.uri)
                && anchor.is_none_or(|anchor| anchor.uri != card.artist.uri)
                && !hidden.contains(&card.artist.uri)
        })
        .collect()
}

#[cfg(test)]
mod tests;
