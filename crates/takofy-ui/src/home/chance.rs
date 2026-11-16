//! The shelf at chance: one item drawn out of the library, and the four cards
//! read from its station.
//!
//! Pure: the deck is the four candidate lists in a drawn order, the station is
//! whatever the read answered, and the shelf keeps what it drew between presses.
//! The page sends the reads; what they mean is decided here (#96).

use std::collections::{HashMap, HashSet};

use takofy_model::home::{HomeAnswer, HomeAsk, SeedKind, Station, StationSeed};
use takofy_model::{FollowedArtist, LikedTrack, PlaylistEntry, SavedAlbum, SavedAt, TrackEntry};

use crate::message::Failure;
use crate::shuffle;

use super::seed::{Seed, below};

/// How many cards the shelf draws from the station it read.
pub const CARDS: usize = 4;

/// How many seeds one press may try before it draws nothing (#96).
///
/// A bag is reshuffled when it is spent, so a library of songs no read can name
/// has no natural end: this is the end, and it covers the label-miss path as
/// well as a press.
pub const SKIPS: usize = 8;

/// How many reads one press may send for the seed it draws: a song's name and
/// its station, or the station alone for a seed already named (#96).
pub const READS_PER_PRESS: usize = 2;

/// The salt each kind's bag is drawn from, which the kind's index is added to.
const SALT: u64 = 0x0044_5241_5700;

/// The salt the kind order is drawn from, told apart from every bag.
const KIND_SALT: u64 = 0x0044_5241_5701;

/// The salt the cards on the shelf are chosen with.
const CARD_SALT: u64 = 0x0044_5241_5702;

/// One thing the deck can draw, with the date its own list gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub kind: SeedKind,
    /// The URI that names it to the read and on the page.
    pub uri: String,
    /// Its name as the list gave it; a song is named by its label read instead.
    pub name: String,
    /// When the library saved it, when the list dated it.
    pub at: Option<SavedAt>,
}

/// The library's own things the deck draws from, one list per kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Candidates<'a> {
    pub playlists: &'a [PlaylistEntry],
    pub artists: &'a [FollowedArtist],
    pub albums: &'a [SavedAlbum],
    pub songs: &'a [LikedTrack],
}

/// One kind's candidates, their drawn order, and how far the deck walked it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Drawer {
    /// The candidates this kind offers, deduplicated by URI.
    candidates: Vec<Candidate>,
    /// A permutation of `candidates`' indices.
    order: Vec<usize>,
    /// How many positions of `order` the deck has drawn.
    at: usize,
}

impl Drawer {
    /// One kind's bag, deduplicated by URI and laid in the seed's order.
    fn laid(items: Vec<Candidate>, seed: Seed, index: usize) -> Self {
        let mut seen = HashSet::new();
        let candidates: Vec<Candidate> = items
            .into_iter()
            .filter(|candidate| seen.insert(candidate.uri.clone()))
            .collect();
        let order = shuffle::order(candidates.len(), seed.split(SALT + index as u64).state());
        Self {
            candidates,
            order,
            at: 0,
        }
    }
}

/// One candidate of `kind`, when the list it came from names it.
fn named_candidate(
    kind: SeedKind,
    uri: &str,
    name: &str,
    named: bool,
    at: Option<SavedAt>,
) -> Option<Candidate> {
    (named && !name.is_empty()).then(|| Candidate {
        kind,
        uri: uri.to_owned(),
        name: name.to_owned(),
        at,
    })
}

/// The four lists in a drawn order, and the kind order they are drawn in.
///
/// A bag is drawn without replacement and reshuffled once it is spent, so a
/// press never runs out of candidates while the library holds one, and the kind
/// the last press took is not the kind the next press takes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Deck {
    seed: Seed,
    /// One drawer per [`SeedKind`], in the model's own order.
    drawers: Vec<Drawer>,
    /// The kinds' indices in the order a draw visits them.
    kinds: Vec<usize>,
    /// Where the next draw starts in `kinds`.
    cursor: usize,
    /// How many bags have been reshuffled, which salts the new order.
    reshuffles: u64,
    /// The candidate the shelf is drawing now.
    current: Option<Candidate>,
}

