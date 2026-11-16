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

use self::pool::{EDGE, PER_KIND};

pub mod body;
pub mod fresh;
pub mod hidden;
pub mod pool;
pub mod walk;

/// How long one row waits after its answer lands before it may re-roll again.
///
/// The wait runs from the answer, not from the press. A re-seed's lookups outlast
/// any cooldown short enough to press through, so a wait started at the press
/// would already be over when the row came back and there would be nothing to
/// draw; started at the answer it is always the same two seconds, the ones the
/// button draws as its border.
pub const REROLL_COOLDOWN_MS: u64 = 2_000;

/// Whether a row's re-roll can act now, and when it can next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reroll {
    /// The row may move on.
    Ready,
    /// The cooldown is the only thing holding it, for this many milliseconds.
    Wait(u64),
    /// The row has no seed left, or the open has no ask to spend.
    Never,
}

impl Reroll {
    /// How much of the wait has run, when there is a wait worth drawing.
    ///
    /// A row that is ready has nothing to fill and a spent row has nothing to
    /// promise, so only the cooldown answers.
    pub fn waited(&self) -> Option<f32> {
        match self {
            Reroll::Wait(ms) => {
                let left = *ms as f32 / REROLL_COOLDOWN_MS as f32;
                Some(1.0 - left.clamp(0.0, 1.0))
            }
            Reroll::Ready | Reroll::Never => None,
        }
    }
}

/// How many asks one open allows, enough for every row to walk its whole pool:
/// one per candidate walked, in all three rows, and one in hand.
///
/// An ask is spent by the walk, not by a draw, so a row of unresolvable seeds
/// spends asks without spending its day.
pub const ASKS_PER_OPEN: usize = EDGE * 2 * 3 + 3;

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
    /// Seeds this row has drawn today, which a rejected one does not count
    /// against. Not persisted, so a restart is generous to a row that was spent
    /// by its draws rather than by its pool.
    drawn: u8,
    reseeds_left: u8,
    read: Read,
    /// When the seed the row is on answered, which is when its wait starts.
    landed_ms: Option<u64>,
}

impl Default for Row {
    fn default() -> Self {
        Self {
            pool: Vec::new(),
            at: 0,
            drawn: 0,
            reseeds_left: 1,
            read: Read::Idle,
            landed_ms: None,
        }
    }
}

impl Row {
    /// Moves to the next seed, going nowhere when the row is spent.
    ///
    /// Spent means either the pool has been walked to its end or the day's draws
    /// are used up. A spent row's position is the pool's end, which is what the
    /// hidden file saves and what a restart reads back as spent.
    fn advance(&mut self) {
        self.at += 1;
        self.read = if self.at < self.pool.len() && !self.spent() {
            Read::Idle
        } else {
            self.at = self.at.min(self.pool.len());
            Read::Gone
        };
    }

    /// Whether the row has drawn all it may today.
    fn spent(&self) -> bool {
        self.drawn >= PER_KIND as u8
    }
}

