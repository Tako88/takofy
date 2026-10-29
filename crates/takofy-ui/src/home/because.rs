//! The because shelf: the artists related to one followed artist.
//!
//! Pure: the followed artists are tried in a drawn order, so an open asks each
//! of them at most once, and the answer that lands names a few of its related
//! artists as round cards. The name the head draws is the followed artist's own,
//! never a name the related answer carried.

use takofy_model::FollowedArtist;
use takofy_model::home::{HomeAnswer, HomeAsk};

use crate::message::Failure;

use super::draw::sample;
use super::seed::{Seed, below};

/// How many related artists the row draws.
pub const DRAWN: usize = 4;

/// How many of a related answer's artists the draw may take from.
///
/// The answer arrives in the wire's order, so the row never reaches past its
/// first dozen even when the artist has more neighbours than it can show.
pub const NEIGHBOURS: usize = 12;

/// The stream the followed artists are tried from, apart from the row's own.
const ORDER_SALT: u64 = 0x10;

/// The stream the row's cards are picked from.
const PICK_SALT: u64 = 0x11;

/// The order the followed artists are tried in, as indices into `artists`.
///
/// One complete pass over the positions: every artist appears once, so no open
/// asks the same artist twice.
pub fn order(artists: usize, seed: Seed) -> Vec<usize> {
    let mut order: Vec<usize> = (0..artists).collect();
    let mut state = seed.state();
    for last in (1..artists).rev() {
        let other = below(&mut state, last + 1);
        order.swap(last, other);
    }
    order
}

/// [`DRAWN`] positions among the first [`NEIGHBOURS`] items, ascending.
pub fn pick(cards: usize, seed: Seed) -> Vec<usize> {
    sample(cards.min(NEIGHBOURS), seed, DRAWN)
}

/// What the because shelf holds between opens.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct State {
    /// The page's seed for this open, so a held seed repeats the shelf.
    seed: Seed,
    /// How many followed artists this open has asked.
    tried: usize,
    /// The artist an answer is outstanding for.
    asked: Option<String>,
    /// The followed artist's name, which the head draws.
    seed_name: String,
    /// The related artists the row draws.
    cards: Vec<FollowedArtist>,
    /// The failure that stopped the shelf's last read.
    failure: Option<Failure>,
}

impl State {
    /// Starts this open, from the page's own seed.
    pub fn open(&mut self, seed: Seed) {
        self.forget();
        self.seed = seed;
    }

    /// Folds one answer the shelf asked for into its state.
    ///
    /// An answer for any artist but the outstanding one is another open's, or a
    /// read this shelf never asked for, so it is dropped rather than drawn. An
    /// answer with no artists leaves nothing to draw and lets the next ask try
    /// the next followed artist.
    pub fn land(&mut self, answer: &HomeAnswer) {
        let HomeAnswer::Related { artist, cards } = answer else {
            return;
        };
        if self.asked.as_deref() != Some(artist.as_str()) {
            return;
        }
        self.asked = None;
        if cards.is_empty() {
            return;
        }
        self.cards = pick(cards.len(), self.seed.split(PICK_SALT))
            .into_iter()
            .filter_map(|at| cards.get(at).cloned())
            .collect();
    }

    /// Records that one of the shelf's reads failed.
    pub fn fail(&mut self, ask: &HomeAsk, failure: Failure) {
        let HomeAsk::Related { artist } = ask else {
            return;
        };
        if self.asked.as_deref() != Some(artist.as_str()) {
            return;
        }
        self.asked = None;
        self.failure = Some(failure);
    }

    /// Drops everything the last open read, so a retry asks again.
    ///
    /// The seed is kept: a retry belongs to the open it retries, so it asks the
    /// same first artist rather than a new one.
    pub fn forget(&mut self) {
        self.tried = 0;
        self.asked = None;
        self.seed_name.clear();
        self.cards.clear();
        self.failure = None;
    }

    /// The seed this open draws with.
    pub fn seed(&self) -> Seed {
        self.seed
    }

    /// How many followed artists this open has asked.
    pub fn tried(&self) -> usize {
        self.tried
    }

    /// The artist an answer is outstanding for.
    pub fn asked(&self) -> Option<&str> {
        self.asked.as_deref()
    }

    /// The failure that stopped the shelf's last read.
    pub fn failure(&self) -> Option<Failure> {
        self.failure
    }

    /// The index `artists` gives the followed artist this open tries next.
    ///
    /// `None` once every one of them has been asked.
    pub fn next_artist(&self, artists: usize) -> Option<usize> {
        order(artists, self.seed.split(ORDER_SALT))
            .get(self.tried)
            .copied()
    }

    /// Records that `artist` is the read now outstanding, and whose row it is.
    pub fn ask(&mut self, artist: &FollowedArtist) {
        self.asked = Some(artist.uri.clone());
        self.seed_name = artist.name.clone();
        self.tried = self.tried.saturating_add(1);
    }

    /// Whether the row has cards to draw.
    pub fn has_cards(&self) -> bool {
        !self.cards.is_empty()
    }

    /// What the shelf draws, once an answer gave it cards.
    pub fn row(&self) -> Option<Row<'_>> {
        self.has_cards().then_some(Row {
            seed_name: &self.seed_name,
            cards: &self.cards,
        })
    }
}

/// What the because shelf draws, borrowed from the library for one frame.
///
/// It owns nothing: everything it draws is a list the library already holds, so
/// a frame costs no copies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Row<'a> {
    /// The followed artist the cards are related to.
    pub seed_name: &'a str,
    /// The related artists the row draws.
    pub cards: &'a [FollowedArtist],
}

#[cfg(test)]
mod tests;
