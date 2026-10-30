use std::collections::HashMap;

use librespot::core::{SpotifyId, SpotifyUri};
use takofy_model::{CoverSet, CoverUrl};

use super::{
    Dated, FollowedError, MAX_PAGES, Pager, ShowCard, entries, newest_dated, newest_first,
    show_order,
};
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
fn newest_first_keeps_what_the_predicate_keeps_and_the_first_of_a_duplicate_pair() {
    assert_eq!(
        newest_first(
            vec![
                item(&show(1), Some(100), false),
                item(&album(2), Some(500), false),
                item(&show(1), Some(2_000), false),
                item(&show(3), None, false),
            ],
            |uri| matches!(uri, SpotifyUri::Show { .. }),
        ),
        vec![show(1), show(3)],
        "an album is not a show, and the first show's date sorts, so the newer duplicate does not win"
    );
}

#[test]
fn newest_dated_keeps_each_uri_with_its_date_in_the_same_order() {
    assert_eq!(
        newest_dated(
            vec![
                item(&show(1), Some(100), false),
                item(&show(2), Some(200), true),
                item(&album(3), Some(300), false),
                item(&show(4), None, false),
                item(&show(1), Some(2_000), false),
                item(&show(5), Some(150), false),
            ],
            |uri| matches!(uri, SpotifyUri::Show { .. }),
        ),
        vec![
            Dated {
                uri: show(5),
                added_at: Some(150),
            },
            Dated {
                uri: show(1),
                added_at: Some(100),
            },
            Dated {
                uri: show(4),
                added_at: None,
            },
        ],
        "removed and non-matching items are dropped, the first duplicate's date is kept, and undated comes last"
    );
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
fn a_capped_pager_errors_on_the_first_token_past_its_cap() {
    let mut pager = Pager::with_cap(3);
    assert_eq!(
        pager.accept(Some("t1".to_owned())).expect("a token"),
        Some("t1".to_owned())
    );
    assert_eq!(
        pager.accept(Some("t2".to_owned())).expect("a token"),
        Some("t2".to_owned())
    );
    let error = pager
        .accept(Some("t3".to_owned()))
        .expect_err("a third token would need a fourth request");
    assert!(
        matches!(&error, FollowedError::Source(message) if message == "the collection has more than 3 pages"),
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
            episodes: 0,
        },
    );
    cards.insert(
        show(2),
        ShowCard {
            name: "Published".to_owned(),
            publisher: Some("Band".to_owned()),
            cover: None,
            episodes: 0,
        },
    );
    let cover = CoverSet::single(CoverUrl::from_file_id(&[1; 20]).expect("a file id"));
    cards.insert(
        show(3),
        ShowCard {
            name: "Art".to_owned(),
            publisher: None,
            cover: Some(cover.clone()),
            episodes: 128,
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

#[test]
fn a_card_carries_its_episode_count_and_an_unnamed_show_carries_none() {
    let mut cards = HashMap::new();
    cards.insert(
        show(3),
        ShowCard {
            name: "Art".to_owned(),
            publisher: None,
            cover: None,
            episodes: 128,
        },
    );

    assert_eq!(entries(&[show(3)], &cards)[0].episodes, Some(128));
    assert_eq!(
        entries(&[show(9)], &cards)[0].episodes,
        None,
        "a show nobody named has no count to draw"
    );
}
