//! One discovery row's lookup, in the order the outside can answer it.
//!
//! The gate is read before anything else, so an off setting costs no read. The
//! seed's anchor is identified by title overlap rather than by name, because
//! the service returns same-name noise. A neighbour is only accepted when its
//! title and credited artist both match, and the genre read comes after the
//! cards so a missing genre never costs a row its artists. A Spotify read that
//! failed is unreachable, never a seed the service rejected.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::future::Future;

use takofy_core::ReadFailed;
use takofy_model::TrackEntry;
use takofy_model::home::{
    DiscoveryArtist, DiscoveryCard, DiscoveryKind, DiscoveryOutcome, DiscoverySeed, OutsideId,
    RING_CARDS, VARIOUS_ARTISTS,
};

use super::verify::{confirm_hit, normalise, overlaps};
use super::{Fetch, Outside, OutsideError, genre, wire};

/// How often an artist must lead a playlist's first page to anchor it.
const ANCHOR_MIN: usize = 2;
/// How many search candidates may be verified before the row is rejected.
const VERIFY_MAX: usize = 3;
/// How many neighbours may be tried before the row stops looking.
const RESOLVE_MAX: usize = 8;
/// How many neighbours a row's first ring may resolve into cards.
const CARDS_MAX: usize = 6;
/// How many neighbours one dig's ring may try before it stops looking.
const DIG_ATTEMPTS_MAX: usize = 16;
/// How many of the anchor's own titles may be compared.
const SEED_TITLES_MAX: usize = 10;

/// The caps one neighbour walk runs under.
struct Limits {
    cards: usize,
    attempts: usize,
}

/// The first ring's caps.
const FIRST_RING: Limits = Limits {
    cards: CARDS_MAX,
    attempts: RESOLVE_MAX,
};

/// A dig's ring: four cards, and the attempts a whole union may cost.
const RING: Limits = Limits {
    cards: RING_CARDS,
    attempts: DIG_ATTEMPTS_MAX,
};

/// The Spotify reads a lookup needs, faked in tests.
///
/// An `Err` is a read that did not answer, which is never the same as a read
/// that answered nothing: a dead session must not look like a seed the service
/// refused.
pub(crate) trait Spotify: Send + Sync {
    fn seed_titles(
        &self,
        artist: &DiscoveryArtist,
    ) -> impl Future<Output = Result<Vec<String>, ReadFailed>> + Send;
    fn playlist_head(
        &self,
        uri: &str,
    ) -> impl Future<Output = Result<Vec<TrackEntry>, ReadFailed>> + Send;
    fn search(
        &self,
        query: &str,
    ) -> impl Future<Output = Result<Vec<TrackEntry>, ReadFailed>> + Send;
    fn cards(
        &self,
        uris: &[String],
    ) -> impl Future<Output = Result<Vec<DiscoveryCard>, ReadFailed>> + Send;
}

