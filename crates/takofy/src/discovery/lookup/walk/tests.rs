//! Scripted related lists and constructed Spotify hits: every name, id and URI
//! is invented.

use std::time::Duration;

use takofy_model::home::{
    DigAsk, DigOutcome, DiscoveryCard, DiscoveryKind, DiscoverySeed, OutsideId,
};

use super::walk;
use crate::discovery::FetchError;
use crate::discovery::test_support::{
    Call, FakeFetch, FakeSpotify, artist, card, hit, one_title, outside,
};

fn artist_seed() -> DiscoverySeed {
    DiscoverySeed {
        kind: DiscoveryKind::Artist,
        uri: "spotify:artist:vela".to_owned(),
        name: "Vela".to_owned(),
        anchor: Some(artist("Vela")),
    }
}

fn playlist_seed() -> DiscoverySeed {
    DiscoverySeed {
        kind: DiscoveryKind::Playlist,
        uri: "spotify:playlist:mix".to_owned(),
        name: "Mix".to_owned(),
        anchor: None,
    }
}

fn ask(seed: DiscoverySeed, frontier: &[i64], held: &[i64]) -> DigAsk {
    DigAsk {
        seed,
        hop: 2,
        frontier: frontier.iter().map(|id| OutsideId::from_id(*id)).collect(),
        held: held.iter().map(|id| OutsideId::from_id(*id)).collect(),
        skip: Vec::new(),
    }
}

