//! What one press of Load more walks, and what it may append.
//!
//! Pure: the walk outside is the binary's, and this only chooses the ring it
//! starts from and the cards that survive landing.

use std::collections::HashSet;

use takofy_model::home::{DiscoveryArtist, DiscoveryCard, OutsideId, RING_CARDS};

use super::fresh::{self, Owned};
use super::hidden::Hidden;

/// The furthest a row digs from the seed it grew from, the first lookup being
/// hop one.
pub const HOPS_MAX: u8 = 3;

/// How long a row says what its last press found before going back to offering
/// another.
pub const DIG_NOTE_MS: u64 = 4_000;

/// What a row's last press found, in the words the view chooses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Note {
    /// There was nothing beyond the cards the row draws.
    Nothing,
    /// The ring was empty but the walk did not reach the end.
    Sparse,
    /// The outside did not answer.
    Unreachable,
}

/// What one row can do next about digging further.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum More {
    /// The row may be pressed.
    Offered,
    /// The row's press is out and its answer has not landed.
    Fetching,
    /// The last press found nothing, for this many milliseconds more.
    Note(Note, u64),
    /// There is nothing further from this seed.
    Done,
}

/// Whether a note stamped at `at_ms` has shown for its whole window by `now_ms`.
/// An unstamped note has not started, so it is never over.
pub fn quiet_over(at_ms: Option<u64>, now_ms: u64) -> bool {
    at_ms.is_some_and(|at| now_ms >= at.saturating_add(DIG_NOTE_MS))
}

/// The ring a row's next dig is walked from: the cards it draws, in the order it
/// draws them, that the row's own tail holds and that carry an outside id.
///
/// Read off the row's kept cards, so a card an earlier row claims is never a
/// node: the dig grows from what the reader can see.
pub fn frontier(kept: &[&DiscoveryCard], tail: &[DiscoveryCard]) -> Vec<OutsideId> {
    kept.iter()
        .filter(|card| tail.iter().any(|grown| grown.artist.uri == card.artist.uri))
        .filter_map(|card| card.outside)
        .take(RING_CARDS)
        .collect()
}

/// The cards a landed ring may append, in the order the walk found them, and the
/// ids that produced no card.
///
/// A card past [`RING_CARDS`] is cut rather than reported: its artist is not
/// given up on for arriving after the cap.
pub fn gather(
    ring: &[DiscoveryCard],
    held: &HashSet<OutsideId>,
    anchor: Option<&DiscoveryArtist>,
    owned: &Owned,
    hidden: &Hidden,
) -> (Vec<DiscoveryCard>, Vec<OutsideId>) {
    let mut cards: Vec<DiscoveryCard> = Vec::new();
    let mut passed: Vec<OutsideId> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for card in ring {
        if cards.len() == RING_CARDS {
            break;
        }
        let already_held = card.outside.is_some_and(|id| held.contains(&id));
        let repeat = !seen.insert(card.artist.uri.as_str());
        if already_held || repeat || !fresh::keeps(card, anchor, owned, hidden) {
            passed.extend(card.outside);
            continue;
        }
        cards.push(card.clone());
    }
    (cards, passed)
}

#[cfg(test)]
mod tests;