/// Looks one seed up outside and returns the seed with its resolved anchor.
pub(crate) async fn lookup<S, F>(
    spotify: &S,
    outside: &Outside<F>,
    mut seed: DiscoverySeed,
) -> (DiscoverySeed, DiscoveryOutcome)
where
    S: Spotify,
    F: Fetch + Sync,
{
    if !outside.is_open() {
        return (seed, DiscoveryOutcome::Off);
    }
    let (anchor, own) = match anchor_for(spotify, &seed).await {
        Ok(Some(found)) => found,
        Ok(None) => return (seed, DiscoveryOutcome::Rejected),
        // Before the verify and related steps: a Spotify read that did not
        // answer is the outside being unreachable, never a seed it rejected.
        Err(ReadFailed) => return (seed, DiscoveryOutcome::Unreachable),
    };
    seed.anchor = Some(anchor.clone());

    let Ok(mut seed_titles) = spotify.seed_titles(&anchor).await else {
        return (seed, DiscoveryOutcome::Unreachable);
    };
    seed_titles.truncate(SEED_TITLES_MAX);
    if seed_titles.is_empty() {
        return (seed, DiscoveryOutcome::Rejected);
    }

    let candidate = match identify(outside, &anchor, &seed_titles).await {
        Ok(Some(candidate)) => candidate,
        Ok(None) => return (seed, DiscoveryOutcome::Rejected),
        Err(error) => return (seed, outcome_of(error)),
    };

    let related = match related(outside, candidate.id).await {
        Ok(related) => related,
        Err(error) => return (seed, outcome_of(error)),
    };

    let own: HashSet<String> = own.into_iter().collect();
    let no_exclusions = HashSet::new();
    let resolved = match neighbours(
        spotify,
        outside,
        &related,
        &own,
        &no_exclusions,
        &FIRST_RING,
    )
    .await
    {
        Ok(resolved) => resolved,
        Err(error) => return (seed, outcome_of(error)),
    };
    if resolved.attempted > 0 && resolved.failed == resolved.attempted {
        return (seed, DiscoveryOutcome::Unreachable);
    }

    let cards = if resolved.uris.is_empty() {
        Vec::new()
    } else {
        match spotify.cards(&resolved.uris).await {
            Ok(cards) => stamp(cards, &resolved),
            Err(ReadFailed) => return (seed, DiscoveryOutcome::Unreachable),
        }
    };
    let genre = genre_for(outside, &seed, candidate.id).await;
    (seed, DiscoveryOutcome::Neighbours { cards, genre })
}

struct Resolved {
    uris: Vec<String>,
    /// The outside id each URI was resolved from, in the same order.
    ids: Vec<i64>,
    /// The ids the walk tried and drew no card from.
    passed: Vec<OutsideId>,
    /// How many candidates the filtered union held.
    candidates: usize,
    attempted: usize,
    failed: usize,
}

/// Sets each card's outside id from the neighbour its URI was resolved from.
fn stamp(cards: Vec<DiscoveryCard>, resolved: &Resolved) -> Vec<DiscoveryCard> {
    cards
        .into_iter()
        .map(|mut card| {
            card.outside = resolved
                .ids
                .iter()
                .zip(&resolved.uris)
                .find(|(_, uri)| uri.as_str() == card.artist.uri)
                .map(|(id, _)| OutsideId::from_id(*id));
            card
        })
        .collect()
}

/// The artist a seed is looked up from, and every artist the seed's own source
/// already names.
///
/// The set is what keeps the band honest: an artist that is in the playlist the
/// row was built from is not "just outside your library", however little of it
/// you own. A playlist's head is all Home can see, so this is what the band has
/// read of it, not the whole list.
async fn anchor_for<S: Spotify>(
    spotify: &S,
    seed: &DiscoverySeed,
) -> Result<Option<(DiscoveryArtist, Vec<String>)>, ReadFailed> {
    match seed.kind {
        DiscoveryKind::Playlist => {
            let head = spotify.playlist_head(&seed.uri).await?;
            let Some(anchor) = recurring_anchor(&head) else {
                return Ok(None);
            };
            let mut own: Vec<String> = head
                .iter()
                .flat_map(|entry| entry.artist_uris.iter().cloned())
                .collect();
            own.push(anchor.uri.clone());
            Ok(Some((anchor, own)))
        }
        DiscoveryKind::Artist | DiscoveryKind::Album => Ok(seed.anchor.clone().map(|anchor| {
            let own = vec![anchor.uri.clone()];
            (anchor, own)
        })),
    }
}