impl Deck {
    /// Fills the four bags from `candidates` for the seed the page drew.
    ///
    /// Only a thing with a name of its own can seed a station, so an unnamed
    /// list gives the deck nothing; a song is named by the read it costs.
    pub fn fill(&mut self, candidates: &Candidates<'_>, seed: Seed) {
        self.seed = seed;
        self.current = None;
        self.cursor = 0;
        self.reshuffles = 0;
        let bags = [
            candidates
                .playlists
                .iter()
                .filter_map(|entry| {
                    named_candidate(
                        SeedKind::Playlist,
                        &entry.uri,
                        &entry.name,
                        entry.named,
                        entry.saved_at,
                    )
                })
                .collect(),
            candidates
                .artists
                .iter()
                .filter_map(|entry| {
                    named_candidate(
                        SeedKind::Artist,
                        &entry.uri,
                        &entry.name,
                        entry.named,
                        entry.followed_at,
                    )
                })
                .collect(),
            candidates
                .albums
                .iter()
                .filter_map(|entry| {
                    named_candidate(
                        SeedKind::Album,
                        &entry.uri,
                        &entry.name,
                        entry.named,
                        entry.saved_at,
                    )
                })
                .collect(),
            // A song has no name of its own here: the label read names it.
            candidates
                .songs
                .iter()
                .map(|song| Candidate {
                    kind: SeedKind::Song,
                    uri: song.uri.clone(),
                    name: String::new(),
                    at: song.saved_at,
                })
                .collect(),
        ];
        self.drawers = bags
            .into_iter()
            .enumerate()
            .map(|(index, items)| Drawer::laid(items, seed, index))
            .collect();
        self.kinds = shuffle::order(self.drawers.len(), seed.split(KIND_SALT).state());
    }

    /// Draws the next candidate, in the drawn kind order.
    pub fn draw(&mut self) -> Option<Candidate> {
        let kinds = self.kinds.len();
        if kinds == 0 {
            return None;
        }
        for _ in 0..kinds {
            let slot = self.cursor % kinds;
            self.cursor = slot + 1;
            let Some(kind) = self.kinds.get(slot).copied() else {
                continue;
            };
            let Some(drawer) = self.drawers.get_mut(kind) else {
                continue;
            };
            if drawer.candidates.is_empty() {
                continue;
            }
            if drawer.at >= drawer.order.len() {
                self.reshuffles = self.reshuffles.saturating_add(1);
                let salt = SALT + kind as u64 + self.reshuffles;
                drawer.order =
                    shuffle::order(drawer.candidates.len(), self.seed.split(salt).state());
                drawer.at = 0;
            }
            let Some(at) = drawer.order.get(drawer.at).copied() else {
                continue;
            };
            drawer.at += 1;
            let Some(candidate) = drawer.candidates.get(at).cloned() else {
                continue;
            };
            self.current = Some(candidate.clone());
            return Some(candidate);
        }
        None
    }

    /// The candidate the deck last drew, which is the seed in flight.
    pub fn current(&self) -> Option<&Candidate> {
        self.current.as_ref()
    }
}

/// Which of the two things a row of a station is drawn as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardKind {
    /// The row itself, a track.
    Track,
    /// The row's album, which the row names.
    Album,
}

/// One card on the shelf, by its row in the station and the kind it is drawn as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardAt {
    pub kind: CardKind,
    /// The row's index in the station it came from.
    pub row: usize,
}

/// One card as the shelf paints it, borrowed from the rows it was drawn out of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Card<'a> {
    pub kind: CardKind,
    pub row: &'a TrackEntry,
}

