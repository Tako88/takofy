//! Home's own state: the seed its shelf draws from, and the labels the liked
//! songs need before they can be drawn as cards.
//!
//! The rest of Home is pure logic in the child modules: the seed and the draw
//! ([`seed`], [`recent`]), the greeting, and the state machine that decides what
//! the page says ([`outlook`]).

use std::collections::{HashMap, HashSet};

use takofy_model::{PlaylistEntry, SavedAlbum, SavedAt, TrackEntry};

use crate::message::Failure;

mod greeting;
mod outlook;
mod recent;
mod seed;

pub use greeting::greeting;
pub use outlook::{Intake, Outlook, Phase, Tally, outlook};
pub use recent::{POOL, pick, pool};
pub use seed::{Seed, below};

/// One thing a card draws, borrowed from the library's own lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Saved<'a> {
    /// A playlist from the rootlist.
    Playlist(&'a PlaylistEntry),
    /// An album from the saved set.
    Album(&'a SavedAlbum),
    /// A liked song, named by its label read.
    Track(&'a TrackEntry),
}

/// One card on the shelf: what it draws and when it was saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Card<'a> {
    pub saved: Saved<'a>,
    /// The date the pool drew the card by.
    pub at: SavedAt,
}

/// What Home draws under the greeting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shelf<'a> {
    /// A read is still coming, so the shelf draws as a skeleton.
    Waiting,
    /// The rootlist failed, so there is no library to build a shelf from.
    Unreadable(Failure),
    /// A source failed and left nothing to draw.
    Incomplete(Failure),
    /// Nothing is saved.
    DayOne,
    /// Things are saved but none is recent.
    Quiet,
    /// The drawn cards, newest first.
    Cards(Vec<Card<'a>>),
}

/// The shelf's seed and the liked songs' labels.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Home {
    seed: Seed,
    /// How many times Home has been opened, so two opens draw different shelves.
    opens: u64,
    /// The label read's answer for each asked URI; `None` is a track the shelf
    /// cannot draw.
    labels: HashMap<String, Option<TrackEntry>>,
    /// The URIs a label read was sent for, so each one is sent once.
    asked: HashSet<String>,
}

impl Home {
    /// Draws this open's seed, keeping the count of opens.
    pub fn open(&mut self, now_ms: u64) {
        self.opens = self.opens.saturating_add(1);
        self.seed = Seed::for_open(now_ms, self.opens);
    }

    /// Records the URIs a label read was sent for, so they are not sent again.
    pub fn mark_asked(&mut self, uris: &[String]) {
        self.asked.extend(uris.iter().cloned());
    }

    /// Folds one label read's answer into the shelf.
    ///
    /// Every URI the read answered is recorded: a track the read did not name,
    /// or named without an album to open, cannot draw a card and is stored as
    /// unusable. Keeping the miss is what stops the shelf waiting on an answer
    /// that already arrived.
    pub fn apply_labels(&mut self, asked: &[String], named: &[TrackEntry]) {
        for uri in asked {
            let usable = named
                .iter()
                .find(|track| track.uri == *uri)
                .filter(|track| track.named && track.album_uri.is_some())
                .cloned();
            self.labels.insert(uri.clone(), usable);
        }
    }

    /// Whether a label read was sent for `uri`.
    pub fn asked(&self, uri: &str) -> bool {
        self.asked.contains(uri)
    }

    /// Whether a label read has answered for `uri`.
    pub fn answered(&self, uri: &str) -> bool {
        self.labels.contains_key(uri)
    }

    /// The named track a label answered for `uri`, when it can draw a card.
    pub fn label(&self, uri: &str) -> Option<&TrackEntry> {
        self.labels.get(uri)?.as_ref()
    }

    /// The seed the shelf draws with.
    pub fn seed(&self) -> Seed {
        self.seed
    }
}

#[cfg(test)]
mod tests;
