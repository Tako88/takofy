//! Constructed Spotify lists and scripted outside bodies: no socket, and every
//! name, id and URI is invented.

use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use takofy_model::TrackEntry;
use takofy_model::home::{
    DiscoveryArtist, DiscoveryCard, DiscoveryKind, DiscoveryOutcome, DiscoverySeed,
};

use super::{Spotify, lookup};
use crate::discovery::test_support::FakeFetch;
use crate::discovery::{FetchError, Outside};

const SEARCH: &str = include_str!("../fixtures/search_artist.json");
const TOP: &str = include_str!("../fixtures/top.json");
const RELATED: &str = include_str!("../fixtures/related.json");
const ALBUMS: &str = include_str!("../fixtures/albums.json");
const GENRE: &str = include_str!("../fixtures/genre.json");

const QUOTA_ERROR: &str = r#"{"error":{"type":"DataException","message":"no data","code":800}}"#;

struct FakeSpotify {
    titles: Vec<String>,
    head: Vec<TrackEntry>,
    searches: Mutex<Vec<(String, Vec<TrackEntry>)>>,
    cards: Vec<DiscoveryCard>,
    reads: AtomicUsize,
    asked_cards: Mutex<Vec<Vec<String>>>,
}

impl FakeSpotify {
    fn new(titles: &[&str]) -> Self {
        Self {
            titles: titles.iter().map(|title| (*title).to_owned()).collect(),
            head: Vec::new(),
            searches: Mutex::new(Vec::new()),
            cards: Vec::new(),
            reads: AtomicUsize::new(0),
            asked_cards: Mutex::new(Vec::new()),
        }
    }

    fn with_head(mut self, head: Vec<TrackEntry>) -> Self {
        self.head = head;
        self
    }

    fn with_search(self, needle: &str, hits: Vec<TrackEntry>) -> Self {
        self.searches
            .lock()
            .expect("searches")
            .push((needle.to_owned(), hits));
        self
    }

    fn with_cards(mut self, cards: Vec<DiscoveryCard>) -> Self {
        self.cards = cards;
        self
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::Relaxed)
    }

    fn asked_cards(&self) -> Vec<Vec<String>> {
        self.asked_cards.lock().expect("cards").clone()
    }
}

impl Spotify for FakeSpotify {
    fn seed_titles(&self, _artist: &DiscoveryArtist) -> impl Future<Output = Vec<String>> + Send {
        self.reads.fetch_add(1, Ordering::Relaxed);
        let titles = self.titles.clone();
        async move { titles }
    }

    fn playlist_head(&self, _uri: &str) -> impl Future<Output = Vec<TrackEntry>> + Send {
        self.reads.fetch_add(1, Ordering::Relaxed);
        let head = self.head.clone();
        async move { head }
    }

    fn search(&self, query: &str) -> impl Future<Output = Vec<TrackEntry>> + Send {
        self.reads.fetch_add(1, Ordering::Relaxed);
        let hits = self
            .searches
            .lock()
            .expect("searches")
            .iter()
            .find(|(needle, _)| query.contains(needle.as_str()))
            .map(|(_, hits)| hits.clone())
            .unwrap_or_default();
        async move { hits }
    }

    fn cards(&self, uris: &[String]) -> impl Future<Output = Vec<DiscoveryCard>> + Send {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.asked_cards.lock().expect("cards").push(uris.to_vec());
        let cards = self.cards.clone();
        async move { cards }
    }
}

fn artist_seed() -> DiscoverySeed {
    DiscoverySeed {
        kind: DiscoveryKind::Artist,
        uri: "spotify:artist:vela".to_owned(),
        name: "Vela".to_owned(),
        anchor: Some(artist("Vela")),
    }
}

fn artist(name: &str) -> DiscoveryArtist {
    DiscoveryArtist {
        uri: format!("spotify:artist:{}", name.to_lowercase().replace(' ', "-")),
        name: name.to_owned(),
    }
}