/// The cards a station yields: one pass over its rows, in the station's order.
///
/// A row is taken when it is named, is not the seed itself, names an album that
/// is not on the shelf yet, and its own URI is new. Taking a card takes its
/// album with it, so no two cards are one album and a track never sits beside
/// its own album (#96).
pub fn draw_cards(rows: &[TrackEntry], seed: &StationSeed, draw: Seed) -> Vec<CardAt> {
    let mut state = draw.state();
    let mut cards: Vec<CardAt> = Vec::with_capacity(CARDS);
    let mut uris: HashSet<&str> = HashSet::new();
    let mut albums: HashSet<&str> = HashSet::new();
    for (row, entry) in rows.iter().enumerate() {
        if cards.len() == CARDS {
            break;
        }
        let Some(album) = entry.album_uri.as_deref() else {
            continue;
        };
        if !entry.named || entry.uri == seed.uri || album == seed.uri {
            continue;
        }
        if uris.contains(entry.uri.as_str()) || albums.contains(album) {
            continue;
        }
        let kind = match below(&mut state, 2) {
            0 => CardKind::Track,
            _ => CardKind::Album,
        };
        uris.insert(entry.uri.as_str());
        albums.insert(album);
        cards.push(CardAt { kind, row });
    }
    cards
}

/// What a name read answered for a song, which is all its station needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The name the head draws.
    pub name: String,
    /// The album the head opens, when the read named one.
    pub album_uri: Option<String>,
}

/// One read the shelf wants the page to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// The seed's name, before its station can be read.
    Name(String),
    /// One station, read from its seed.
    Station(StationSeed),
}

/// What the shelf's read is doing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Read {
    /// No read is owed and none is out.
    #[default]
    Idle,
    /// A read this shelf owes the page, which no frame has sent yet.
    Owed(Ask),
    /// A read this shelf sent, waiting on its answer.
    Out(Ask),
}

/// The draw the shelf last landed: the item it drew and the cards read of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    /// The drawn item, named by the read it needed.
    pub seed: Candidate,
    /// The station's rows, which the cards index into.
    pub rows: Vec<TrackEntry>,
    /// The cards, ascending by their row.
    pub cards: Vec<CardAt>,
}

/// What the shelf paints this frame, borrowed from the draw it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draw<'a> {
    /// The item the shelf drew, as its head names it. Owned, because a seed is
    /// nameable before its station answers.
    pub seed: Candidate,
    /// The album a song seed's name read named, which its head opens.
    pub album_uri: Option<String>,
    /// The cards the station yielded, in station order.
    pub cards: Vec<Card<'a>>,
    /// How many of them are tracks, for the ledger.
    pub tracks: usize,
    /// How many of them are albums, for the ledger.
    pub albums: usize,
    /// Whether a read the shelf sent is still out.
    pub pending: bool,
    /// Whether the last draw found nothing to draw.
    pub nothing: bool,
    /// The read that failed, when one did.
    pub failed: Option<Failure>,
    /// When the last press was made, for the square's sweep.
    pub pressed_ms: Option<u64>,
}

/// The deck, the four on screen, and what is in flight.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct State {
    /// The page's own seed, which every draw of this open comes from.
    seed: Seed,
    deck: Option<Deck>,
    read: Read,
    shown: Option<Shown>,
    /// The songs a name read could not name, so a press walks past them.
    unnameable: HashSet<String>,
    /// The songs a name read named, by URI.
    names: HashMap<String, Found>,
    failed: Option<Failure>,
    nothing: bool,
    pressed_ms: Option<u64>,
    /// How many seeds the draw in flight has tried, bounded by [`SKIPS`].
    skips: usize,
}

impl State {
    /// Starts this open from the page's seed.
    pub fn open(&mut self, seed: Seed) {
        self.seed = seed;
        self.forget();
    }

    /// Builds the deck once the feeds have settled, and starts the first draw.
    pub fn fill(&mut self, candidates: &Candidates<'_>) {
        if self.deck.is_some() {
            return;
        }
        let mut deck = Deck::default();
        deck.fill(candidates, self.seed);
        self.deck = Some(deck);
        self.skips = 0;
        self.walk();
    }

    /// The read the shelf owes the page, which no frame has sent yet.
    pub fn owed(&self) -> Option<Ask> {
        match &self.read {
            Read::Owed(ask) => Some(ask.clone()),
            Read::Idle | Read::Out(_) => None,
        }
    }

    /// Claims `ask`, so two frames in one press cannot send it twice.
    pub fn asked(&mut self, ask: Ask) {
        if self.owed().as_ref() == Some(&ask) {
            self.read = Read::Out(ask);
        }
    }

