//! Discovery's three rows: what each was seeded from, what came back, and how
//! one open spends its asks.
//!
//! Pure. The day a page opened on is latched, so a page left open across
//! midnight keeps showing its own day until the next `go_home`; a row asks at
//! most once per seed and never spends an ask on a row that already answered.
//! A row that lands on nothing the library does not already have spends its one
//! re-seed and then goes, so an empty row costs two lookups a day rather than
//! one every frame.

use std::collections::HashSet;

use takofy_model::home::{
    Day, DiscoveryCard, DiscoveryKind, DiscoveryOutcome, DiscoverySeed, HomeAnswer,
};
use takofy_model::{FollowedArtist, PlaylistEntry, SavedAlbum};

use self::pool::PER_KIND;

pub mod fresh;
pub mod hidden;
pub mod pool;

/// How long one row waits before it may re-roll again.
pub const REROLL_COOLDOWN_MS: u64 = 10_000;

/// How many asks one open allows, enough for every row to advance through its
/// whole pool.
pub const ASKS_PER_OPEN: usize = PER_KIND * 3;

/// What one row's read is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    /// The row may be asked for its seed.
    Idle,
    /// The row's seed has gone out and its answer has not come back.
    Asked,
    /// The neighbours the lookup found.
    Landed {
        cards: Vec<DiscoveryCard>,
        genre: Option<String>,
    },
    /// The outside did not answer, so the row waits for the next open.
    Unreachable,
    /// The row has no seed left today.
    Gone,
}

/// One row's pool and how far through it the row has got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pool: Vec<DiscoverySeed>,
    at: usize,
    reseeds_left: u8,
    read: Read,
    last_reroll_ms: Option<u64>,
}

impl Default for Row {
    fn default() -> Self {
        Self {
            pool: Vec::new(),
            at: 0,
            reseeds_left: 1,
            read: Read::Idle,
            last_reroll_ms: None,
        }
    }
}

impl Row {
    /// Moves to the next seed, going nowhere when the pool is spent.
    fn advance(&mut self) {
        self.at += 1;
        self.read = if self.at < self.pool.len() {
            Read::Idle
        } else {
            Read::Gone
        };
    }
}

/// Discovery's rows between opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    day: Option<Day>,
    latched: Option<Day>,
    rows: [Row; 3],
    asks_left: usize,
}

impl Default for State {
    fn default() -> Self {
        Self {
            day: None,
            latched: None,
            rows: std::array::from_fn(|_| Row::default()),
            asks_left: 0,
        }
    }
}

impl State {
    /// Latches the day the page opened on, once.
    ///
    /// A later frame's day is ignored until the next [`Self::open`], and a
    /// latched day that differs from the one the rows were built for starts them
    /// over.
    pub fn latch(&mut self, day: Day) {
        if self.latched.is_some() {
            return;
        }
        self.latched = Some(day);
        if self.day != Some(day) {
            self.day = None;
            self.rows = std::array::from_fn(|_| Row::default());
        }
    }

    /// The day this page opened on, once the binary has named it.
    pub fn latched(&self) -> Option<Day> {
        self.latched
    }

    /// Whether `day`'s pools have been built.
    pub fn chosen(&self, day: Day) -> bool {
        self.day == Some(day)
    }

    /// Starts a page visit: the rows may be asked again, and the extras the last
    /// visit left behind are not drawn.
    pub fn open(&mut self) {
        self.latched = None;
        self.asks_left = ASKS_PER_OPEN;
        for row in &mut self.rows {
            if matches!(row.read, Read::Asked | Read::Unreachable) {
                row.read = Read::Idle;
            }
        }
    }

    /// Builds the day's pools, once, and starts the rows where the saved
    /// positions left them.
    pub fn choose(
        &mut self,
        day: Day,
        playlists: &[PlaylistEntry],
        artists: &[FollowedArtist],
        albums: &[SavedAlbum],
        positions: Option<[usize; 3]>,
    ) {
        if self.day == Some(day) {
            return;
        }
        self.day = Some(day);
        let pools = pool::day_pool(playlists, artists, albums, day);
        self.rows = pools.map(|seeds| Row {
            pool: seeds,
            ..Row::default()
        });
        if let Some(positions) = positions {
            for (row, position) in self.rows.iter_mut().zip(positions) {
                row.at = position.min(row.pool.len().saturating_sub(1));
            }
        }
    }

    /// The next seed to look up, in kind order, or `None` when no row may ask.
    ///
    /// It spends an ask and marks the row, so a row whose answer never comes is
    /// asked once rather than on every frame.
    pub fn next_ask(&mut self) -> Option<DiscoverySeed> {
        if self.asks_left == 0 {
            return None;
        }
        for row in &mut self.rows {
            if row.read != Read::Idle {
                continue;
            }
            let Some(seed) = row.pool.get(row.at).cloned() else {
                continue;
            };
            row.read = Read::Asked;
            self.asks_left -= 1;
            return Some(seed);
        }
        None
    }

