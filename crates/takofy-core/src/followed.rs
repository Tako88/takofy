//! Which shows the user follows, in the order the library draws them.
//!
//! Pure: the collection's paging items and one card per show in, the ordered
//! followed shows out. The request itself is in `remote::read_followed_shows`.
//!
//! A show can appear on the wire twice. Removed items are dropped first, so a
//! show followed again after an unfollow is read once; among the remaining
//! occurrences the **first wire occurrence** wins, and its `added_at` is the one
//! the order sorts by.
//!
//! A live occurrence followed by a removed one keeps the show, because the
//! removal is dropped before anything is de-duplicated. Whether the wire ever
//! sends that order is unknown and unprobed.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use librespot::core::SpotifyUri;
use takofy_model::{CoverSet, FollowedShow};

use crate::collection::Item;

/// The most answers one followed-shows read accepts.
pub(crate) const MAX_PAGES: usize = 20;

/// How many items one paging request asks the collection for.
pub(crate) const PAGE_LIMIT: u32 = 500;

/// The most answers one collection-set read accepts.
///
/// The collection set holds liked songs, so it is far larger than a follow
/// set; the probe measured 3 pages at 500 for 1461 items. 200 pages is
/// 100,000 items, and failing loudly past that is the intent.
pub(crate) const COLLECTION_MAX_PAGES: usize = 200;

/// Why a collection-set read or write failed.
#[derive(Debug, thiserror::Error)]
pub enum FollowedError {
    /// The request failed; the message is the source's.
    #[error("{0}")]
    Source(String),
}

/// The token dance one paged read walks.
///
/// The first request carries no token; every answer carries the token that asks
/// for the next page, or none at the end.
///
/// The cap is [`MAX_PAGES`] answers. A `None` answer is the end however many
/// answers came before it, because the set has ended and there is nothing left to
/// ask for. A token that arrives as the [`MAX_PAGES`]-th answer is an error
/// instead of one more request: reading the rest of the set would need a request
/// past the cap, and a list cut short would draw as if it were whole.
///
/// A token equal to the previous one means the endpoint is not advancing, which
/// is also an error rather than a loop. That repeat check only compares against
/// the immediately previous token, so a token that comes back after a different
/// one is not caught here; the page cap is what bounds that read.
#[derive(Debug)]
pub struct Pager {
    pages: usize,
    last: Option<String>,
    max: usize,
}

impl Default for Pager {
    fn default() -> Self {
        Self::new()
    }
}

impl Pager {
    pub fn new() -> Self {
        Self::with_cap(MAX_PAGES)
    }

    /// A pager whose read may take up to `pages` answers.
    pub fn with_cap(pages: usize) -> Self {
        Self {
            pages: 0,
            last: None,
            max: pages,
        }
    }

    /// The next token to ask with, or `None` when the read has ended.
    ///
    /// `next` is the token the answer just carried. This call counts one answer.
    pub fn accept(&mut self, next: Option<String>) -> Result<Option<String>, FollowedError> {
        self.pages = self.pages.saturating_add(1);
        match next {
            // The set has ended. A last answer is never one page too many.
            None => Ok(None),
            // This answer asks for another page, and the cap allows no more of them.
            Some(_) if self.pages >= self.max => Err(FollowedError::Source(format!(
                "the collection has more than {} pages",
                self.max
            ))),
            Some(next) => {
                if self.last.as_deref() == Some(next.as_str()) {
                    return Err(FollowedError::Source(
                        "the collection answer did not end".to_owned(),
                    ));
                }
                self.last = Some(next.clone());
                Ok(Some(next))
            }
        }
    }
}

/// The followed shows' URIs, newest follow first.
///
/// Removed items and anything that is not a show are dropped, the first
/// occurrence of a URI is the one kept, and items without a date come last. The
/// sort is stable, so items sharing a date keep the wire's order.
pub(crate) fn show_order(items: Vec<Item>) -> Vec<String> {
    newest_first(items, |uri| matches!(uri, SpotifyUri::Show { .. }))
}

/// The items' URIs, newest first.
///
/// Removed items and anything the `keep` predicate does not name are dropped,
/// the first occurrence of a URI is the one kept, and items without a date come
/// last. The sort is stable, so items sharing a date keep the wire's order.
pub(crate) fn newest_first(items: Vec<Item>, keep: fn(&SpotifyUri) -> bool) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut kept: Vec<(Option<i64>, String)> = items
        .into_iter()
        .filter(|item| !item.removed)
        .filter(|item| match SpotifyUri::from_uri(&item.uri) {
            Ok(uri) => keep(&uri),
            Err(_) => false,
        })
        .filter(|item| seen.insert(item.uri.clone()))
        .map(|item| (item.added_at, item.uri))
        .collect();
    kept.sort_by_key(|(added_at, _)| Reverse(*added_at));
    kept.into_iter().map(|(_, uri)| uri).collect()
}

/// A show as its card draws it, before it is joined to the followed order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShowCard {
    pub name: String,
    pub publisher: Option<String>,
    pub cover: Option<CoverSet>,
    /// How many episodes the show lists.
    pub episodes: usize,
}

/// The followed shows the grid draws, in the order given.
///
/// A show whose card is missing falls back to its URI and reads as unnamed, so
/// its card still opens the show. A blank publisher is no publisher.
pub(crate) fn entries(order: &[String], cards: &HashMap<String, ShowCard>) -> Vec<FollowedShow> {
    order
        .iter()
        .map(|uri| {
            let card = cards.get(uri);
            FollowedShow {
                uri: uri.clone(),
                name: card.map_or_else(|| uri.clone(), |card| card.name.clone()),
                named: card.is_some(),
                publisher: card
                    .and_then(|card| card.publisher.as_deref())
                    .map(str::trim)
                    .filter(|publisher| !publisher.is_empty())
                    .map(str::to_owned),
                cover: card.and_then(|card| card.cover.clone()),
                episodes: card.map(|card| card.episodes),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
