//! The stations shelf: a two-column band built from the library's own seeds.
//!
//! Pure: the seeds are drawn from the four candidate lists, the rows are drawn
//! from each station's own list, and the band is whatever of it landed. Only a
//! track is ever a row, so a station that answered with nothing the band can
//! draw is dropped rather than offering a play that would queue nothing.

use std::collections::{HashMap, HashSet};

use takofy_model::TrackEntry;
use takofy_model::home::{HomeAnswer, HomeAsk, SeedKind, Station, StationSeed};

use crate::format::station_source;
use crate::message::Failure;
use crate::shuffle;

use super::Seed;

/// How many stations the band holds.
pub const STATIONS: usize = 4;

/// How many of a station's rows the band shows.
pub const ROWS: usize = 3;

/// The library's own things a station can be seeded from, one list per kind.
///
/// A seed is named by its URI and nothing else, so two playlists with one title
/// are two candidates and one URI listed twice is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Candidates<'a> {
    pub playlists: &'a [String],
    pub artists: &'a [String],
    pub albums: &'a [String],
    pub songs: &'a [String],
}

/// The seeds this open builds its stations from.
///
/// One seed of every kind in the page's order, then a second seed of whichever
/// kinds still have one, round-robin, until the band is full: a library with
/// nothing liked still fills the band from its playlists, and a library with one
/// album draws one station rather than four.
pub fn pick_seeds(candidates: &Candidates<'_>, seed: Seed) -> Vec<StationSeed> {
    let kinds = [
        (SeedKind::Playlist, candidates.playlists),
        (SeedKind::Artist, candidates.artists),
        (SeedKind::Album, candidates.albums),
        (SeedKind::Song, candidates.songs),
    ];
    // Each kind is drawn into its own order first, so a kind with many
    // candidates is never the only kind a small band shows.
    let drawn: Vec<(SeedKind, Vec<String>)> = kinds
        .iter()
        .enumerate()
        .map(|(salt, (kind, uris))| (*kind, drawn_uris(uris, seed.split(salt as u64))))
        .collect();

    let mut next = vec![0usize; drawn.len()];
    let mut taken: Vec<&str> = Vec::new();
    let mut seeds: Vec<StationSeed> = Vec::with_capacity(STATIONS);
    loop {
        let mut placed = false;
        for (index, (kind, uris)) in drawn.iter().enumerate() {
            if seeds.len() == STATIONS {
                break;
            }
            while let Some(uri) = uris.get(next[index]) {
                next[index] += 1;
                if taken.contains(&uri.as_str()) {
                    continue;
                }
                taken.push(uri);
                seeds.push(StationSeed {
                    kind: *kind,
                    uri: uri.clone(),
                });
                placed = true;
                break;
            }
        }
        if seeds.len() == STATIONS || !placed {
            break;
        }
    }
    seeds
}

/// One kind's URIs in a drawn order.
///
/// The list is deduplicated by URI before it is drawn, so a URI the library
/// holds twice is one candidate and a draw without replacement can never take it
/// twice.
fn drawn_uris(uris: &[String], seed: Seed) -> Vec<String> {
    let mut seen = HashSet::new();
    let distinct: Vec<&String> = uris
        .iter()
        .filter(|uri| seen.insert(uri.as_str()))
        .collect();
    shuffle::order(distinct.len(), seed.state())
        .into_iter()
        .filter_map(|at| distinct.get(at).map(|uri| (*uri).clone()))
        .collect()
}

/// The rows the band shows of one station: [`ROWS`] of them, ascending.
///
/// A held seed pins the same rows, and a station with fewer rows shows all it
/// has.
pub fn shown_rows(rows: &[TrackEntry], seed: Seed) -> Vec<usize> {
    let mut shown: Vec<usize> = shuffle::order(rows.len(), seed.state())
        .into_iter()
        .take(ROWS)
        .collect();
    shown.sort_unstable();
    shown
}

/// What the stations shelf draws, borrowed from the stations it holds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Band<'a> {
    pub stations: Vec<Shown<'a>>,
}

/// One station: its head, and the rows the band shows of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown<'a> {
    /// The line under the seed's name, saying what the station was built from.
    pub source: &'static str,
    /// The station's own title, which is the seed's name.
    pub head: &'a str,
    /// The station's own URI, which is what its play hands the queue.
    pub uri: &'a str,
    /// The rows the band shows, as indices into [`Self::whole`], ascending.
    pub shown: Vec<usize>,
    /// Every row the station holds, which is what its play queues.
    pub whole: &'a [TrackEntry],
}