/// The first-credited artist that leads at least [`ANCHOR_MIN`] rows, ties to
/// the one that appeared first.
fn recurring_anchor(head: &[TrackEntry]) -> Option<DiscoveryArtist> {
    let mut counts: Vec<(DiscoveryArtist, usize)> = Vec::new();
    for entry in head {
        let Some(name) = entry.artist_names.first() else {
            continue;
        };
        let Some(uri) = entry.artist_uris.first() else {
            continue;
        };
        if name.trim().is_empty() || name == VARIOUS_ARTISTS || uri.is_empty() {
            continue;
        }
        if let Some((_, count)) = counts.iter_mut().find(|(artist, _)| artist.uri == *uri) {
            *count += 1;
        } else {
            counts.push((
                DiscoveryArtist {
                    uri: uri.clone(),
                    name: name.clone(),
                },
                1,
            ));
        }
    }
    let mut best: Option<(DiscoveryArtist, usize)> = None;
    for candidate in counts {
        if candidate.1 < ANCHOR_MIN {
            continue;
        }
        if best.as_ref().is_none_or(|(_, count)| candidate.1 > *count) {
            best = Some(candidate);
        }
    }
    best.map(|(artist, _)| artist)
}

/// One artist's related list, which is what a ring walks from.
async fn related<F: Fetch + Sync>(
    outside: &Outside<F>,
    id: i64,
) -> Result<Vec<wire::ArtistHit>, OutsideError> {
    let body = outside
        .get(&format!("/artist/{id}/related?limit=10"))
        .await?;
    wire::related_artists(&body).map_err(|_| OutsideError::Failed)
}

async fn identify<F: Fetch + Sync>(
    outside: &Outside<F>,
    anchor: &DiscoveryArtist,
    seed_titles: &[String],
) -> Result<Option<wire::ArtistHit>, OutsideError> {
    let path = format!("/search/artist?q={}&limit=5", encode(&anchor.name));
    let body = match outside.get(&path).await {
        Ok(body) => body,
        Err(OutsideError::Absent) => return Ok(None),
        Err(error) => return Err(error),
    };
    let candidates = wire::search_artists(&body).map_err(|_| OutsideError::Failed)?;
    let mut failed = false;
    for candidate in candidates.into_iter().take(VERIFY_MAX) {
        let path = format!("/artist/{}/top?limit=10", candidate.id);
        let body = match outside.get(&path).await {
            Ok(body) => body,
            Err(OutsideError::Off) => return Err(OutsideError::Off),
            Err(OutsideError::Backoff) => return Err(OutsideError::Backoff),
            Err(OutsideError::Absent) => continue,
            Err(OutsideError::Failed) => {
                failed = true;
                continue;
            }
        };
        let titles = match wire::top_titles(&body) {
            Ok(titles) => titles,
            Err(_) => {
                failed = true;
                continue;
            }
        };
        if overlaps(seed_titles, &titles) {
            return Ok(Some(candidate));
        }
    }
    if failed {
        Err(OutsideError::Failed)
    } else {
        Ok(None)
    }
}

/// The related list ordered so the least-followed neighbours come first.
///
/// Deezer orders this list by similarity, and the closest peers are the ones
/// everybody already knows, so walking it as it arrives puts the household names
/// in every row. The follower count is in the same answer — nothing extra is
/// asked for — and a stable sort keeps Deezer's own order between equal counts.
fn deep_cuts(related: &[wire::ArtistHit]) -> Vec<&wire::ArtistHit> {
    let mut ordered: Vec<&wire::ArtistHit> = related.iter().collect();
    ordered.sort_by_key(|hit| hit.fans);
    ordered
}

