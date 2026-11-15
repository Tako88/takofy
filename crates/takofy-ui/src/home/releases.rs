//! The release shelf: what the followed artists released recently.
//!
//! Pure: the read's answer in, the window and the cards out. The request is in
//! `library::home::releases`.
//!
//! The window is the last [`WINDOW_DAYS`] days, or the days since the artist was
//! followed when that is later, so an artist followed yesterday shows only what
//! is newer than the follow. Release dates are civil dates in the service's own
//! frame compared against the local day, so a release near midnight can sit one
//! day off; the follow day uses the local offset at open time, so a zone change
//! between the follow and the open can move an edge by a day.

use std::collections::{BTreeMap, HashSet};

use takofy_model::home::{ArtistReleases, Day, HomeAnswer, HomeAsk, NewRelease};
use takofy_model::{FollowedArtist, SavedAt};

use crate::message::Failure;

/// How many followed artists one open asks about.
///
/// Sixty caps a cold open at about 23 requests. An artist followed beyond the
/// sixtieth draws no card until a new day, and the aside counts what is drawn,
/// so it never overstates.
pub const SHELF_ARTISTS: usize = 60;

/// How many days back the window reaches, which is what "new" means here.
pub const WINDOW_DAYS: i32 = 30;

/// How many cards the shelf draws, which is the page's one card row.
pub const DRAWN: usize = 4;

/// How many seconds a day has, for turning an instant into a day.
const SECS_PER_DAY: i64 = 86_400;

/// The day number of 1970-01-01 in the count [`Day`] keeps from 0001-01-01.
const EPOCH_DAY: i32 = 719_163;

/// One release the shelf draws, borrowed from what the read held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Card<'a> {
    pub release: &'a NewRelease,
}

/// What the release shelf draws this frame.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct List<'a> {
    pub cards: Vec<Card<'a>>,
}

/// What the release shelf holds between opens.
///
/// The cache is in memory and per launch: the held releases are kept for the
/// latched day only, so every app start on a given day pays the cold read again.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct State {
    /// The day the window was latched on, which is what the cache is keyed by.
    day: Option<Day>,
    /// The artists this open asked about.
    asked: Vec<String>,
    /// The artists this open's read answered, whether or not they had anything.
    /// An artist the read left out is neither held nor asked again this open.
    answered: HashSet<String>,
    /// Each answered artist's releases, keyed by the artist's URI.
    held: BTreeMap<String, Vec<NewRelease>>,
    /// The failure that stopped this open's read.
    failed: Option<Failure>,
}

impl State {
    /// Latches the day the window is measured from, dropping a previous day's
    /// held releases.
    pub fn latch(&mut self, day: Day) {
        if self.day == Some(day) {
            return;
        }
        self.day = Some(day);
        self.forget();
    }

    /// The day the window is measured from, once the binary has named it.
    pub fn day(&self) -> Option<Day> {
        self.day
    }

    /// Starts this open: the reads may be asked for again, and what a previous
    /// open held stays held.
    pub fn open(&mut self) {
        self.asked.clear();
        self.answered.clear();
        self.failed = None;
    }

    /// Drops everything, for a retry that reads the same day again.
    pub fn forget(&mut self) {
        self.asked.clear();
        self.answered.clear();
        self.held.clear();
        self.failed = None;
    }

    /// Records the artists the read of this open names.
    pub fn ask(&mut self, artists: &[String]) {
        for artist in artists {
            if !self.has_asked(artist) {
                self.asked.push(artist.clone());
            }
        }
    }

    /// Whether this open already asked about `artist`.
    pub fn has_asked(&self, artist: &str) -> bool {
        self.asked.iter().any(|asked| asked == artist)
    }

    /// Whether an artist this open asked about has not answered yet.
    pub fn pending(&self) -> bool {
        self.asked
            .iter()
            .any(|asked| !self.answered.contains(asked))
    }

    /// Folds the read's answer into the shelf.
    ///
    /// A release the shelf cannot place is refused rather than held: outside the
    /// floor's window or dated after today, no follow date can bring it in.
    pub fn land(&mut self, answer: &HomeAnswer) {
        let HomeAnswer::Releases { asked, found } = answer else {
            return;
        };
        let Some(today) = self.day else {
            return;
        };
        for artist in asked {
            self.answered.insert(artist.clone());
        }
        for ArtistReleases { artist, releases } in found {
            let kept = releases
                .iter()
                .filter(|release| placeable(release.day, today))
                .cloned()
                .collect();
            self.held.insert(artist.clone(), kept);
        }
    }

