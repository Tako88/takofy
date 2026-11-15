//! The day's seed pool: what each of the three rows may be seeded from.
//!
//! Pure, and drawn from the lists the page already holds. A kind's candidates are
//! its newest [`EDGE`], which are always there, and a window of [`EDGE`] that the
//! day moves one step through everything behind them: the ends alone meant a
//! library's middle could never seed a row, and ninety-eight saved albums offered
//! the same twenty-four every day. The day also decides the order the kept ones
//! are tried in. The same day and the same lists always give the same pool, so a
//! page left open does not reshuffle under the user.

use std::collections::HashSet;

use takofy_model::home::{Day, DiscoveryArtist, DiscoveryKind, DiscoverySeed, VARIOUS_ARTISTS};
use takofy_model::{FollowedArtist, PlaylistEntry, SavedAlbum};

use crate::shuffle;

/// How many of a kind's newest items are always candidates, and how wide the
/// day's window through the rest of the list is.
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

/// One kind's kept candidates: its candidate set in the day's drawn order.
fn drawn(seeds: Vec<DiscoverySeed>, kind: DiscoveryKind, day: Day) -> Vec<DiscoverySeed> {
    let candidates = candidates(seeds, day);
    shuffle::order(candidates.len(), mix(day, kind.index()))
        .into_iter()
        .filter_map(|at| candidates.get(at).cloned())
        .take(PER_KIND)
        .collect()
}

/// The day's candidate set: a kind's newest [`EDGE`], then a window of [`EDGE`]
/// that the day moves one step further through everything behind them.
///
/// The band's cost is the pool it walks, not the size of this set, so a large
/// library may compete with more of itself than a small one without spending a
/// request on it. What it must not do is cut off the middle, which is where most
/// of a big library lives — and the window reaches all of it within a few days,
/// taking each item in turn rather than leaving which items by chance.
fn candidates(seeds: Vec<DiscoverySeed>, day: Day) -> Vec<DiscoverySeed> {
    if seeds.len() <= 2 * EDGE {
        return seeds;
    }
    let (newest, rest) = seeds.split_at(EDGE);
    let steps = rest.len().div_ceil(EDGE);
    let step = day.days().rem_euclid(steps as i32) as usize;
    let mut candidates = newest.to_vec();
    candidates.extend((0..EDGE).map(|at| rest[(step * EDGE + at) % rest.len()].clone()));
    candidates
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
