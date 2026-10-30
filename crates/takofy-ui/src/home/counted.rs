//! The counted shelf: the artists the library holds the most of.
//!
//! Pure: the ranking is a count over the artists every saved album and every
//! labelled liked song names, and the draw is a subset of the ranking's top
//! pool. Nothing here reads a `Library`, so every state is reachable in a test.

use std::collections::{HashMap, HashSet};

use takofy_model::CoverSet;
use takofy_model::home::{HomeAnswer, HomeAsk};

use crate::message::Failure;

use super::Seed;
use super::draw::sample;

/// How much of the ranking the draw may take from: the shelf never shows an
/// artist outside its own top pool, however many the library holds.
pub const POOL: usize = 12;

/// How many tiles the shelf draws from that pool.
pub const DRAWN: usize = 6;

/// The share of a label read that may come back unnamed, in percent.
pub const UNLABELLED_MAX_PERCENT: usize = 1;

/// The salt that keeps this shelf's draw off the page's other shelves.
pub(super) const SALT: u64 = 0x0043_4F55_4E54;

/// One artist one item of the library names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Holding<'a> {
    /// The artist's URI, which is what the ranking counts and the tile opens.
    pub artist: &'a str,
    /// The name the same item gives that artist.
    pub name: &'a str,
}

/// One artist of the ranking: who it is, and what it is called.
///
/// The count is not part of it: the head prints no number, and the order
/// already says which artist the library holds most of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ranked<'a> {
    pub uri: &'a str,
    pub name: &'a str,
}

/// One artist tile, borrowed from the library's own lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tile<'a> {
    pub uri: &'a str,
    pub name: &'a str,
    /// The face the followed-artists read gives this artist, when it gives one.
    pub portrait: Option<&'a CoverSet>,
}

/// What the counted shelf draws: its tiles, in ranking order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Tiles<'a> {
    pub artists: Vec<Tile<'a>>,
}

/// `pairs` as holdings: each artist once, in the order the item named them.
///
/// One item holds an artist once however often its row repeats the credit, so a
/// track credited to the same artist twice is one holding.
pub fn one_each<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<Holding<'a>> {
    let mut seen: HashSet<&str> = HashSet::new();
    pairs
        .into_iter()
        .filter(|(artist, _)| seen.insert(artist))
        .map(|(artist, name)| Holding { artist, name })
        .collect()
}

/// The holdings ranked by how many items name them, most first.
///
/// A tie keeps the order the holdings came in, so the library always ranks the
/// same way and two opens cannot reshuffle an untouched shelf.
pub fn rank<'a>(holdings: &[Holding<'a>]) -> Vec<Ranked<'a>> {
    let mut order: Vec<Ranked<'a>> = Vec::new();
    let mut counts: Vec<usize> = Vec::new();
    let mut at: HashMap<&str, usize> = HashMap::new();
    for holding in holdings {
        if let Some(&index) = at.get(holding.artist) {
            if let Some(count) = counts.get_mut(index) {
                *count += 1;
            }
            continue;
        }
        at.insert(holding.artist, order.len());
        order.push(Ranked {
            uri: holding.artist,
            name: holding.name,
        });
        counts.push(1);
    }
    let mut counted: Vec<(usize, Ranked<'a>)> = counts.into_iter().zip(order).collect();
    counted.sort_by(|(left, _), (right, _)| right.cmp(left));
    counted.into_iter().map(|(_, artist)| artist).collect()
}

/// The ranking positions this open draws, ascending, all from the top [`POOL`].
pub fn pick(ranked_len: usize, seed: Seed) -> Vec<usize> {
    sample(ranked_len.min(POOL), seed, DRAWN)
}

/// Whether a label read named enough of what it asked for to rank.
///
/// A miss answers with the track's own URI and no artists, so a read that
/// failed quietly is indistinguishable from a thin library by its own report.
/// Counting the misses is what tells the two apart.
///
/// One miss is allowed outright, whatever the library's size: a shelf that
/// failed on a single unnamed like would fail a small library forever, since a
/// retry re-reads the same data. Above that the percentage decides.
pub fn readable(asked: usize, unlabelled: usize) -> bool {
    unlabelled <= 1
        || unlabelled.saturating_mul(100) <= asked.saturating_mul(UNLABELLED_MAX_PERCENT)
}

/// What this open's label read is doing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Read {
    /// No ask has been sent this open.
    #[default]
    Idle,
    /// Sent, waiting for the answer that names exactly these URIs.
    Asking(Vec<String>),
    /// The labels this open draws from are known.
    Landed,
    /// The answer could not be used.
    Failed(Failure),
}

/// What the counted shelf holds between opens.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct State {
    /// This open's own draw of the ranking.
    seed: Seed,
    /// The artists each answered liked song names, by the song's URI.
    labels: HashMap<String, Vec<(String, String)>>,
    /// The face read for each ranked artist this open asked about; `None` is an
    /// artist the wire gave no portrait this client can draw.
    portraits: HashMap<String, Option<CoverSet>>,
    /// The artists whose face this open asked for, so none is asked twice.
    asked: HashSet<String>,
    read: Read,
}