/// What one station's read is doing this open.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Read {
    /// The ask is out and no answer has landed.
    Asked,
    /// The station landed with rows the band can draw, and the rows it shows.
    Landed {
        station: Station,
        /// The rows the band shows, drawn once for this open.
        shown: Vec<usize>,
    },
    /// The station landed with no row the band can draw, so it is dropped.
    Empty,
    /// The read failed.
    Failed,
}

/// What the stations shelf holds between opens.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct State {
    /// The page's own seed, which the shown rows are drawn from.
    seed: Seed,
    /// The seeds this open builds its stations from, once the feeds settled.
    seeds: Option<Vec<StationSeed>>,
    /// What each asked seed's read is doing, keyed by the seed's URI.
    reads: HashMap<String, Read>,
    /// The first read that failed this open, so a band that drew nothing names it.
    failed: Option<Failure>,
}

impl State {
    /// Starts this open, from the page's own seed.
    pub fn open(&mut self, seed: Seed) {
        self.seed = seed;
        self.forget();
    }

    /// Folds one answer the shelf asked for into its state.
    ///
    /// Only an answer to a seed this open asked for is kept: anything else is a
    /// previous open's read arriving after the page moved on.
    pub fn land(&mut self, answer: &HomeAnswer) {
        let HomeAnswer::Station(station) = answer else {
            return;
        };
        let uri = station.seed.uri.clone();
        if !matches!(self.reads.get(&uri), Some(Read::Asked)) {
            return;
        }
        let read = if station.rows.is_empty() {
            Read::Empty
        } else {
            let Some(at) = self
                .seeds
                .as_ref()
                .and_then(|seeds| seeds.iter().position(|seed| seed.uri == uri))
            else {
                return;
            };
            Read::Landed {
                station: station.clone(),
                shown: shown_rows(&station.rows, self.seed.split(at as u64)),
            }
        };
        self.reads.insert(uri, read);
    }

    /// Records that one of the shelf's reads failed.
    pub fn fail(&mut self, ask: &HomeAsk, failure: Failure) {
        let HomeAsk::Station { seed } = ask else {
            return;
        };
        if !matches!(self.reads.get(&seed.uri), Some(Read::Asked)) {
            return;
        }
        self.reads.insert(seed.uri.clone(), Read::Failed);
        self.failed.get_or_insert(failure);
    }

    /// Drops everything the last open read, so a retry asks again.
    pub fn forget(&mut self) {
        self.seeds = None;
        self.reads.clear();
        self.failed = None;
    }

    /// Whether this open has chosen the seeds it builds its stations from.
    pub fn chosen(&self) -> bool {
        self.seeds.is_some()
    }

    /// Keeps the seeds this open builds its stations from.
    pub fn choose(&mut self, seeds: Vec<StationSeed>) {
        self.seeds = Some(seeds);
    }

    /// The first of this open's seeds no read has been asked for yet.
    pub fn next(&self) -> Option<&StationSeed> {
        let seeds = self.seeds.as_ref()?;
        seeds
            .iter()
            .find(|seed| !self.reads.contains_key(&seed.uri))
    }

    /// Records that `seed`'s read is out.
    pub fn asked(&mut self, seed: &StationSeed) {
        self.reads.insert(seed.uri.clone(), Read::Asked);
    }

    /// Whether a read this shelf sent is still out.
    pub fn pending(&self) -> bool {
        self.reads.values().any(|read| matches!(read, Read::Asked))
    }

    /// Whether every seed this open chose has an answer, asked for or not.
    pub fn answered(&self) -> bool {
        self.seeds.as_ref().is_some_and(|seeds| {
            seeds.iter().all(|seed| {
                matches!(
                    self.reads.get(&seed.uri),
                    Some(Read::Landed { .. } | Read::Empty | Read::Failed)
                )
            })
        })
    }

    /// The first failure this open, when a read failed.
    pub fn failure(&self) -> Option<Failure> {
        self.failed
    }

    /// The stations this open landed with rows to draw, or `None` without one.
    pub fn band(&self) -> Option<Band<'_>> {
        let seeds = self.seeds.as_deref().unwrap_or_default();
        let mut stations = Vec::new();
        for seed in seeds {
            let Some(Read::Landed { station, shown }) = self.reads.get(&seed.uri) else {
                continue;
            };
            if station.rows.is_empty() {
                continue;
            }
            stations.push(Shown {
                source: station_source(seed.kind),
                head: &station.title,
                uri: &station.uri,
                shown: shown.clone(),
                whole: &station.rows,
            });
        }
        (!stations.is_empty()).then_some(Band { stations })
    }
}

#[cfg(test)]
mod tests;