/// Discovery's rows between opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    day: Option<Day>,
    latched: Option<Day>,
    rows: [Row; 3],
    asks_left: usize,
    /// The artists followed since this page was opened.
    ///
    /// A follow removes its card, because the artist is no longer outside the
    /// library. Keeping the click that did it undoable for the rest of the open
    /// is worth more than the tidiness of an immediate removal: the band is
    /// rebuilt on the next open, and the artist is not in it then.
    just_followed: HashSet<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            day: None,
            latched: None,
            rows: std::array::from_fn(|_| Row::default()),
            asks_left: 0,
            just_followed: HashSet::new(),
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
            self.just_followed.clear();
        }
    }

    /// The day this page opened on, once the binary has named it.
    pub fn latched(&self) -> Option<Day> {
        self.latched
    }

    /// Records a follow the wire confirmed, so its card keeps its place.
    pub fn note_follow(&mut self, artist: &str) {
        self.just_followed.insert(artist.to_owned());
    }

    /// Records an unfollow, which puts the card back among the fresh ones.
    pub fn note_unfollow(&mut self, artist: &str) {
        self.just_followed.remove(artist);
    }

    /// Whether `artist` was followed since this page was opened.
    pub fn followed_here(&self, artist: &str) -> bool {
        self.just_followed.contains(artist)
    }

    /// The artists followed since this page was opened, in no order.
    pub fn just_followed(&self) -> impl Iterator<Item = &str> {
        self.just_followed.iter().map(String::as_str)
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
        self.just_followed.clear();
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
                // The pool's end is a row spent on this day, which a restart
                // must not refill.
                row.at = position.min(row.pool.len());
                row.read = if row.at < row.pool.len() {
                    Read::Idle
                } else {
                    Read::Gone
                };
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
        row.landed_ms = None;
        match &answer.outcome {
            DiscoveryOutcome::Off => row.read = Read::Idle,
            DiscoveryOutcome::Unreachable => row.read = Read::Unreachable,
            DiscoveryOutcome::Rejected => row.advance(),
            DiscoveryOutcome::Neighbours { cards, genre } => {
                row.pool[row.at].anchor = answer.seed.anchor.clone();
                row.drawn = row.drawn.saturating_add(1);
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
    /// this wherever it likes in the frame. Counting and drawing share
    /// [`Self::kept`], so a row counts exactly the cards the band shows.
    pub fn fresh_counts(&self, owned: &fresh::Owned, hidden: &hidden::Hidden) -> [usize; 3] {
        self.kept(owned, hidden).map(|cards| cards.len())
    }

    /// The cards each row may draw, in kind order, each artist kept for the
    /// first row that offers it.
    ///
    /// A row that has not landed keeps nothing. The library's filter is applied
    /// before the artist is claimed, so a card one row drops never hides the same
    /// artist from the next.
    fn kept<'a>(
        &'a self,
        owned: &fresh::Owned,
        hidden: &hidden::Hidden,
    ) -> [Vec<&'a DiscoveryCard>; 3] {
        let mut kept: [Vec<&'a DiscoveryCard>; 3] = std::array::from_fn(|_| Vec::new());
        let mut seen: HashSet<&'a str> = HashSet::new();
        for (slot, row) in kept.iter_mut().zip(&self.rows) {
            let Read::Landed { cards, .. } = &row.read else {
                continue;
            };
            let anchor = row.pool.get(row.at).and_then(|seed| seed.anchor.as_ref());
            *slot = fresh::fresh(cards, anchor, owned, hidden)
                .into_iter()
                .filter(|card| seen.insert(card.artist.uri.as_str()))
                .collect();
        }
        kept
    }

    /// Spends the one re-seed a landed row has when nothing it found is fresh.
    pub fn settle(&mut self, fresh_counts: [usize; 3]) {
        for (row, fresh) in self.rows.iter_mut().zip(fresh_counts) {
            if !matches!(row.read, Read::Landed { .. }) || fresh > 0 {
                continue;
            }
            if row.reseeds_left == 0 {
                row.at = row.pool.len();
                row.read = Read::Gone;
                continue;
            }
            row.reseeds_left -= 1;
            row.advance();
        }
    }

    /// Whether one row can re-roll now, and when it can next.
    ///
    /// The cooldown's remaining time is reported so a view can wake at its end
    /// rather than staying dim until the pointer moves.
    pub fn reroll_state(&self, kind: DiscoveryKind, now_ms: u64) -> Reroll {
        if self.asks_left == 0 {
            return Reroll::Never;
        }
        let row = &self.rows[kind.index()];
        if row.at + 1 >= row.pool.len() || row.spent() {
            return Reroll::Never;
        }
        match row.landed_ms {
            Some(landed) => {
                let left = landed
                    .saturating_add(REROLL_COOLDOWN_MS)
                    .saturating_sub(now_ms);
                if left > 0 {
                    Reroll::Wait(left)
                } else {
                    Reroll::Ready
                }
            }
            // Its answer is still owed, so the wait it will draw has not begun.
            None => Reroll::Wait(REROLL_COOLDOWN_MS),
        }
    }

    /// Stamps the rows whose answer has just arrived, and clears the stamp of a
    /// row that has left its answer behind.
    ///
    /// The app calls this once a frame because it is the only place that holds the
    /// wall clock; a row stamped here is a row whose wait the button can draw.
    pub fn stamp_landed(&mut self, now_ms: u64) {
        for row in &mut self.rows {
            if matches!(row.read, Read::Landed { .. }) {
                row.landed_ms.get_or_insert(now_ms);
            } else {
                row.landed_ms = None;
            }
        }
    }

    /// Whether one row can re-roll now.
    pub fn can_reroll(&self, kind: DiscoveryKind, now_ms: u64) -> bool {
        matches!(self.reroll_state(kind, now_ms), Reroll::Ready)
    }

    /// Moves one row on to its next seed, once its own wait is out and never past
    /// the pool's end.
    ///
    /// A re-roll the open cannot ask for is refused: moving the row to `Idle`
    /// with no ask behind it would draw a row that is loading forever.
    pub fn reroll(&mut self, kind: DiscoveryKind, now_ms: u64) -> bool {
        if !self.can_reroll(kind, now_ms) {
            return false;
        }
        let row = &mut self.rows[kind.index()];
        row.at += 1;
        row.read = Read::Idle;
        row.landed_ms = None;
        true
    }

    /// Where each row is, for the hidden file to remember.
    pub fn positions(&self) -> [usize; 3] {
        std::array::from_fn(|index| self.rows[index].at)
    }

    /// What the band draws this frame.
    ///
    /// The setting being off and the library's inputs being absent are the
    /// caller's to answer; this covers the rows themselves. A landed row whose
    /// cards all belong to an earlier row is not drawn, and a band left with no
    /// row at all is absent rather than empty.
    pub fn band(&self, owned: &fresh::Owned, hidden: &hidden::Hidden) -> Band<'_> {
        let cards = self.kept(owned, hidden);
        let mut views: Vec<RowView<'_>> = Vec::with_capacity(self.rows.len());
        for (row, cards) in self.rows.iter().zip(cards) {
            let Some(seed) = row.pool.get(row.at) else {
                continue;
            };
            let (read, genre, cards) = match &row.read {
                Read::Idle | Read::Asked => (RowRead::Loading, None, Vec::new()),
                Read::Unreachable => (RowRead::Unreachable, None, Vec::new()),
                Read::Gone => continue,
                Read::Landed { genre, .. } => {
                    if cards.is_empty() {
                        continue;
                    }
                    (RowRead::Ready, genre.as_deref(), cards)
                }
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