impl State {
    /// Starts this open, from the page's own seed, and forgets the last read.
    pub fn open(&mut self, seed: Seed) {
        self.seed = seed.split(SALT);
        self.labels.clear();
        self.read = Read::Idle;
        self.forget_faces();
    }

    /// The draw this open picks the ranking with.
    pub fn seed(&self) -> Seed {
        self.seed
    }

    /// Whether this open's ask is still owed.
    pub fn owed(&self) -> bool {
        matches!(self.read, Read::Idle)
    }

    /// Marks the ask this open sends, so no later frame sends another.
    pub fn mark_asked(&mut self, tracks: Vec<String>) {
        self.read = Read::Asking(tracks);
    }

    /// Settles this open without a read: nothing is liked, so the shelf has no
    /// labels to wait for and must not hold Home's page open for nothing.
    pub fn nothing_to_ask(&mut self) {
        self.read = Read::Landed;
    }

    /// The artists the answer named for `track`, when it named any.
    pub fn artists(&self, track: &str) -> Option<&[(String, String)]> {
        self.labels.get(track).map(Vec::as_slice)
    }

    /// Whether the labels this open draws from are known.
    pub fn settled(&self) -> bool {
        matches!(self.read, Read::Landed)
    }

    /// Why the answer could not be used, when it could not.
    pub fn failed(&self) -> Option<Failure> {
        match self.read {
            Read::Failed(failure) => Some(failure),
            Read::Idle | Read::Asking(_) | Read::Landed => None,
        }
    }

    /// Folds one answer the shelf asked for into its state.
    ///
    /// The answer is matched by the URIs the ask named: one shelf failing must
    /// never overwrite another's, and a retry's answer is not the one an older
    /// ask waits for. The answer is indexed once, by the URI of each track it
    /// could name, so drawing never scans it.
    pub fn land(&mut self, answer: &HomeAnswer) {
        if let HomeAnswer::Portrait { artist, portrait } = answer {
            if self.asked.contains(artist) {
                self.portraits.insert(artist.clone(), portrait.clone());
            }
            return;
        }
        let HomeAnswer::Labelled { asked, named } = answer else {
            return;
        };
        if !self.answers(asked) {
            return;
        }
        let mut labels: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for track in named {
            let pairs: Vec<(String, String)> = track
                .artist_uris
                .iter()
                .cloned()
                .zip(track.artist_names.iter().cloned())
                .collect();
            // A track that fell back to its URI names no artist, so it is one
            // of the misses this read is judged on.
            if pairs.is_empty() {
                continue;
            }
            labels.insert(track.uri.clone(), pairs);
        }
        let unlabelled = asked
            .iter()
            .filter(|uri| !labels.contains_key(*uri))
            .count();
        if !readable(asked.len(), unlabelled) {
            self.read = Read::Failed(Failure::Load);
            return;
        }
        self.labels = labels;
        self.read = Read::Landed;
    }

    /// The face this open landed for `artist`, when it landed one.
    pub fn portrait(&self, artist: &str) -> Option<&CoverSet> {
        self.portraits.get(artist)?.as_ref()
    }

    /// Whether this open already asked for `artist`'s face.
    pub fn portrait_asked(&self, artist: &str) -> bool {
        self.asked.contains(artist)
    }

    /// Records that `artist`'s face is asked for, so no later frame asks again.
    pub fn ask_portrait(&mut self, artist: &str) {
        self.asked.insert(artist.to_owned());
    }

    /// Records that one of the shelf's reads failed.
    pub fn fail(&mut self, ask: &HomeAsk, failure: Failure) {
        if let HomeAsk::Portrait { artist } = ask {
            // A face that will not read is an artist without one: the shelf
            // draws the slot and moves on rather than asking again.
            if self.asked.contains(artist) {
                self.portraits.insert(artist.clone(), None);
            }
            return;
        }
        let HomeAsk::Labels { tracks } = ask else {
            return;
        };
        if self.answers(tracks) {
            self.read = Read::Failed(failure);
        }
    }

    /// Drops everything the last open read, so a retry asks again.
    pub fn forget(&mut self) {
        self.labels.clear();
        self.read = Read::Idle;
        self.forget_faces();
    }

    /// Drops this open's faces, so a retry reads them again.
    fn forget_faces(&mut self) {
        self.portraits.clear();
        self.asked.clear();
    }

    /// Whether `asked` is the ask this open sent, and has not answered yet.
    fn answers(&self, asked: &[String]) -> bool {
        match &self.read {
            Read::Asking(sent) => sent == asked,
            Read::Idle | Read::Landed | Read::Failed(_) => false,
        }
    }
}

#[cfg(test)]
mod tests;
