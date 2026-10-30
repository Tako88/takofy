//! The episodes shelf: the newest episodes of the followed shows.
//!
//! Pure: the pool is the newest dozen dated episodes across the shows this open
//! read, and the draw is a subset of it, so two opens over the same shows differ
//! and one open never draws an episode twice. One read names every show this
//! open asks for, so an answer that names no asked show at all belongs to
//! another open and is dropped rather than drawn.

use takofy_model::home::{HomeAnswer, HomeAsk};
use takofy_model::{Episode, PublishedAt, SavedAt, ShowDetail};

use crate::message::Failure;

use super::Seed;
use super::draw::{draw, newest};

/// How many of the newest followed shows one open reads.
pub const SHOWS: usize = 8;

/// How many of the newest dated episodes the draw pools.
///
/// It is the pool over every show read this open, not a per-show count, so a
/// show with a long recent run can fill the shelf on its own.
pub const POOL: usize = 12;

/// How many rows the shelf draws from that pool.
pub const DRAWN: usize = 4;

/// The salt that keeps this shelf's draw off the page's other shelves.
pub(super) const SALT: u64 = 0x0045_5049_534F;

/// The episodes this open draws, as indices into `dates`, newest first.
///
/// `dates` are the episodes' publish dates in the order their shows were
/// flattened. The pool is the newest [`POOL`] dated entries, so an episode with
/// no date is never drawn ahead of one with a date, and the draw reaches no
/// further than the pool however many episodes the shows list.
pub fn pick(dates: &[Option<u64>], seed: Seed) -> Vec<usize> {
    let dated: Vec<Option<SavedAt>> = dates
        .iter()
        .map(|at| {
            at.and_then(|ms| i64::try_from(ms).ok())
                .and_then(SavedAt::from_millis)
        })
        .collect();
    let pool = newest(&dated, POOL);
    draw(&pool, seed, DRAWN)
        .into_iter()
        .filter_map(|position| pool.get(position).copied())
        .collect()
}

/// What the episodes shelf holds between opens.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct State {
    /// The page's seed for this open, so a held seed repeats the shelf.
    seed: Seed,
    /// The shows this open asked for, in the order it asked.
    asked: Vec<String>,
    /// The show answers that landed this open, in the order they arrived.
    landed: Vec<ShowDetail>,
    /// The shows this open asked for that failed, and why.
    failed: Vec<(String, Failure)>,
}

impl State {
    /// Starts this open, from the page's own seed.
    pub fn open(&mut self, seed: Seed) {
        self.forget();
        self.seed = seed.split(SALT);
    }

    /// Folds the one answer the shelf asked for into its state.
    ///
    /// The match is by show URI among the shows this open asked for. An answer
    /// that names none of them belongs to another open, so it settles nothing;
    /// otherwise every asked show the answer left out is a read that did not
    /// happen, not one still owed. A show that failed earlier and answers now
    /// is a landed show, not both.
    pub fn land(&mut self, answer: &HomeAnswer) {
        let HomeAnswer::Episodes(details) = answer else {
            return;
        };
        let mut matched = false;
        for detail in details {
            let uri = detail.show.uri.as_str();
            if !self.has_asked(uri) {
                continue;
            }
            matched = true;
            let detail = named(detail);
            self.failed.retain(|(failed, _)| failed != uri);
            if let Some(held) = self.landed.iter_mut().find(|held| held.show.uri == uri) {
                *held = detail;
            } else {
                self.landed.push(detail);
            }
        }
        if !matched {
            return;
        }
        let missing: Vec<String> = self
            .asked
            .iter()
            .filter(|uri| !self.answered(uri))
            .cloned()
            .collect();
        for uri in missing {
            self.failed.push((uri, Failure::Load));
        }
    }

    /// Records that the shelf's read failed.
    ///
    /// A failure costs the shows it names that have not landed and no more: the
    /// shows this open already read still draw, and only an open whose every
    /// asked show failed settles as a failure.
    pub fn fail(&mut self, ask: &HomeAsk, failure: Failure) {
        let HomeAsk::Episodes { shows } = ask else {
            return;
        };
        for show in shows {
            if !self.has_asked(show) || self.landed.iter().any(|held| held.show.uri == *show) {
                continue;
            }
            if let Some((_, held)) = self.failed.iter_mut().find(|(uri, _)| uri == show) {
                *held = failure;
            } else {
                self.failed.push((show.clone(), failure));
            }
        }
    }