    /// Records that this open's read failed, which settles the shelf as absent
    /// and stops it asking again this open.
    pub fn fail(&mut self, ask: &HomeAsk, failure: Failure) {
        let HomeAsk::Releases { artists, .. } = ask else {
            return;
        };
        self.failed = Some(failure);
        self.ask(artists);
        for artist in artists {
            self.answered.insert(artist.clone());
        }
    }

    /// The failure that stopped this open's read, if one did.
    pub fn failure(&self) -> Option<Failure> {
        self.failed
    }

    /// The artists this open still owes a read for: the first [`SHELF_ARTISTS`]
    /// followed that are neither held nor asked.
    pub fn owed(&self, followed: &[FollowedArtist]) -> Vec<String> {
        followed
            .iter()
            .take(SHELF_ARTISTS)
            .map(|artist| &artist.uri)
            .filter(|uri| !self.held.contains_key(*uri) && !self.has_asked(uri))
            .cloned()
            .collect()
    }

    /// The cards the shelf draws, from `followed`' own order and each artist's
    /// own window.
    ///
    /// The window is built from the followed list, so an artist the user no
    /// longer follows drops out however recently it released.
    pub fn pick(&self, followed: &[FollowedArtist], offset_secs: i32, limit: usize) -> List<'_> {
        let Some(today) = self.day else {
            return List::default();
        };
        let mut candidates: Vec<(usize, &NewRelease)> = Vec::new();
        for (order, artist) in followed.iter().take(SHELF_ARTISTS).enumerate() {
            let Some(releases) = self.held.get(&artist.uri) else {
                continue;
            };
            let start = window_start(artist.followed_at, today, offset_secs);
            for release in releases {
                if admits(release.day, start, today) {
                    candidates.push((order, release));
                }
            }
        }
        ordered(candidates, limit)
    }

    /// Every held release, ordered the same way and filtered by nothing.
    ///
    /// The followed list is `Idle` or `Loading` through the reload a follow
    /// causes, so the shelf draws what it already holds until the list answers
    /// again: it neither blinks to a skeleton nor loses its rule mid-reload.
    pub fn held_list(&self, limit: usize) -> List<'_> {
        let candidates: Vec<(usize, &NewRelease)> = self
            .held
            .values()
            .flatten()
            .map(|release| (0, release))
            .collect();
        ordered(candidates, limit)
    }
}

/// The cards from `candidates`: newest first, ties by followed order and then by
/// URI, one per release URI, cut at `limit`.
fn ordered(candidates: Vec<(usize, &NewRelease)>, limit: usize) -> List<'_> {
    let mut candidates = candidates;
    candidates.sort_by(|(left_order, left), (right_order, right)| {
        right
            .day
            .days()
            .cmp(&left.day.days())
            .then(left_order.cmp(right_order))
            .then(left.uri.cmp(&right.uri))
    });
    let mut seen = HashSet::new();
    let cards = candidates
        .into_iter()
        .filter(|(_, release)| seen.insert(release.uri.as_str()))
        .take(limit)
        .map(|(_, release)| Card { release })
        .collect();
    List { cards }
}

/// The day a follow was made, in the local frame of `offset_secs`.
pub fn followed_day(at: SavedAt, offset_secs: i32) -> Day {
    let millis = i64::try_from(at.as_millis()).unwrap_or(i64::MAX);
    let secs = millis / 1_000 + i64::from(offset_secs);
    let days = i32::try_from(secs.div_euclid(SECS_PER_DAY)).unwrap_or(i32::MAX);
    Day::from_days(EPOCH_DAY.saturating_add(days))
}

/// The first day the window admits: the floor, or the follow when that is later.
pub fn window_start(followed_at: Option<SavedAt>, today: Day, offset_secs: i32) -> Day {
    let floor = Day::from_days(today.days() - WINDOW_DAYS);
    match followed_at {
        Some(at) => {
            let followed = followed_day(at, offset_secs);
            Day::from_days(followed.days().max(floor.days()))
        }
        None => floor,
    }
}

/// Whether a release dated `day` is inside the window that starts on `start`.
pub fn admits(day: Day, start: Day, today: Day) -> bool {
    day.days() >= start.days() && day.days() <= today.days()
}

/// Whether `day` could fall in any window measured from `today`, so a release
/// outside it never has to be held.
fn placeable(day: Day, today: Day) -> bool {
    admits(day, Day::from_days(today.days() - WINDOW_DAYS), today)
}

#[cfg(test)]
mod tests;
