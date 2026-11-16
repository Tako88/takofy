//! Test doubles shared by the discovery tests.

use std::collections::VecDeque;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use takofy_core::ReadFailed;
use takofy_model::TrackEntry;
use takofy_model::home::{DiscoveryArtist, DiscoveryCard};

use super::lookup::Spotify;
use super::{Fetch, FetchError, Outside};

pub(crate) const SEARCH: &str = include_str!("fixtures/search_artist.json");
pub(crate) const TOP: &str = include_str!("fixtures/top.json");
pub(crate) const RELATED: &str = include_str!("fixtures/related.json");
pub(crate) const ALBUMS: &str = include_str!("fixtures/albums.json");
pub(crate) const GENRE: &str = include_str!("fixtures/genre.json");

/// A fetch that answers scripted bodies and counts every call.
#[derive(Clone, Default)]
pub(crate) struct FakeFetch {
    inner: Arc<Inner>,
}

type Answer = Result<Vec<u8>, FetchError>;
type Routes = Mutex<Vec<(String, VecDeque<Answer>)>>;

#[derive(Default)]
struct Inner {
    routes: Routes,
    calls: Mutex<Vec<String>>,
}

impl FakeFetch {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push(&self, needle: &str, response: Result<Vec<u8>, FetchError>) {
        let mut routes = self.inner.routes.lock().expect("routes");
        match routes.iter_mut().find(|(name, _)| name == needle) {
            Some((_, queue)) => queue.push_back(response),
            None => routes.push((needle.to_owned(), VecDeque::from([response]))),
        }
    }

    pub(crate) fn ok(&self, needle: &str, body: Vec<u8>) {
        self.push(needle, Ok(body));
    }

    pub(crate) fn count(&self) -> usize {
        self.inner.calls.lock().expect("calls").len()
    }

    fn response_for(&self, path: &str) -> Result<Vec<u8>, FetchError> {
        let mut routes = self.inner.routes.lock().expect("routes");
        let Some((_, queue)) = routes
            .iter_mut()
            .find(|(needle, _)| path.contains(needle.as_str()))
        else {
            return Err(FetchError::Transport);
        };
        queue.pop_front().unwrap_or(Err(FetchError::Transport))
    }
}

impl Fetch for FakeFetch {
    fn get(
        &self,
        path_and_query: &str,
    ) -> impl Future<Output = Result<Vec<u8>, FetchError>> + Send {
        self.inner
            .calls
            .lock()
            .expect("calls")
            .push(path_and_query.to_owned());
        let response = self.response_for(path_and_query);
        async move { response }
    }
}

/// A Spotify that answers constructed lists and counts its reads.
pub(crate) struct FakeSpotify {
    titles: Vec<String>,
    head: Vec<TrackEntry>,
    searches: Mutex<Vec<(String, Vec<TrackEntry>)>>,
    cards: Vec<DiscoveryCard>,
    reads: AtomicUsize,
    asked_cards: Mutex<Vec<Vec<String>>>,
    /// The reads this fake answers with an error instead.
    failing: Vec<Call>,
    /// The queries whose search answers with an error.
    failing_searches: Mutex<Vec<String>>,
}

/// One of the four reads a lookup asks Spotify for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Call {
    Titles,
    Head,
    Search,
    Cards,
}

