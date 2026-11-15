//! The day's seed pool: what each of the three rows may be seeded from.
//!
//! Pure, and drawn from the lists the page already holds. A kind's candidates
//! are the two ends of its usable items — the newest few and the oldest few —
//! so a large library costs no more than a small one, and the day decides the
//! order the four kept ones are tried in. The same day and the same lists always
//! give the same pool, so a page left open does not reshuffle under the user.

use std::collections::HashSet;

use takofy_model::home::{Day, DiscoveryArtist, DiscoveryKind, DiscoverySeed, VARIOUS_ARTISTS};
use takofy_model::{FollowedArtist, PlaylistEntry, SavedAlbum};

use crate::shuffle;

/// How many items at each end of a kind's usable list are candidates.
pub const EDGE: usize = 12;

/// How many candidates a kind keeps from the drawn edge set.
///
/// A row's pool is what the `↻` walks, so its size is how many times a day one
/// row can be re-seeded before it runs out. Twelve spends the whole edge set of a
/// small library and half of a large one's, which is what a person actually does
/// with a band they are digging through: eight ran out in a sitting.
pub const PER_KIND: usize = 12;

/// The day's pool, one list per row in the page's kind order.
pub fn day_pool(
    playlists: &[PlaylistEntry],
    artists: &[FollowedArtist],
    albums: &[SavedAlbum],
    day: Day,
) -> [Vec<DiscoverySeed>; 3] {
    [
        drawn(playlist_seeds(playlists), DiscoveryKind::Playlist, day),
        drawn(artist_seeds(artists), DiscoveryKind::Artist, day),
        drawn(album_seeds(albums), DiscoveryKind::Album, day),
    ]
}

/// The playlists a row may be seeded from, first of a repeated URI winning.
fn playlist_seeds(playlists: &[PlaylistEntry]) -> Vec<DiscoverySeed> {
    let mut seen: HashSet<&str> = HashSet::with_capacity(playlists.len());
    let mut seeds = Vec::with_capacity(playlists.len());
    for entry in playlists {
        if !entry.named || entry.tracks == Some(0) || !seen.insert(entry.uri.as_str()) {
            continue;
        }
        seeds.push(DiscoverySeed {
            kind: DiscoveryKind::Playlist,
            uri: entry.uri.clone(),
            name: entry.name.clone(),
            anchor: None,
        });
    }
    seeds
}

/// The followed artists a row may be seeded from, first of a repeated URI
/// winning.
fn artist_seeds(artists: &[FollowedArtist]) -> Vec<DiscoverySeed> {
    let mut seen: HashSet<&str> = HashSet::with_capacity(artists.len());
    let mut seeds = Vec::with_capacity(artists.len());
    for entry in artists {
        if !entry.named || !seen.insert(entry.uri.as_str()) {
            continue;
        }
        seeds.push(DiscoverySeed {
            kind: DiscoveryKind::Artist,
            uri: entry.uri.clone(),
            name: entry.name.clone(),
            anchor: Some(DiscoveryArtist {
                uri: entry.uri.clone(),
                name: entry.name.clone(),
            }),
        });
    }
    seeds
}

/// The saved albums a row may be seeded from, one per first credited artist so
/// a shelf of albums by one artist offers that artist once.
fn album_seeds(albums: &[SavedAlbum]) -> Vec<DiscoverySeed> {
    let mut seen: HashSet<&str> = HashSet::with_capacity(albums.len());
    let mut seeds = Vec::with_capacity(albums.len());
    for entry in albums {
        if !entry.named {
            continue;
        }
        let Some(uri) = entry.artist_uris.first() else {
            continue;
        };
        let Some(name) = entry.artist_names.first() else {
            continue;
        };
        if name == VARIOUS_ARTISTS || !seen.insert(uri.as_str()) {
            continue;
        }
        seeds.push(DiscoverySeed {
            kind: DiscoveryKind::Album,
            uri: entry.uri.clone(),
            name: entry.name.clone(),
            anchor: Some(DiscoveryArtist {
                uri: uri.clone(),
                name: name.clone(),
            }),
        });
    }
    seeds
}

/// One kind's kept candidates: its edge set in the day's drawn order.
fn drawn(seeds: Vec<DiscoverySeed>, kind: DiscoveryKind, day: Day) -> Vec<DiscoverySeed> {
    let edges: Vec<DiscoverySeed> = if seeds.len() <= 2 * EDGE {
        seeds
    } else {
        seeds[..EDGE]
            .iter()
            .chain(seeds[seeds.len() - EDGE..].iter())
            .cloned()
            .collect()
    };
    shuffle::order(edges.len(), mix(day, kind.index()))
        .into_iter()
        .filter_map(|at| edges.get(at).cloned())
        .take(PER_KIND)
        .collect()
}

/// The shuffle seed for one day's one row.
fn mix(day: Day, kind: usize) -> u64 {
    let mut state = day.days() as u64;
    let day = shuffle::splitmix(&mut state);
    let mut state = day.wrapping_add(kind as u64);
    shuffle::splitmix(&mut state)
}

#[cfg(test)]
mod tests;
