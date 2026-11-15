//! What shape one discovery row's body takes, and what it therefore counts.
//!
//! Pure: the visible cards in, the body and the numbers the ledger prints out.
//! The row is allowed to be short, absent or missing; nothing stretches to fill
//! a slot that was dropped.

use takofy_model::TrackEntry;
use takofy_model::home::DiscoveryCard;

/// How many cards a row draws, however many the lookup found.
pub const DRAWN: usize = 4;

/// The shape of one row's body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Body {
    /// The cards as faces on the shared track width.
    Faces { cards: usize },
    /// Two cards turned on their side, wide only.
    Pair,
    /// One card and the named tracks worth hearing by it.
    Split { rows: usize },
    /// One card alone.
    One,
}

/// The first [`DRAWN`] cards, which is all a row draws.
pub fn visible_cards<'a>(cards: &'a [&'a DiscoveryCard]) -> &'a [&'a DiscoveryCard] {
    &cards[..cards.len().min(DRAWN)]
}

/// The body `cards` takes at this width.
///
/// One card with at least two named tracks is a split: the card is the row's
/// subject and the tracks are what it has to offer. Fewer named tracks leave the
/// card alone, because a mini-row of one is a list nobody needed.
pub fn body_of(cards: &[&DiscoveryCard], narrow: bool) -> Body {
    let cards = visible_cards(cards);
    match cards {
        [one] => match named_tracks(one).len() {
            0 | 1 => Body::One,
            rows => Body::Split { rows },
        },
        [_, _] if !narrow => Body::Pair,
        [_, _] => Body::Faces { cards: 2 },
        many => Body::Faces { cards: many.len() },
    }
}

/// The columns a body lays its cards in at this width.
///
/// Four is the shared track width and two is the narrow one; a lone card is the
/// one exception, because stretching it over the whole rule would make it a
/// banner rather than a card.
pub fn columns_for(body: Body, narrow: bool) -> usize {
    match body {
        Body::Faces { .. } => {
            if narrow {
                2
            } else {
                4
            }
        }
        Body::Pair => 2,
        Body::Split { .. } | Body::One => 1,
    }
}

/// The named tracks a card draws as mini-rows, in the wire's order.
///
/// An unnamed position stays in the card for play and is never a row: its title
/// is a URI, which is not something a reader can choose between.
pub fn named_tracks(card: &DiscoveryCard) -> Vec<&TrackEntry> {
    card.tracks.iter().filter(|track| track.named).collect()
}

/// The tracks a row's own play queues, in the order the row draws them.
///
/// A split row plays its mini-rows; every other body plays the first track of
/// each face it draws, which is the track its card's own play names.
pub fn row_tracks(cards: &[&DiscoveryCard], body: Body) -> Vec<TrackEntry> {
    let cards = visible_cards(cards);
    match body {
        Body::Split { .. } => cards
            .first()
            .map(|card| named_tracks(card).into_iter().cloned().collect())
            .unwrap_or_default(),
        _ => cards
            .iter()
            .filter_map(|card| card.tracks.first().cloned())
            .collect(),
    }
}

/// The ledger's numbers: how many artists, and how many tracks when the body is
/// a split.
pub fn ledger_counts(cards: &[&DiscoveryCard], body: Body) -> (usize, Option<usize>) {
    let cards = visible_cards(cards);
    match body {
        Body::Split { .. } => (
            cards.len(),
            cards.first().map(|card| named_tracks(card).len()),
        ),
        _ => (cards.len(), None),
    }
}

#[cfg(test)]
mod tests;