    /// The ask the shelf is waiting on.
    ///
    /// It is the test seam: a station ask and a one-track labels ask have the
    /// same shape whichever shelf sent them, so a fixture cannot attribute one
    /// by shape and asks the shelf itself instead.
    pub fn in_flight(&self) -> Option<Ask> {
        match &self.read {
            Read::Out(ask) => Some(ask.clone()),
            Read::Idle | Read::Owed(_) => None,
        }
    }

    /// Whether a read this shelf sent is still out.
    pub fn pending(&self) -> bool {
        !matches!(self.read, Read::Idle)
    }

    /// Whether the deck has been built this open.
    pub fn filled(&self) -> bool {
        self.deck.is_some()
    }

    /// Whether the last draw found nothing to draw.
    pub fn nothing(&self) -> bool {
        self.nothing
    }

    /// The read that failed, when one did.
    pub fn failure(&self) -> Option<Failure> {
        self.failed
    }

    /// Draws the next item, keeping the four on screen until it lands.
    ///
    /// A press while a read is out is dropped: the sweep on the square says why,
    /// and a queue of presses would be a second state machine (#96).
    pub fn press(&mut self, now_ms: u64) -> bool {
        if self.deck.is_none() || !matches!(self.read, Read::Idle) {
            return false;
        }
        self.pressed_ms = Some(now_ms);
        self.skips = 0;
        self.nothing = false;
        // A press is a retry, so the sentence the last read left goes with it.
        self.failed = None;
        self.walk();
        !matches!(self.read, Read::Idle)
    }

    /// Folds one answer the shelf asked for into its state.
    pub fn land(&mut self, answer: &HomeAnswer) {
        match answer {
            HomeAnswer::Station(station) => self.land_station(station),
            HomeAnswer::Labelled { asked, named } => self.land_labelled(asked, named),
            _ => {}
        }
    }

    /// One station landing, when it answers the seed this shelf is waiting on.
    fn land_station(&mut self, station: &Station) {
        let Read::Out(Ask::Station(seed)) = self.read.clone() else {
            return;
        };
        if seed.uri != station.seed.uri {
            return;
        }
        let Some(candidate) = self.deck.as_ref().and_then(Deck::current).cloned() else {
            return;
        };
        if candidate.uri != seed.uri {
            return;
        }
        let mut sown = candidate;
        if let Some(found) = self.names.get(&seed.uri) {
            sown.name = found.name.clone();
        }
        let cards = draw_cards(&station.rows, &seed, self.seed.split(CARD_SALT));
        self.nothing = cards.is_empty();
        self.shown = Some(Shown {
            seed: sown,
            rows: station.rows.clone(),
            cards,
        });
        self.failed = None;
        self.read = Read::Idle;
    }

    /// One name read landing: the seed's station is owed, or it is passed over.
    fn land_labelled(&mut self, asked: &[String], named: &[TrackEntry]) {
        let Read::Out(Ask::Name(uri)) = self.read.clone() else {
            return;
        };
        if !asked.contains(&uri) {
            return;
        }
        let found = named
            .iter()
            .find(|track| track.uri == uri)
            .filter(|track| track.named)
            .map(|track| Found {
                name: track.title.clone(),
                album_uri: track.album_uri.clone(),
            });
        match found {
            Some(found) => {
                self.names.insert(uri.clone(), found);
                self.read = Read::Owed(Ask::Station(StationSeed {
                    kind: SeedKind::Song,
                    uri,
                }));
            }
            None => {
                self.unnameable.insert(uri);
                self.read = Read::Idle;
                self.walk();
            }
        }
    }

    /// Records that the read `ask` named failed, so the foot line says so.
    pub fn fail(&mut self, ask: &HomeAsk, failure: Failure) {
        let matched = match (&self.read, ask) {
            (Read::Out(Ask::Name(uri)), HomeAsk::Labels { tracks }) => tracks.contains(uri),
            (Read::Out(Ask::Station(seed)), HomeAsk::Station { seed: asked }) => {
                seed.uri == asked.uri
            }
            _ => false,
        };
        if !matched {
            return;
        }
        self.failed = Some(failure);
        self.read = Read::Idle;
        // A song whose name read failed has no head to paint, and the square is the
        // shelf's only control: with nothing on screen worth keeping, walk on (#96).
        if self.shown.is_none() && matches!(ask, HomeAsk::Labels { .. }) {
            self.walk();
        }
    }