fn hit(title: &str, name: &str, uri: &str) -> TrackEntry {
    TrackEntry {
        uri: format!("spotify:track:{}", title.replace(' ', "-")),
        title: title.to_owned(),
        artists: name.to_owned(),
        album: None,
        album_uri: None,
        artist_uris: vec![uri.to_owned()],
        artist_names: vec![name.to_owned()],
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

fn card(uri: &str, name: &str) -> DiscoveryCard {
    DiscoveryCard {
        artist: DiscoveryArtist {
            uri: uri.to_owned(),
            name: name.to_owned(),
        },
        portrait: None,
        top_track: format!("spotify:track:{name}"),
        top_title: "Top".to_owned(),
        latest_year: None,
    }
}

fn one_title(title: &str) -> Vec<u8> {
    format!(
        r#"{{"_synthetic":"synthetic","data":[{{"id":1,"title":"{title}"}}],"next":null,"total":1}}"#
    )
    .into_bytes()
}

fn no_overlap() -> Vec<u8> {
    br#"{"_synthetic":"synthetic","data":[{"title":"Nothing Alike"},{"title":"Still Nothing"}],"next":null,"total":2}"#.to_vec()
}

fn empty_list() -> Vec<u8> {
    br#"{"_synthetic":"synthetic","data":[],"next":null,"total":0}"#.to_vec()
}

fn excluded_albums() -> Vec<u8> {
    br#"{"_synthetic":"synthetic","data":[{"title":"One","genre_id":0},{"title":"Two","genre_id":1},{"title":"Three","genre_id":10}],"total":3}"#.to_vec()
}

fn album_seed_albums() -> Vec<u8> {
    br#"{"_synthetic":"synthetic","data":[{"title":"Day Break","genre_id":13},{"title":"Other One","genre_id":11},{"title":"Other Two","genre_id":11}],"total":3}"#.to_vec()
}

fn route_happy(fetch: &FakeFetch) {
    fetch.ok("search/artist", SEARCH.as_bytes().to_vec());
    fetch.ok("1001/top", TOP.as_bytes().to_vec());
    fetch.ok("1001/related", RELATED.as_bytes().to_vec());
    fetch.ok("1001/albums", ALBUMS.as_bytes().to_vec());
    fetch.ok("/genre", GENRE.as_bytes().to_vec());
    for (id, title) in [("2002", "Window"), ("2003", "Plains"), ("2004", "Fracture")] {
        fetch.ok(&format!("{id}/top"), one_title(title));
    }
}

fn spotify_happy(titles: &[&str]) -> FakeSpotify {
    FakeSpotify::new(titles)
        .with_search(
            "Aphex Twin Window",
            vec![hit("Window", "Aphex Twin", "spotify:artist:aphex")],
        )
        .with_search(
            "Seefeel Plains",
            vec![hit("Plains", "Seefeel", "spotify:artist:seefeel")],
        )
        .with_search(
            "Clark Fracture",
            vec![hit("Fracture", "Clark", "spotify:artist:clark")],
        )
        .with_cards(vec![
            card("spotify:artist:aphex", "Aphex Twin"),
            card("spotify:artist:seefeel", "Seefeel"),
            card("spotify:artist:clark", "Clark"),
        ])
}

fn outside(fetch: &FakeFetch, gate: bool) -> Outside<FakeFetch> {
    Outside::new(fetch.clone(), Arc::new(AtomicBool::new(gate)))
}

#[test]
fn a_name_with_a_separator_is_encoded() {
    assert_eq!(
        super::encode("Simon & Garfunkel"),
        "Simon%20%26%20Garfunkel"
    );
}

#[tokio::test]
async fn off_costs_no_fetch_and_no_spotify_read() {
    let fetch = FakeFetch::new();
    route_happy(&fetch);
    let spotify = spotify_happy(&["Signal Fire", "Night Watch"]);
    let outside = outside(&fetch, false);

    let (_, outcome) = lookup(&spotify, &outside, artist_seed()).await;

    assert_eq!(outcome, DiscoveryOutcome::Off);
    assert_eq!(fetch.count(), 0);
    assert_eq!(spotify.reads(), 0);
}

#[tokio::test]
async fn the_happy_path_returns_the_cards_and_the_vote() {
    let fetch = FakeFetch::new();
    route_happy(&fetch);
    let spotify = spotify_happy(&["Signal Fire", "Night Watch"]);
    let outside = outside(&fetch, true);

    let (seed, outcome) = lookup(&spotify, &outside, artist_seed()).await;

    assert_eq!(seed.anchor, Some(artist("Vela")));
    match outcome {
        DiscoveryOutcome::Neighbours { cards, genre } => {
            assert_eq!(cards.len(), 3);
            assert_eq!(genre.as_deref(), Some("Rock"));
        }
        other => panic!("expected neighbours, got {other:?}"),
    }
    assert_eq!(fetch.count(), 8);
    assert_eq!(spotify.reads(), 5);
}

#[tokio::test]
async fn a_seed_nobody_matches_is_rejected() {
    let fetch = FakeFetch::new();
    fetch.ok("search/artist", SEARCH.as_bytes().to_vec());
    fetch.ok("1001/top", no_overlap());
    fetch.ok("1002/top", no_overlap());
    let spotify = FakeSpotify::new(&["Signal Fire", "Night Watch"]);
    let outside = outside(&fetch, true);

    let (_, outcome) = lookup(&spotify, &outside, artist_seed()).await;

    assert_eq!(outcome, DiscoveryOutcome::Rejected);
    assert_eq!(fetch.count(), 3);
}

#[tokio::test]
async fn the_first_candidate_can_miss_and_the_second_hit() {
    let fetch = FakeFetch::new();
    fetch.ok("search/artist", SEARCH.as_bytes().to_vec());
    fetch.ok("1001/top", no_overlap());
    fetch.ok("1002/top", TOP.as_bytes().to_vec());
    fetch.ok("1002/related", RELATED.as_bytes().to_vec());
    fetch.ok("1002/albums", ALBUMS.as_bytes().to_vec());
    fetch.ok("/genre", GENRE.as_bytes().to_vec());
    for (id, title) in [("2002", "Window"), ("2003", "Plains"), ("2004", "Fracture")] {
        fetch.ok(&format!("{id}/top"), one_title(title));
    }
    let spotify = spotify_happy(&["Signal Fire", "Night Watch"]);
    let outside = outside(&fetch, true);

    let (_, outcome) = lookup(&spotify, &outside, artist_seed()).await;

    match outcome {
        DiscoveryOutcome::Neighbours { cards, .. } => assert_eq!(cards.len(), 3),
        other => panic!("expected neighbours, got {other:?}"),
    }
    assert_eq!(fetch.count(), 9);
}

#[tokio::test]
async fn a_neighbour_with_the_wrong_title_or_name_is_dropped() {
    let fetch = FakeFetch::new();
    fetch.ok("search/artist", SEARCH.as_bytes().to_vec());
    fetch.ok("1001/top", TOP.as_bytes().to_vec());
    fetch.ok("1001/related", RELATED.as_bytes().to_vec());
    fetch.ok("1001/albums", ALBUMS.as_bytes().to_vec());
    fetch.ok("/genre", GENRE.as_bytes().to_vec());
    fetch.ok("2002/top", one_title("Window"));
    fetch.ok("2003/top", one_title("Plains"));
    fetch.ok("2004/top", one_title("Fracture"));
    let spotify = FakeSpotify::new(&["Signal Fire", "Night Watch"])
        .with_search(
            "Aphex Twin Window",
            vec![hit("Window", "Aphex Twin", "spotify:artist:aphex")],
        )
        .with_search(
            "Seefeel Plains",
            vec![hit("Other Song", "Seefeel", "spotify:artist:seefeel")],
        )
        .with_search(
            "Clark Fracture",
            vec![hit("Fracture", "Clark Kent", "spotify:artist:clark-kent")],
        )
        .with_cards(vec![card("spotify:artist:aphex", "Aphex Twin")]);
    let outside = outside(&fetch, true);

    let (_, outcome) = lookup(&spotify, &outside, artist_seed()).await;

    match outcome {
        DiscoveryOutcome::Neighbours { cards, genre } => {
            assert_eq!(cards.len(), 1);
            assert_eq!(genre.as_deref(), Some("Rock"));
        }
        other => panic!("expected neighbours, got {other:?}"),
    }
    assert_eq!(
        spotify.asked_cards(),
        vec![vec!["spotify:artist:aphex".to_owned()]]
    );
}

#[tokio::test]
async fn every_neighbour_failing_is_unreachable() {
    let fetch = FakeFetch::new();
    fetch.ok("search/artist", SEARCH.as_bytes().to_vec());
    fetch.ok("1001/top", TOP.as_bytes().to_vec());
    fetch.ok("1001/related", RELATED.as_bytes().to_vec());
    fetch.push("2002/top", Err(FetchError::Transport));
    fetch.push("2003/top", Err(FetchError::Transport));
    fetch.push("2004/top", Err(FetchError::Transport));
    let spotify = FakeSpotify::new(&["Signal Fire", "Night Watch"]);
    let outside = outside(&fetch, true);

    let (_, outcome) = lookup(&spotify, &outside, artist_seed()).await;

    assert_eq!(outcome, DiscoveryOutcome::Unreachable);
    assert_eq!(fetch.count(), 6);
    assert_eq!(spotify.reads(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_429_blocks_the_next_lookup() {
    let fetch = FakeFetch::new();
    fetch.push(
        "search/artist",
        Err(FetchError::Status(429, Some(Duration::from_secs(60)))),
    );
    let spotify = FakeSpotify::new(&["Signal Fire", "Night Watch"]);
    let outside = outside(&fetch, true);

    let (_, first) = lookup(&spotify, &outside, artist_seed()).await;
    let (_, second) = lookup(&spotify, &outside, artist_seed()).await;

    assert_eq!(first, DiscoveryOutcome::Unreachable);
    assert_eq!(second, DiscoveryOutcome::Unreachable);
    assert_eq!(fetch.count(), 1);
}

#[tokio::test]
async fn an_error_body_is_not_an_empty_list() {
    let fetch = FakeFetch::new();
    fetch.ok("search/artist", QUOTA_ERROR.as_bytes().to_vec());
    let spotify = FakeSpotify::new(&["Signal Fire", "Night Watch"]);
    let outside = outside(&fetch, true);

    let (_, outcome) = lookup(&spotify, &outside, artist_seed()).await;

    assert_eq!(outcome, DiscoveryOutcome::Unreachable);
    assert_eq!(fetch.count(), 1);
}

#[tokio::test]
async fn a_failed_genre_read_keeps_the_cards() {
    let fetch = FakeFetch::new();
    fetch.ok("search/artist", SEARCH.as_bytes().to_vec());
    fetch.ok("1001/top", TOP.as_bytes().to_vec());
    fetch.ok("1001/related", RELATED.as_bytes().to_vec());
    fetch.push("1001/albums", Err(FetchError::Transport));
    for (id, title) in [("2002", "Window"), ("2003", "Plains"), ("2004", "Fracture")] {
        fetch.ok(&format!("{id}/top"), one_title(title));
    }
    let spotify = spotify_happy(&["Signal Fire", "Night Watch"]);
    let outside = outside(&fetch, true);

    let (_, outcome) = lookup(&spotify, &outside, artist_seed()).await;

    match outcome {
        DiscoveryOutcome::Neighbours { cards, genre } => {
            assert_eq!(cards.len(), 3);
            assert_eq!(genre, None);
        }
        other => panic!("expected neighbours, got {other:?}"),
    }
}

#[tokio::test]
async fn a_split_vote_is_silent() {
    let fetch = FakeFetch::new();
    fetch.ok("search/artist", SEARCH.as_bytes().to_vec());
    fetch.ok("1001/top", TOP.as_bytes().to_vec());
    fetch.ok("1001/related", RELATED.as_bytes().to_vec());
    fetch.ok("1001/albums", excluded_albums());
    fetch.ok("/genre", GENRE.as_bytes().to_vec());
    for (id, title) in [("2002", "Window"), ("2003", "Plains"), ("2004", "Fracture")] {
        fetch.ok(&format!("{id}/top"), one_title(title));
    }
    let spotify = spotify_happy(&["Signal Fire", "Night Watch"]);
    let outside = outside(&fetch, true);

    let (_, outcome) = lookup(&spotify, &outside, artist_seed()).await;

    match outcome {
        DiscoveryOutcome::Neighbours { cards, genre } => {
            assert_eq!(cards.len(), 3);
            assert_eq!(genre, None);
        }
        other => panic!("expected neighbours, got {other:?}"),
    }
}

#[tokio::test]
async fn an_album_seed_votes_only_for_its_own_record() {
    let fetch = FakeFetch::new();
    fetch.ok("search/artist", SEARCH.as_bytes().to_vec());
    fetch.ok("1001/top", TOP.as_bytes().to_vec());
    fetch.ok("1001/related", empty_list());
    fetch.ok("1001/albums", album_seed_albums());
    fetch.ok("/genre", GENRE.as_bytes().to_vec());
    let spotify = FakeSpotify::new(&["Signal Fire", "Night Watch"]);
    let outside = outside(&fetch, true);
    let seed = DiscoverySeed {
        kind: DiscoveryKind::Album,
        uri: "spotify:album:night-watch".to_owned(),
        name: "Day Break".to_owned(),
        anchor: Some(artist("Vela")),
    };

    let (_, outcome) = lookup(&spotify, &outside, seed).await;

    match outcome {
        DiscoveryOutcome::Neighbours { cards, genre } => {
            assert!(cards.is_empty());
            assert_eq!(genre.as_deref(), Some("Metal"));
        }
        other => panic!("expected neighbours, got {other:?}"),
    }
    assert_eq!(fetch.count(), 5);
}

#[tokio::test]
async fn a_playlist_anchors_on_its_most_recurring_first_credit() {
    let fetch = FakeFetch::new();
    route_happy(&fetch);
    let head = vec![
        hit("One", "Vela", "spotify:artist:vela"),
        hit("Two", "Other", "spotify:artist:other"),
        hit("Three", "Vela", "spotify:artist:vela"),
    ];
    let spotify = spotify_happy(&["Signal Fire", "Night Watch"]).with_head(head);
    let outside = outside(&fetch, true);
    let seed = DiscoverySeed {
        kind: DiscoveryKind::Playlist,
        uri: "spotify:playlist:mix".to_owned(),
        name: "Mix".to_owned(),
        anchor: None,
    };

    let (seed, outcome) = lookup(&spotify, &outside, seed).await;

    assert_eq!(seed.anchor, Some(artist("Vela")));
    match outcome {
        DiscoveryOutcome::Neighbours { cards, .. } => assert_eq!(cards.len(), 3),
        other => panic!("expected neighbours, got {other:?}"),
    }
}

#[tokio::test]
async fn a_playlist_with_no_recurrence_is_rejected_without_a_fetch() {
    let fetch = FakeFetch::new();
    route_happy(&fetch);
    let head = vec![
        hit("One", "Vela", "spotify:artist:vela"),
        hit("Two", "Other", "spotify:artist:other"),
    ];
    let spotify = spotify_happy(&["Signal Fire", "Night Watch"]).with_head(head);
    let outside = outside(&fetch, true);
    let seed = DiscoverySeed {
        kind: DiscoveryKind::Playlist,
        uri: "spotify:playlist:mix".to_owned(),
        name: "Mix".to_owned(),
        anchor: None,
    };

    let (seed, outcome) = lookup(&spotify, &outside, seed).await;

    assert_eq!(outcome, DiscoveryOutcome::Rejected);
    assert_eq!(seed.anchor, None);
    assert_eq!(fetch.count(), 0);
    assert_eq!(spotify.reads(), 1);
}

#[tokio::test]
async fn a_playlist_tie_goes_to_the_first_appearance() {
    let fetch = FakeFetch::new();
    route_happy(&fetch);
    let head = vec![
        hit("One", "Vela", "spotify:artist:vela"),
        hit("Two", "Other", "spotify:artist:other"),
        hit("Three", "Other", "spotify:artist:other"),
        hit("Four", "Vela", "spotify:artist:vela"),
    ];
    let spotify = spotify_happy(&["Signal Fire", "Night Watch"]).with_head(head);
    let outside = outside(&fetch, true);
    let seed = DiscoverySeed {
        kind: DiscoveryKind::Playlist,
        uri: "spotify:playlist:mix".to_owned(),
        name: "Mix".to_owned(),
        anchor: None,
    };

    let (seed, _) = lookup(&spotify, &outside, seed).await;

    assert_eq!(seed.anchor, Some(artist("Vela")));
}

#[tokio::test]
async fn one_neighbour_failing_is_dropped_and_the_rest_stay() {
    let fetch = FakeFetch::new();
    fetch.ok("search/artist", SEARCH.as_bytes().to_vec());
    fetch.ok("1001/top", TOP.as_bytes().to_vec());
    fetch.ok("1001/related", RELATED.as_bytes().to_vec());
    fetch.ok("1001/albums", ALBUMS.as_bytes().to_vec());
    fetch.ok("/genre", GENRE.as_bytes().to_vec());
    fetch.push("2002/top", Err(FetchError::Transport));
    fetch.ok("2003/top", one_title("Plains"));
    fetch.ok("2004/top", one_title("Fracture"));
    let spotify = FakeSpotify::new(&["Signal Fire", "Night Watch"])
        .with_search(
            "Seefeel Plains",
            vec![hit("Plains", "Seefeel", "spotify:artist:seefeel")],
        )
        .with_search(
            "Clark Fracture",
            vec![hit("Fracture", "Clark", "spotify:artist:clark")],
        )
        .with_cards(vec![
            card("spotify:artist:seefeel", "Seefeel"),
            card("spotify:artist:clark", "Clark"),
        ]);
    let outside = outside(&fetch, true);

    let (_, outcome) = lookup(&spotify, &outside, artist_seed()).await;

    match outcome {
        DiscoveryOutcome::Neighbours { cards, genre } => {
            assert_eq!(cards.len(), 2);
            assert_eq!(genre.as_deref(), Some("Rock"));
        }
        other => panic!("expected neighbours, got {other:?}"),
    }
}

#[tokio::test]
async fn a_429_during_verify_stops_before_the_next_candidate() {
    let fetch = FakeFetch::new();
    fetch.ok("search/artist", SEARCH.as_bytes().to_vec());
    fetch.push(
        "1001/top",
        Err(FetchError::Status(429, Some(Duration::from_secs(60)))),
    );
    fetch.ok("1002/top", TOP.as_bytes().to_vec());
    let spotify = FakeSpotify::new(&["Signal Fire", "Night Watch"]);
    let outside = outside(&fetch, true);

    let (_, outcome) = lookup(&spotify, &outside, artist_seed()).await;

    assert_eq!(outcome, DiscoveryOutcome::Unreachable);
    assert_eq!(fetch.count(), 2);
}

#[tokio::test]
async fn a_429_during_a_neighbour_stops_the_lookup() {
    let fetch = FakeFetch::new();
    fetch.ok("search/artist", SEARCH.as_bytes().to_vec());
    fetch.ok("1001/top", TOP.as_bytes().to_vec());
    fetch.ok("1001/related", RELATED.as_bytes().to_vec());
    fetch.push(
        "2002/top",
        Err(FetchError::Status(429, Some(Duration::from_secs(60)))),
    );
    fetch.ok("2003/top", one_title("Plains"));
    fetch.ok("2004/top", one_title("Fracture"));
    let spotify = FakeSpotify::new(&["Signal Fire", "Night Watch"]);
    let outside = outside(&fetch, true);

    let (_, outcome) = lookup(&spotify, &outside, artist_seed()).await;

    assert_eq!(outcome, DiscoveryOutcome::Unreachable);
    assert_eq!(fetch.count(), 4);
}

#[tokio::test]
async fn the_call_count_stays_within_the_stated_worst_case() {
    let fetch = FakeFetch::new();
    let search = format!(
        r#"{{"_synthetic":"synthetic","data":[{},{},{}],"next":null,"total":3}}"#,
        r#"{"id":3001,"name":"Vela"}"#,
        r#"{"id":3002,"name":"Vela"}"#,
        r#"{"id":3003,"name":"Vela"}"#,
    );
    fetch.ok("search/artist", search.into_bytes());
    fetch.ok("3001/top", no_overlap());
    fetch.ok("3002/top", no_overlap());
    fetch.ok("3003/top", TOP.as_bytes().to_vec());
    let related = (0..8)
        .map(|index| format!(r#"{{"id":{},"name":"Neighbour {index}"}}"#, 4000 + index))
        .collect::<Vec<_>>()
        .join(",");
    fetch.ok(
        "3003/related",
        format!(r#"{{"_synthetic":"synthetic","data":[{related}],"next":null,"total":8}}"#)
            .into_bytes(),
    );
    for index in 0..8 {
        fetch.ok(
            &format!("{}/top", 4000 + index),
            one_title(&format!("Track {index}")),
        );
    }
    fetch.ok("3003/albums", ALBUMS.as_bytes().to_vec());
    fetch.ok("/genre", GENRE.as_bytes().to_vec());
    let spotify = FakeSpotify::new(&["Signal Fire", "Night Watch", "Third", "Fourth"]);
    let outside = outside(&fetch, true);

    let (_, outcome) = lookup(&spotify, &outside, artist_seed()).await;

    match outcome {
        DiscoveryOutcome::Neighbours { cards, genre } => {
            assert!(cards.is_empty());
            assert_eq!(genre.as_deref(), Some("Rock"));
        }
        other => panic!("expected neighbours, got {other:?}"),
    }
    // 1 identify + 3 verify + 1 related + 8 neighbour tops + 1 albums + 1 genre
    // table: the row's stated worst case.
    assert_eq!(fetch.count(), 15);
    assert!(spotify.asked_cards().is_empty());
}