async fn neighbours<S, F>(
    spotify: &S,
    outside: &Outside<F>,
    related: &[wire::ArtistHit],
    own: &HashSet<String>,
    exclude_ids: &HashSet<i64>,
    limits: &Limits,
) -> Result<Resolved, OutsideError>
where
    S: Spotify,
    F: Fetch + Sync,
{
    // A held or excluded artist is dropped before the ranking, so it neither
    // costs an attempt nor takes a deep cut's place in the ring.
    let mut seen_ids: HashSet<i64> = HashSet::new();
    let filtered: Vec<wire::ArtistHit> = related
        .iter()
        .filter(|hit| !exclude_ids.contains(&hit.id) && seen_ids.insert(hit.id))
        .cloned()
        .collect();
    let candidates = filtered.len();
    let mut uris = Vec::new();
    let mut ids = Vec::new();
    let mut passed = Vec::new();
    let mut seen_uris: HashSet<String> = HashSet::new();
    let mut attempted = 0usize;
    let mut failed = 0usize;
    for neighbour in deep_cuts(&filtered) {
        if uris.len() >= limits.cards || attempted >= limits.attempts {
            break;
        }
        attempted += 1;
        let tried = OutsideId::from_id(neighbour.id);
        let path = format!("/artist/{}/top?limit=1", neighbour.id);
        let body = match outside.get(&path).await {
            Ok(body) => body,
            Err(OutsideError::Off) => return Err(OutsideError::Off),
            Err(OutsideError::Backoff) => return Err(OutsideError::Backoff),
            Err(OutsideError::Absent) => {
                passed.push(tried);
                continue;
            }
            Err(OutsideError::Failed) => {
                failed += 1;
                passed.push(tried);
                continue;
            }
        };
        let titles = match wire::top_titles(&body) {
            Ok(titles) => titles,
            Err(_) => {
                failed += 1;
                passed.push(tried);
                continue;
            }
        };
        let Some(title) = titles.into_iter().next() else {
            passed.push(tried);
            continue;
        };
        let query = format!("{} {}", neighbour.name, title);
        let hits = match spotify.search(&query).await {
            Ok(hits) => hits,
            Err(ReadFailed) => {
                failed += 1;
                passed.push(tried);
                continue;
            }
        };
        let Some((entry, index)) = hits.iter().find_map(|entry| {
            confirm_hit(entry, &title, &neighbour.name).map(|index| (entry, index))
        }) else {
            passed.push(tried);
            continue;
        };
        let Some(uri) = entry.artist_uris.get(index) else {
            passed.push(tried);
            continue;
        };
        if uri.is_empty() || own.contains(uri) || !seen_uris.insert(uri.clone()) {
            passed.push(tried);
            continue;
        }
        ids.push(neighbour.id);
        uris.push(uri.clone());
    }
    Ok(Resolved {
        uris,
        ids,
        passed,
        candidates,
        attempted,
        failed,
    })
}

async fn genre_for<F: Fetch + Sync>(
    outside: &Outside<F>,
    seed: &DiscoverySeed,
    artist_id: i64,
) -> Option<String> {
    let body = outside
        .get(&format!("/artist/{artist_id}/albums?limit=100"))
        .await
        .ok()?;
    let albums = wire::albums(&body).ok()?;
    let records: Vec<i64> = match seed.kind {
        DiscoveryKind::Album => {
            let wanted = normalise(&seed.name);
            albums
                .iter()
                .find(|album| normalise(&album.title) == wanted)
                .and_then(|album| album.genre_id)
                .into_iter()
                .collect()
        }
        DiscoveryKind::Artist | DiscoveryKind::Playlist => {
            albums.iter().filter_map(|album| album.genre_id).collect()
        }
    };
    if records.is_empty() {
        return None;
    }
    let table = outside.genres().await.ok()?;
    genre::vote(&records, &table)
}

fn outcome_of(error: OutsideError) -> DiscoveryOutcome {
    match error {
        OutsideError::Off => DiscoveryOutcome::Off,
        // A record the service does not hold says nothing about this seed: the
        // row moves on to the next one instead of waiting for an open.
        OutsideError::Absent => DiscoveryOutcome::Rejected,
        OutsideError::Backoff | OutsideError::Failed => DiscoveryOutcome::Unreachable,
    }
}

/// Percent-encodes one query value so a name containing `&`, `=` or a space
/// cannot split the query it is part of.
fn encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                let _ = write!(encoded, "%{byte:02X}");
            }
        }
    }
    encoded
}

pub(crate) mod walk;

#[cfg(test)]
mod tests;
