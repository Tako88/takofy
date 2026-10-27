//! The order a show page draws its episodes in.
//!
//! The page opens on the order the wire sent, which is the show's own intent,
//! and one chip turns it round. Both orders keep the wire's order wherever the
//! dates cannot decide, so an undated show is never shuffled behind the user's
//! back.

use std::cmp::Reverse;

use takofy_model::{Episode, PublishedAt};

/// Which end of a show the page reads from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShowOrder {
    /// Most recently published first.
    #[default]
    Newest,
    /// Earliest published first.
    Oldest,
}

impl ShowOrder {
    /// The other end, which is what pressing the order chip asks for.
    pub fn flip(self) -> Self {
        match self {
            Self::Newest => Self::Oldest,
            Self::Oldest => Self::Newest,
        }
    }
}

/// The order the show's own list is already in.
///
/// The wire sends each show in the order it means to be heard, so the two ends
/// name that order. A list with nothing to compare — one episode, or no dates
/// at all — reads the same either way, and opens newest first.
pub fn default_order(episodes: &[Episode]) -> ShowOrder {
    let first = episodes.iter().find_map(|episode| episode.published);
    let last = episodes.iter().rev().find_map(|episode| episode.published);
    match (first, last) {
        (Some(first), Some(last)) if first.as_millis() < last.as_millis() => ShowOrder::Oldest,
        _ => ShowOrder::Newest,
    }
}

/// The episodes in `order`, borrowed from the page's own list.
///
/// Undated episodes come last whichever direction is chosen, in the wire's
/// order among themselves: a date the wire did not send is not an early date.
/// The sort is stable, so two episodes published in the same millisecond keep
/// the order the wire gave them.
pub fn ordered(episodes: &[Episode], order: ShowOrder) -> Vec<&Episode> {
    let stamp = |index: usize| episodes[index].published.map(PublishedAt::as_millis);
    let (mut dated, undated): (Vec<usize>, Vec<usize>) =
        (0..episodes.len()).partition(|&index| stamp(index).is_some());
    if order == ShowOrder::Newest {
        dated.sort_by_key(|&index| Reverse(stamp(index)));
    } else {
        dated.sort_by_key(|&index| stamp(index));
    }
    dated
        .into_iter()
        .chain(undated)
        .map(|index| &episodes[index])
        .collect()
}

#[cfg(test)]
mod tests;