    /// Drops everything the last open read, so a retry asks again.
    ///
    /// The seed is kept: a retry belongs to the open it retries, so it draws the
    /// same rows from the same shows.
    pub fn forget(&mut self) {
        self.asked.clear();
        self.landed.clear();
        self.failed.clear();
    }

    /// Records the shows the read this open sent names.
    pub fn ask(&mut self, shows: &[String]) {
        for show in shows {
            if !self.has_asked(show) {
                self.asked.push(show.clone());
            }
        }
    }

    /// The shows this open asked for, in the order it asked.
    pub fn asked(&self) -> &[String] {
        &self.asked
    }

    /// Whether this open already asked for `show`.
    pub fn has_asked(&self, show: &str) -> bool {
        self.asked.iter().any(|asked| asked == show)
    }

    /// Whether a show this open asked for has neither answered nor failed.
    pub fn pending(&self) -> bool {
        self.asked.iter().any(|asked| !self.answered(asked))
    }

    /// The shows whose answers landed this open.
    pub fn landed(&self) -> &[ShowDetail] {
        &self.landed
    }

    /// Whether any episode this open landed can name itself.
    ///
    /// An episode the wire could not name answers with its own URI and no
    /// publish date, so the pool of newest episodes drops it. Counting the
    /// misses is what tells a read that fell back to its URIs from a library
    /// whose followed shows have nothing new.
    pub fn readable(&self) -> bool {
        self.landed
            .iter()
            .flat_map(|detail| &detail.episodes)
            .any(|episode| episode.named)
    }

    /// The first failure this open hit that no answer made good.
    pub fn failure(&self) -> Option<Failure> {
        self.failed.first().map(|(_, failure)| *failure)
    }

    /// What the shelf draws, once at least one show answered with an episode.
    ///
    /// The shows are read in URI order, so the pool's ties do not depend on the
    /// order the wire's answers happened to arrive in.
    pub fn list(&self) -> Option<List<'_>> {
        if self.landed.is_empty() {
            return None;
        }
        let mut shows: Vec<&ShowDetail> = self.landed.iter().collect();
        shows.sort_by(|left, right| left.show.uri.cmp(&right.show.uri));

        let mut episodes: Vec<&Episode> = Vec::new();
        let mut dates: Vec<Option<u64>> = Vec::new();
        for detail in shows {
            for episode in &detail.episodes {
                episodes.push(episode);
                dates.push(episode.published.map(PublishedAt::as_millis));
            }
        }
        let rows: Vec<&Episode> = pick(&dates, self.seed)
            .into_iter()
            .filter_map(|at| episodes.get(at).copied())
            .collect();
        (!rows.is_empty()).then_some(List { rows })
    }

    /// Whether `show`'s answer has landed or its read failed.
    fn answered(&self, show: &str) -> bool {
        self.landed.iter().any(|held| held.show.uri == show)
            || self.failed.iter().any(|(uri, _)| uri == show)
    }
}

/// What the episodes shelf draws, borrowed from the library for one frame.
///
/// It owns nothing: every row is an episode the library already holds, so a
/// frame costs no copies.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct List<'a> {
    /// The drawn episodes, newest first.
    pub rows: Vec<&'a Episode>,
}

/// One show's answer, with each episode naming the show it came from.
///
/// A row draws its byline from the episode, and the wire does not promise every
/// row repeats its show, so a blank name or a missing address is filled from the
/// show the answer is about.
fn named(detail: &ShowDetail) -> ShowDetail {
    let mut detail = detail.clone();
    for episode in &mut detail.episodes {
        if episode.show.trim().is_empty() {
            episode.show = detail.show.name.clone();
        }
        if episode.show_uri.is_none() {
            episode.show_uri = Some(detail.show.uri.clone());
        }
    }
    detail
}

#[cfg(test)]
mod tests;