    /// Applies the answer to the row that asked for it, dropping a late or
    /// mismatched one.
    pub fn land(&mut self, answer: &HomeAnswer) {
        let HomeAnswer::Discovery(answer) = answer else {
            return;
        };
        let row = &mut self.rows[answer.seed.kind.index()];
        let Some(seed) = row.pool.get(row.at) else {
            return;
        };
        if row.read != Read::Asked || seed.kind != answer.seed.kind || seed.uri != answer.seed.uri {
            return;
        }
        match &answer.outcome {
            DiscoveryOutcome::Off => row.read = Read::Idle,
            DiscoveryOutcome::Unreachable => row.read = Read::Unreachable,
            DiscoveryOutcome::Rejected => row.advance(),
            DiscoveryOutcome::Neighbours { cards, genre } => {
                row.pool[row.at].anchor = answer.seed.anchor.clone();
                row.read = Read::Landed {
                    cards: cards.clone(),
                    genre: genre.clone(),
                };
            }
        }
    }

    /// How many of each landed row's cards the library does not already hold
    /// and the user has not hidden, in kind order.
    ///
    /// A row that has not landed counts nothing, so a caller may settle with
    /// this wherever it likes in the frame.
    pub fn fresh_counts(&self, owned: &fresh::Owned, hidden: &hidden::Hidden) -> [usize; 3] {
        std::array::from_fn(|index| {
            let row = &self.rows[index];
            let Read::Landed { cards, .. } = &row.read else {
                return 0;
            };
            let anchor = row.pool.get(row.at).and_then(|seed| seed.anchor.as_ref());
            fresh::fresh(cards, anchor, owned, hidden).len()
        })
    }

    /// Spends the one re-seed a landed row has when nothing it found is fresh.
    pub fn settle(&mut self, fresh_counts: [usize; 3]) {
        for (row, fresh) in self.rows.iter_mut().zip(fresh_counts) {
            if !matches!(row.read, Read::Landed { .. }) || fresh > 0 {
                continue;
            }
            if row.reseeds_left == 0 {
                row.read = Read::Gone;
                continue;
            }
            row.reseeds_left -= 1;
            row.advance();
        }
    }

    /// Moves one row on to its next seed, at most once per cooldown and never
    /// past the pool's end.
    pub fn reroll(&mut self, kind: DiscoveryKind, now_ms: u64) -> bool {
        let row = &mut self.rows[kind.index()];
        if let Some(last) = row.last_reroll_ms
            && now_ms.saturating_sub(last) < REROLL_COOLDOWN_MS
        {
            return false;
        }
        if row.at + 1 >= row.pool.len() {
            return false;
        }
        row.at += 1;
        row.read = Read::Idle;
        row.last_reroll_ms = Some(now_ms);
        true
    }

    /// Where each row is, for the hidden file to remember.
    pub fn positions(&self) -> [usize; 3] {
        std::array::from_fn(|index| self.rows[index].at)
    }

    /// What the band draws this frame.
    ///
    /// The setting being off and the library's inputs being absent are the
    /// caller's to answer; this covers the rows themselves.
    pub fn band(&self) -> Band<'_> {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut views: Vec<RowView<'_>> = Vec::with_capacity(self.rows.len());
        for row in &self.rows {
            let Some(seed) = row.pool.get(row.at) else {
                continue;
            };
            let (read, genre, cards) = match &row.read {
                Read::Idle | Read::Asked => (RowRead::Loading, None, Vec::new()),
                Read::Unreachable => (RowRead::Unreachable, None, Vec::new()),
                Read::Gone => continue,
                Read::Landed { cards, genre } => (
                    RowRead::Ready,
                    genre.as_deref(),
                    cards
                        .iter()
                        .filter(|card| seen.insert(card.artist.uri.as_str()))
                        .collect(),
                ),
            };
            views.push(RowView {
                seed,
                genre,
                cards,
                read,
            });
        }
        if views.is_empty() {
            return Band::Absent;
        }
        if views.iter().all(|view| view.read == RowRead::Unreachable) {
            return Band::Unreachable;
        }
        Band::Rows(views)
    }
}

/// What a row in the band is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowRead {
    /// The lookup has not answered yet.
    Loading,
    /// The outside did not answer.
    Unreachable,
    /// The row has cards to draw.
    Ready,
}

/// One row the band draws, borrowed from the state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowView<'a> {
    pub seed: &'a DiscoverySeed,
    pub genre: Option<&'a str>,
    pub cards: Vec<&'a DiscoveryCard>,
    pub read: RowRead,
}

/// What Discovery's section draws this frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Band<'a> {
    /// The setting is off, so nothing was read.
    Off,
    /// A read the band draws from has not answered.
    Waiting,
    /// There is no row to draw, or a library input failed.
    Absent,
    /// Every row that is here could not reach the outside.
    Unreachable,
    /// The rows, in kind order.
    Rows(Vec<RowView<'a>>),
}

#[cfg(test)]
mod tests;