impl FakeSpotify {
    pub(crate) fn new(titles: &[&str]) -> Self {
        Self {
            titles: titles.iter().map(|title| (*title).to_owned()).collect(),
            head: Vec::new(),
            searches: Mutex::new(Vec::new()),
            cards: Vec::new(),
            reads: AtomicUsize::new(0),
            asked_cards: Mutex::new(Vec::new()),
            failing: Vec::new(),
            failing_searches: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn failing(mut self, call: Call) -> Self {
        self.failing.push(call);
        self
    }

    pub(crate) fn failing_search(self, needle: &str) -> Self {
        self.failing_searches
            .lock()
            .expect("failing searches")
            .push(needle.to_owned());
        self
    }

    fn fails(&self, call: Call) -> bool {
        self.failing.contains(&call)
    }

    pub(crate) fn with_head(mut self, head: Vec<TrackEntry>) -> Self {
        self.head = head;
        self
    }

    pub(crate) fn with_search(self, needle: &str, hits: Vec<TrackEntry>) -> Self {
        self.searches
            .lock()
            .expect("searches")
            .push((needle.to_owned(), hits));
        self
    }

    pub(crate) fn with_cards(mut self, cards: Vec<DiscoveryCard>) -> Self {
        self.cards = cards;
        self
    }

    pub(crate) fn reads(&self) -> usize {
        self.reads.load(Ordering::Relaxed)
    }

    pub(crate) fn asked_cards(&self) -> Vec<Vec<String>> {
        self.asked_cards.lock().expect("cards").clone()
    }
}

impl Spotify for FakeSpotify {
    fn seed_titles(
        &self,
        _artist: &DiscoveryArtist,
    ) -> impl Future<Output = Result<Vec<String>, ReadFailed>> + Send {
        self.reads.fetch_add(1, Ordering::Relaxed);
        let titles = self.titles.clone();
        let fails = self.fails(Call::Titles);
        async move { if fails { Err(ReadFailed) } else { Ok(titles) } }
    }

    fn playlist_head(
        &self,
        _uri: &str,
    ) -> impl Future<Output = Result<Vec<TrackEntry>, ReadFailed>> + Send {
        self.reads.fetch_add(1, Ordering::Relaxed);
        let head = self.head.clone();
        let fails = self.fails(Call::Head);
        async move { if fails { Err(ReadFailed) } else { Ok(head) } }
    }

    fn search(
        &self,
        query: &str,
    ) -> impl Future<Output = Result<Vec<TrackEntry>, ReadFailed>> + Send {
        self.reads.fetch_add(1, Ordering::Relaxed);
        let hits = self
            .searches
            .lock()
            .expect("searches")
            .iter()
            .find(|(needle, _)| query.contains(needle.as_str()))
            .map(|(_, hits)| hits.clone())
            .unwrap_or_default();
        let fails = self.fails(Call::Search)
            || self
                .failing_searches
                .lock()
                .expect("failing searches")
                .iter()
                .any(|needle| query.contains(needle.as_str()));
        async move { if fails { Err(ReadFailed) } else { Ok(hits) } }
    }

    fn cards(
        &self,
        uris: &[String],
    ) -> impl Future<Output = Result<Vec<DiscoveryCard>, ReadFailed>> + Send {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.asked_cards.lock().expect("cards").push(uris.to_vec());
        let cards = self.cards.clone();
        let fails = self.fails(Call::Cards);
        async move { if fails { Err(ReadFailed) } else { Ok(cards) } }
    }
}

pub(crate) fn artist(name: &str) -> DiscoveryArtist {
    DiscoveryArtist {
        uri: format!("spotify:artist:{}", name.to_lowercase().replace(' ', "-")),
        name: name.to_owned(),
    }
}

pub(crate) fn hit(title: &str, name: &str, uri: &str) -> TrackEntry {
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

pub(crate) fn card(uri: &str, name: &str) -> DiscoveryCard {
    DiscoveryCard {
        artist: DiscoveryArtist {
            uri: uri.to_owned(),
            name: name.to_owned(),
        },
        outside: None,
        portrait: None,
        tracks: Vec::new(),
        latest_year: None,
    }
}

pub(crate) fn one_title(title: &str) -> Vec<u8> {
    format!(
        r#"{{"_synthetic":"synthetic","data":[{{"id":1,"title":"{title}"}}],"next":null,"total":1}}"#
    )
    .into_bytes()
}

/// The outside bodies one happy first-ring lookup reads.
pub(crate) fn route_happy(fetch: &FakeFetch) {
    fetch.ok("search/artist", SEARCH.as_bytes().to_vec());
    fetch.ok("1001/top", TOP.as_bytes().to_vec());
    fetch.ok("1001/related", RELATED.as_bytes().to_vec());
    fetch.ok("1001/albums", ALBUMS.as_bytes().to_vec());
    fetch.ok("/genre", GENRE.as_bytes().to_vec());
    for (id, title) in [("2002", "Window"), ("2003", "Plains"), ("2004", "Fracture")] {
        fetch.ok(&format!("{id}/top"), one_title(title));
    }
}

/// The Spotify reads one happy first-ring lookup answers.
pub(crate) fn spotify_happy(titles: &[&str]) -> FakeSpotify {
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

pub(crate) fn outside(fetch: &FakeFetch, gate: bool) -> Outside<FakeFetch> {
    Outside::new(fetch.clone(), Arc::new(AtomicBool::new(gate)))
}