    /// Drops the deck, the store and the four, so a retry reads everything again.
    pub fn forget(&mut self) {
        self.deck = None;
        self.read = Read::Idle;
        self.shown = None;
        self.unnameable.clear();
        self.names.clear();
        self.failed = None;
        self.nothing = false;
        self.pressed_ms = None;
        self.skips = 0;
    }

    /// The draw the shelf paints, or `None` while there is no seed to name.
    ///
    /// A seed the deck has already reached is nameable before its station answers,
    /// so a first draw that fails still paints its head and the sentence on the
    /// foot — the square is the only control here, and a failure that takes it
    /// away leaves nothing to press (#96).
    pub fn draw(&self) -> Option<Draw<'_>> {
        let Some(shown) = self.shown.as_ref() else {
            return self.unlanded();
        };
        let cards: Vec<Card<'_>> = shown
            .cards
            .iter()
            .filter_map(|at| {
                Some(Card {
                    kind: at.kind,
                    row: shown.rows.get(at.row)?,
                })
            })
            .collect();
        let tracks = cards
            .iter()
            .filter(|card| card.kind == CardKind::Track)
            .count();
        let albums = cards.len() - tracks;
        Some(Draw {
            seed: shown.seed.clone(),
            album_uri: self.album_of(&shown.seed.uri),
            cards,
            tracks,
            albums,
            pending: self.pending(),
            nothing: self.nothing,
            failed: self.failed,
            pressed_ms: self.pressed_ms,
        })
    }

    /// The draw for a seed the deck reached but whose station has not answered.
    ///
    /// `None` while the seed has no name yet: a song is named by its own read, and a
    /// shelf whose head could not name itself has nothing to paint.
    fn unlanded(&self) -> Option<Draw<'_>> {
        let candidate = self.deck.as_ref()?.current()?;
        let mut sown = candidate.clone();
        if let Some(found) = self.names.get(&candidate.uri) {
            sown.name = found.name.clone();
        }
        if sown.name.is_empty() {
            return None;
        }
        Some(Draw {
            seed: sown,
            album_uri: self.album_of(&candidate.uri),
            cards: Vec::new(),
            tracks: 0,
            albums: 0,
            pending: self.pending(),
            nothing: self.nothing,
            failed: self.failed,
            pressed_ms: self.pressed_ms,
        })
    }

    /// The album a song seed's name read named, when it named one.
    fn album_of(&self, uri: &str) -> Option<String> {
        self.names
            .get(uri)
            .and_then(|found| found.album_uri.clone())
    }

    /// Takes the next candidate the deck offers, or says nothing was drawn.
    ///
    /// The one place a seed is passed over, so [`SKIPS`] bounds the press and the
    /// label-miss path alike.
    fn walk(&mut self) {
        while self.skips < SKIPS {
            let Some(candidate) = self.deck.as_mut().and_then(Deck::draw) else {
                self.nothing = self.shown.is_none();
                self.read = Read::Idle;
                return;
            };
            self.skips += 1;
            let seed = StationSeed {
                kind: candidate.kind,
                uri: candidate.uri,
            };
            if seed.kind == SeedKind::Song && !self.names.contains_key(&seed.uri) {
                if self.unnameable.contains(&seed.uri) {
                    continue;
                }
                self.read = Read::Owed(Ask::Name(seed.uri));
                return;
            }
            self.read = Read::Owed(Ask::Station(seed));
            return;
        }
        // The four on screen stay when the walk runs out on them, so the shelf has
        // nothing to announce; only a shelf that never drew says it drew nothing.
        self.nothing = self.shown.is_none();
        self.read = Read::Idle;
    }
}

/// How far a press's sweep has run, as `0.0..=1.0`, or `None` outside a press.
pub fn waited(since_ms: Option<u64>, now_ms: u64, span_ms: u64) -> Option<f32> {
    let since = since_ms?;
    let span = span_ms.max(1);
    let elapsed = now_ms.saturating_sub(since);
    Some((elapsed as f32 / span as f32).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests;