/// One related list, ordered as the service would send it.
fn related_body(hits: &[(i64, &str, u64)]) -> Vec<u8> {
    let data = hits
        .iter()
        .map(|(id, name, fans)| format!(r#"{{"id":{id},"name":"{name}","nb_fan":{fans}}}"#))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"_synthetic":"synthetic","data":[{data}],"next":null,"total":{}}}"#,
        hits.len()
    )
    .into_bytes()
}

/// A related list of `count` candidates, each one unfollowed and unfindable.
fn unfindable(count: usize) -> Vec<u8> {
    let data = (0..count)
        .map(|index| {
            format!(
                r#"{{"id":{},"name":"Neighbour {index}","nb_fan":{index}}}"#,
                1000 + index
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(r#"{{"_synthetic":"synthetic","data":[{data}],"next":null,"total":{count}}}"#)
        .into_bytes()
}

fn top(fetch: &FakeFetch, id: i64, title: &str) {
    fetch.ok(&format!("/artist/{id}/top"), one_title(title));
}

fn related(fetch: &FakeFetch, id: i64, hits: &[(i64, &str, u64)]) {
    fetch.ok(&format!("/artist/{id}/related"), related_body(hits));
}

/// The Spotify side of a ring: which candidate resolves to which artist, and
/// which of those come back as cards.
struct Script {
    spotify: FakeSpotify,
    cards: Vec<DiscoveryCard>,
}

impl Script {
    fn new() -> Self {
        Self {
            spotify: FakeSpotify::new(&["Seed Track"]),
            cards: Vec::new(),
        }
    }

    fn card(self, name: &str, uri: &str) -> Self {
        self.resolves_to(name, uri).with_card(uri, name)
    }

    fn resolves_to(self, name: &str, uri: &str) -> Self {
        Self {
            spotify: self.spotify.with_search(
                &format!("{name} Set {name}"),
                vec![hit(&format!("Set {name}"), name, uri)],
            ),
            ..self
        }
    }

    fn with_card(mut self, uri: &str, name: &str) -> Self {
        self.cards.push(card(uri, name));
        self
    }

    fn build(self) -> FakeSpotify {
        self.spotify.with_cards(self.cards)
    }
}

fn ring(outcome: DigOutcome) -> (Vec<String>, Vec<OutsideId>, bool) {
    match outcome {
        DigOutcome::Ring {
            cards,
            passed,
            exhausted,
        } => (
            cards.iter().map(|card| card.artist.uri.clone()).collect(),
            passed,
            exhausted,
        ),
        other => panic!("expected a ring, got {other:?}"),
    }
}

fn cards(outcome: DigOutcome) -> Vec<DiscoveryCard> {
    match outcome {
        DigOutcome::Ring { cards, .. } => cards,
        other => panic!("expected a ring, got {other:?}"),
    }
}

#[tokio::test]
async fn a_ring_unions_the_frontier_lists_and_reads_each_once() {
    let fetch = FakeFetch::new();
    related(&fetch, 100, &[(1, "Alpha", 10), (2, "Beta", 20)]);
    related(&fetch, 200, &[(3, "Gamma", 30), (1, "Alpha", 10)]);
    top(&fetch, 1, "Set Alpha");
    top(&fetch, 2, "Set Beta");
    top(&fetch, 3, "Set Gamma");
    let spotify = Script::new()
        .card("Alpha", "spotify:artist:alpha")
        .card("Beta", "spotify:artist:beta")
        .card("Gamma", "spotify:artist:gamma")
        .build();
    let outside = outside(&fetch, true);

    let (uris, passed, exhausted) =
        ring(walk(&spotify, &outside, &ask(artist_seed(), &[100, 200], &[])).await);

    assert_eq!(
        uris,
        vec![
            "spotify:artist:alpha",
            "spotify:artist:beta",
            "spotify:artist:gamma"
        ]
    );
    assert!(passed.is_empty());
    assert!(exhausted);
    assert_eq!(
        fetch.count(),
        5,
        "one related read per frontier card, and one read per candidate"
    );
}

#[tokio::test]
async fn held_and_frontier_ids_cost_no_attempt() {
    let fetch = FakeFetch::new();
    related(
        &fetch,
        100,
        &[(100, "Itself", 1), (500, "Held", 2), (1, "Alpha", 3)],
    );
    top(&fetch, 1, "Set Alpha");
    // No top route for 100 or 500: an attempt would fail as a transport error.
    let spotify = Script::new().card("Alpha", "spotify:artist:alpha").build();
    let outside = outside(&fetch, true);

    let (uris, passed, exhausted) =
        ring(walk(&spotify, &outside, &ask(artist_seed(), &[100], &[500])).await);

    assert_eq!(uris, vec!["spotify:artist:alpha"]);
    assert!(passed.is_empty());
    assert!(exhausted);
    assert_eq!(fetch.count(), 2, "one related read and one candidate read");
}

#[tokio::test]
async fn held_artists_do_not_use_up_the_ring() {
    let fetch = FakeFetch::new();
    related(
        &fetch,
        100,
        &[
            (1, "Held A", 1),
            (2, "Held B", 2),
            (3, "Gamma", 3),
            (4, "Delta", 4),
            (5, "Epsilon", 5),
            (6, "Zeta", 6),
        ],
    );
    for (id, name) in [(3, "Gamma"), (4, "Delta"), (5, "Epsilon"), (6, "Zeta")] {
        top(&fetch, id, &format!("Set {name}"));
    }
    let spotify = Script::new()
        .card("Gamma", "spotify:artist:gamma")
        .card("Delta", "spotify:artist:delta")
        .card("Epsilon", "spotify:artist:epsilon")
        .card("Zeta", "spotify:artist:zeta")
        .build();
    let outside = outside(&fetch, true);

    let (uris, passed, exhausted) =
        ring(walk(&spotify, &outside, &ask(artist_seed(), &[100], &[1, 2])).await);

    assert_eq!(
        uris,
        vec![
            "spotify:artist:gamma",
            "spotify:artist:delta",
            "spotify:artist:epsilon",
            "spotify:artist:zeta"
        ],
        "the ring fills from the candidates the row does not hold"
    );
    assert!(passed.is_empty());
    assert!(exhausted);
    assert_eq!(fetch.count(), 5);
}

#[tokio::test]
async fn the_ring_caps_at_four_cards() {
    let fetch = FakeFetch::new();
    related(
        &fetch,
        100,
        &[
            (1, "Alpha", 1),
            (2, "Beta", 2),
            (3, "Gamma", 3),
            (4, "Delta", 4),
            (5, "Epsilon", 5),
            (6, "Zeta", 6),
        ],
    );
    for (id, name) in [
        (1, "Alpha"),
        (2, "Beta"),
        (3, "Gamma"),
        (4, "Delta"),
        (5, "Epsilon"),
        (6, "Zeta"),
    ] {
        top(&fetch, id, &format!("Set {name}"));
    }
    let spotify = Script::new()
        .card("Alpha", "spotify:artist:alpha")
        .card("Beta", "spotify:artist:beta")
        .card("Gamma", "spotify:artist:gamma")
        .card("Delta", "spotify:artist:delta")
        .build();
    let outside = outside(&fetch, true);

    let (uris, _, exhausted) =
        ring(walk(&spotify, &outside, &ask(artist_seed(), &[100], &[])).await);

    assert_eq!(
        uris,
        vec![
            "spotify:artist:alpha",
            "spotify:artist:beta",
            "spotify:artist:gamma",
            "spotify:artist:delta"
        ]
    );
    assert!(!exhausted, "two candidates were never tried");
    assert_eq!(fetch.count(), 5);
}

#[tokio::test]
async fn an_artist_the_library_holds_gives_no_card_and_is_remembered() {
    let fetch = FakeFetch::new();
    related(&fetch, 100, &[(7, "Owned", 10), (8, "Fresh", 20)]);
    top(&fetch, 7, "Set Owned");
    top(&fetch, 8, "Set Fresh");
    let spotify = Script::new()
        .resolves_to("Owned", "spotify:artist:owned")
        .card("Fresh", "spotify:artist:fresh")
        .build();
    let outside = outside(&fetch, true);
    let mut dig = ask(artist_seed(), &[100], &[]);
    dig.skip.push("spotify:artist:owned".to_owned());

    let (uris, passed, exhausted) = ring(walk(&spotify, &outside, &dig).await);

    assert_eq!(uris, vec!["spotify:artist:fresh"]);
    assert_eq!(passed, vec![OutsideId::from_id(7)]);
    assert!(exhausted);
    assert_eq!(
        fetch.count(),
        3,
        "a skipped artist still costs the attempt it resolved in"
    );
}

#[tokio::test]
async fn a_playlist_seed_reads_its_head_once_and_skips_it() {
    let fetch = FakeFetch::new();
    related(&fetch, 100, &[(7, "In Head", 10), (8, "Out", 20)]);
    top(&fetch, 7, "Set In Head");
    top(&fetch, 8, "Set Out");
    let head = vec![
        hit("One", "Vela", "spotify:artist:vela"),
        hit("Two", "Vela", "spotify:artist:vela"),
        hit("Three", "In Head", "spotify:artist:in-head"),
    ];
    let spotify = Script::new()
        .resolves_to("In Head", "spotify:artist:in-head")
        .card("Out", "spotify:artist:out")
        .build()
        .with_head(head);
    let outside = outside(&fetch, true);

    let (uris, passed, _) =
        ring(walk(&spotify, &outside, &ask(playlist_seed(), &[100], &[])).await);

    assert_eq!(uris, vec!["spotify:artist:out"]);
    assert_eq!(passed, vec![OutsideId::from_id(7)]);
    assert_eq!(
        spotify.reads(),
        4,
        "one head read, one search per candidate and one card read"
    );
}

#[tokio::test]
async fn attempts_stop_at_sixteen_and_leave_the_ring_unexhausted() {
    let fetch = FakeFetch::new();
    fetch.ok("/artist/100/related", unfindable(20));
    let spotify = Script::new().build();
    let outside = outside(&fetch, true);

    let outcome = walk(&spotify, &outside, &ask(artist_seed(), &[100], &[])).await;

    match outcome {
        DigOutcome::Ring {
            cards,
            passed,
            exhausted,
        } => {
            assert!(cards.is_empty());
            assert_eq!(passed.len(), 16);
            assert!(!exhausted, "the union held more than the attempts allowed");
        }
        other => panic!("expected a ring, got {other:?}"),
    }
    assert_eq!(fetch.count(), 17, "one related read and sixteen attempts");
}

#[tokio::test]
async fn an_empty_union_is_exhausted() {
    let fetch = FakeFetch::new();
    related(&fetch, 100, &[]);
    let spotify = Script::new().build();
    let outside = outside(&fetch, true);

    let (uris, passed, exhausted) =
        ring(walk(&spotify, &outside, &ask(artist_seed(), &[100], &[])).await);

    assert!(uris.is_empty());
    assert!(passed.is_empty());
    assert!(exhausted);
    assert_eq!(fetch.count(), 1);
}

#[tokio::test]
async fn the_whole_ring_stays_within_four_related_reads_and_sixteen_tops() {
    let fetch = FakeFetch::new();
    for id in [100, 200, 300, 400] {
        fetch.ok(&format!("/artist/{id}/related"), unfindable(20));
    }
    let spotify = Script::new().build();
    let outside = outside(&fetch, true);

    let (uris, passed, exhausted) = ring(
        walk(
            &spotify,
            &outside,
            &ask(artist_seed(), &[100, 200, 300, 400], &[]),
        )
        .await,
    );

    assert!(uris.is_empty());
    assert_eq!(passed.len(), 16);
    assert!(!exhausted);
    assert_eq!(fetch.count(), 20, "four related reads and sixteen attempts");
}

#[tokio::test]
async fn off_costs_no_fetch_and_no_spotify_read() {
    let fetch = FakeFetch::new();
    let spotify = Script::new().build();
    let outside = outside(&fetch, false);

    let outcome = walk(&spotify, &outside, &ask(artist_seed(), &[100], &[])).await;

    assert_eq!(outcome, DigOutcome::Off);
    assert_eq!(fetch.count(), 0);
    assert_eq!(spotify.reads(), 0);
}

#[tokio::test]
async fn a_record_the_service_does_not_hold_is_skipped() {
    let fetch = FakeFetch::new();
    fetch.ok(
        "/artist/100/related",
        br#"{"error":{"type":"DataException","code":800}}"#.to_vec(),
    );
    related(&fetch, 200, &[(1, "Alpha", 10)]);
    top(&fetch, 1, "Set Alpha");
    let spotify = Script::new().card("Alpha", "spotify:artist:alpha").build();
    let outside = outside(&fetch, true);

    let (uris, _, exhausted) =
        ring(walk(&spotify, &outside, &ask(artist_seed(), &[100, 200], &[])).await);

    assert_eq!(uris, vec!["spotify:artist:alpha"]);
    assert!(exhausted);
    assert_eq!(fetch.count(), 3);
}

#[tokio::test]
async fn every_related_read_failing_is_unreachable() {
    let fetch = FakeFetch::new();
    fetch.push("/artist/100/related", Err(FetchError::Transport));
    fetch.push("/artist/200/related", Err(FetchError::Transport));
    let spotify = Script::new().build();
    let outside = outside(&fetch, true);

    let outcome = walk(&spotify, &outside, &ask(artist_seed(), &[100, 200], &[])).await;

    assert_eq!(outcome, DigOutcome::Unreachable);
    assert_eq!(fetch.count(), 2);
}

#[tokio::test]
async fn a_429_during_a_related_read_is_unreachable() {
    let fetch = FakeFetch::new();
    fetch.push(
        "/artist/100/related",
        Err(FetchError::Status(429, Some(Duration::from_secs(60)))),
    );
    related(&fetch, 200, &[(1, "Alpha", 10)]);
    let spotify = Script::new().build();
    let outside = outside(&fetch, true);

    let outcome = walk(&spotify, &outside, &ask(artist_seed(), &[100, 200], &[])).await;

    assert_eq!(outcome, DigOutcome::Unreachable);
    assert_eq!(fetch.count(), 1, "a pause stops the ring at once");
}

#[tokio::test]
async fn a_failed_card_read_is_unreachable() {
    let fetch = FakeFetch::new();
    related(&fetch, 100, &[(1, "Alpha", 10)]);
    top(&fetch, 1, "Set Alpha");
    let spotify = Script::new()
        .card("Alpha", "spotify:artist:alpha")
        .build()
        .failing(Call::Cards);
    let outside = outside(&fetch, true);

    let outcome = walk(&spotify, &outside, &ask(artist_seed(), &[100], &[])).await;

    assert_eq!(outcome, DigOutcome::Unreachable);
    assert_eq!(spotify.asked_cards().len(), 1);
}

#[tokio::test]
async fn a_playlist_with_no_anchor_answers_an_empty_exhausted_ring() {
    let fetch = FakeFetch::new();
    let head = vec![hit("One", "Only", "spotify:artist:only")];
    let spotify = Script::new().build().with_head(head);
    let outside = outside(&fetch, true);

    let outcome = walk(&spotify, &outside, &ask(playlist_seed(), &[100], &[])).await;

    let (uris, passed, exhausted) = ring(outcome);
    assert!(uris.is_empty());
    assert!(passed.is_empty());
    assert!(exhausted);
    assert_eq!(fetch.count(), 0);
    assert_eq!(spotify.reads(), 1, "the head was read once");
}

#[tokio::test]
async fn every_card_carries_the_id_of_the_neighbour_it_came_from() {
    let fetch = FakeFetch::new();
    related(&fetch, 100, &[(7, "Alpha", 10), (8, "Beta", 20)]);
    top(&fetch, 7, "Set Alpha");
    top(&fetch, 8, "Set Beta");
    let spotify = Script::new()
        .card("Alpha", "spotify:artist:alpha")
        .card("Beta", "spotify:artist:beta")
        .build();
    let outside = outside(&fetch, true);

    let cards = cards(walk(&spotify, &outside, &ask(artist_seed(), &[100], &[])).await);

    let stamped: Vec<(&str, Option<OutsideId>)> = cards
        .iter()
        .map(|card| (card.artist.uri.as_str(), card.outside))
        .collect();
    assert_eq!(
        stamped,
        vec![
            ("spotify:artist:alpha", Some(OutsideId::from_id(7))),
            ("spotify:artist:beta", Some(OutsideId::from_id(8))),
        ]
    );
}
