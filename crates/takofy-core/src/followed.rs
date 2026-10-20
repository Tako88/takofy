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
use takofy_model::{CoverUrl, FollowedShow};

use crate::collection::Item;

/// The most answers one followed-shows read accepts.
pub(crate) const MAX_PAGES: usize = 20;

/// How many items one paging request asks the collection for.
#[allow(
    dead_code,
    reason = "the followed-shows read in `remote` asks for this many items a page"
)]
pub(crate) const PAGE_LIMIT: u32 = 500;

/// Why a followed-shows read failed.
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
#[derive(Debug, Default)]
pub struct Pager {
    pages: usize,
    last: Option<String>,
}

impl Pager {
    pub fn new() -> Self {
        Self::default()
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
            Some(_) if self.pages >= MAX_PAGES => Err(FollowedError::Source(format!(
                "the collection has more than {MAX_PAGES} pages"
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
#[allow(
    dead_code,
    reason = "the followed-shows read in `remote` orders with it"
)]
pub(crate) fn show_order(items: Vec<Item>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut kept: Vec<(Option<i64>, String)> = items
        .into_iter()
        .filter(|item| !item.removed)
        .filter(|item| matches!(SpotifyUri::from_uri(&item.uri), Ok(SpotifyUri::Show { .. })))
        .filter(|item| seen.insert(item.uri.clone()))
        .map(|item| (item.added_at, item.uri))
        .collect();
    kept.sort_by_key(|(added_at, _)| Reverse(*added_at));
    kept.into_iter().map(|(_, uri)| uri).collect()
}

/// A show as its card draws it, before it is joined to the followed order.
pub struct ShowCard {
    pub name: String,
    pub publisher: Option<String>,
    pub cover: Option<CoverUrl>,
}

/// The followed shows the grid draws, in the order given.
///
/// A show whose card is missing falls back to its URI and reads as unnamed, so
/// its card still opens the show. A blank publisher is no publisher.
#[allow(
    dead_code,
    reason = "the followed-shows read in `remote` joins cards with it"
)]
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
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use librespot::core::{SpotifyId, SpotifyUri};
    use takofy_model::CoverUrl;

    use super::{FollowedError, MAX_PAGES, Pager, ShowCard, entries, show_order};
    use crate::collection::Item;

    fn item(uri: &str, added_at: Option<i64>, removed: bool) -> Item {
        Item {
            uri: uri.to_owned(),
            added_at,
            removed,
        }
    }

    fn id(byte: u8) -> SpotifyId {
        SpotifyId::from_raw(&[byte; 16]).expect("a 16-byte id")
    }

    fn show(byte: u8) -> String {
        SpotifyUri::Show { id: id(byte) }.to_uri().expect("a uri")
    }

    fn album(byte: u8) -> String {
        SpotifyUri::Album { id: id(byte) }.to_uri().expect("a uri")
    }

    #[test]
    fn order_drops_removed_and_non_show_items() {
        assert_eq!(
            show_order(vec![
                item(&show(1), Some(100), false),
                item(&show(2), Some(200), true),
                item(&album(3), Some(300), false),
                item(&show(4), None, false),
            ]),
            vec![show(1), show(4)],
            "a removed show and an album are not followed shows"
        );
    }

    #[test]
    fn order_keeps_the_first_duplicate() {
        assert_eq!(
            show_order(vec![
                item(&show(1), Some(100), false),
                item(&show(2), Some(300), false),
                item(&show(1), Some(2_000), false),
            ]),
            vec![show(2), show(1)],
            "the first occurrence's date sorts, so the later, newer duplicate does not win"
        );
    }

    #[test]
    fn order_drops_a_removed_occurrence_before_de_duplicating() {
        assert_eq!(
            show_order(vec![
                item(&show(1), Some(100), true), // the first occurrence, since removed
                item(&show(3), Some(200), false),
                item(&show(1), Some(300), false), // the same show, followed again
            ]),
            vec![show(1), show(3)],
            "the removed occurrence does not hide the live one, and the live date is the one sorted by"
        );
    }

    #[test]
    fn order_is_newest_added_first_with_undated_last_and_stable_ties() {
        assert_eq!(
            show_order(vec![
                item(&show(6), None, false),
                item(&show(4), Some(100), false),
                item(&show(3), Some(300), false),
                item(&show(2), Some(100), false),
                item(&show(1), None, false),
            ]),
            vec![show(3), show(4), show(2), show(6), show(1)],
            "the dated tie keeps the wire's descending order, and so does the undated pair"
        );
    }

    #[test]
    fn an_empty_item_list_gives_an_empty_order() {
        assert!(show_order(Vec::new()).is_empty());
    }

    #[test]
    fn the_pager_follows_distinct_tokens_and_stops_on_none() {
        let mut pager = Pager::new();
        assert_eq!(
            pager.accept(Some("t1".to_owned())).expect("a token"),
            Some("t1".to_owned())
        );
        assert_eq!(
            pager.accept(Some("t2".to_owned())).expect("a token"),
            Some("t2".to_owned())
        );
        assert_eq!(pager.accept(None).expect("the end"), None);
    }

    #[test]
    fn a_first_answer_without_a_token_ends_the_read() {
        let mut pager = Pager::new();
        assert_eq!(
            pager.accept(None).expect("a set with one page"),
            None,
            "one answer and no token is a whole set, not an empty read"
        );
    }

    #[test]
    fn the_pager_refuses_a_repeated_token() {
        let mut pager = Pager::new();
        pager
            .accept(Some("t1".to_owned()))
            .expect("the first token");
        let error = pager
            .accept(Some("t1".to_owned()))
            .expect_err("the same token again is not a page");
        assert!(
            matches!(&error, FollowedError::Source(message) if message == "the collection answer did not end"),
            "{error}"
        );
    }

    #[test]
    fn the_twentieth_answer_without_a_token_ends_the_read() {
        let mut pager = Pager::new();
        for page in 1..MAX_PAGES {
            assert_eq!(
                pager.accept(Some(format!("t{page}"))).expect("a token"),
                Some(format!("t{page}")),
                "page {page} still has a page after it"
            );
        }
        assert_eq!(
            pager.accept(None).expect("the twentieth and last answer"),
            None,
            "twenty answers fill the cap exactly, so the last one may end the set"
        );
    }

    #[test]
    fn a_token_on_the_twentieth_answer_exceeds_the_cap() {
        let mut pager = Pager::new();
        for page in 1..MAX_PAGES {
            pager
                .accept(Some(format!("t{page}")))
                .expect("a token inside the cap");
        }
        let error = pager
            .accept(Some("one too many".to_owned()))
            .expect_err("a twentieth token would need a twenty-first request");
        assert!(
            matches!(&error, FollowedError::Source(message) if message == "the collection has more than 20 pages"),
            "{error}"
        );
    }

    #[test]
    fn a_token_after_a_finished_read_exceeds_the_cap() {
        let mut pager = Pager::new();
        for page in 1..MAX_PAGES {
            pager
                .accept(Some(format!("t{page}")))
                .expect("a token inside the cap");
        }
        assert_eq!(pager.accept(None).expect("the end"), None);
        assert!(
            pager.accept(Some("after the end".to_owned())).is_err(),
            "a token after twenty accepted answers is one page too many"
        );
    }

    #[test]
    fn an_unlabelled_show_falls_back_to_its_uri_unnamed() {
        let entries = entries(&[show(1)], &HashMap::new());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].uri, show(1));
        assert_eq!(entries[0].name, show(1));
        assert!(!entries[0].named);
        assert_eq!(entries[0].publisher, None);
        assert_eq!(entries[0].cover, None);
    }

    #[test]
    fn a_blank_publisher_is_none() {
        let mut cards = HashMap::new();
        cards.insert(
            show(1),
            ShowCard {
                name: "Lateral with Tom Scott".to_owned(),
                publisher: Some("   ".to_owned()),
                cover: None,
            },
        );
        cards.insert(
            show(2),
            ShowCard {
                name: "Published".to_owned(),
                publisher: Some("Band".to_owned()),
                cover: None,
            },
        );
        let cover = CoverUrl::from_file_id(&[1; 20]).expect("a file id");
        cards.insert(
            show(3),
            ShowCard {
                name: "Art".to_owned(),
                publisher: None,
                cover: Some(cover.clone()),
            },
        );

        let blank = entries(&[show(1)], &cards);
        assert_eq!(blank[0].name, "Lateral with Tom Scott");
        assert!(blank[0].named);
        assert_eq!(blank[0].publisher, None);

        assert_eq!(
            entries(&[show(2)], &cards)[0].publisher,
            Some("Band".to_owned())
        );
        assert_eq!(entries(&[show(3)], &cards)[0].cover, Some(cover));
    }
}
