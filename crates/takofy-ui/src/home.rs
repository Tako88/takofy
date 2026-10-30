//! Home's own state: the seed its shelves draw from, the labels the liked songs
//! need before they can be drawn as cards, and one state per section.
//!
//! The rest of Home is pure logic in the child modules: the seed and the draws
//! ([`seed`], [`draw`]), the greeting, the state machine that decides what the
//! dated shelf says ([`outlook`]), and the section table ([`section`]).

use std::collections::{HashMap, HashSet};

use takofy_model::home::{HomeAnswer, HomeAsk};
use takofy_model::{PlaylistEntry, SavedAlbum, SavedAt, TrackEntry};

use crate::message::Failure;

pub mod because;
pub mod counted;
pub mod draw;
pub mod episodes;
mod greeting;
mod outlook;
mod section;
mod seed;
pub mod stations;

pub use draw::{pick, pool};
pub use greeting::greeting;
pub use outlook::{Intake, Outlook, Phase, Tally, outlook};
pub use section::{Budget, Presence, Section, Slot, inherit, settle};
pub use seed::Seed;

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

/// How much room the page has for its shelves this frame.
///
/// It is read once, before any shelf draws, so the room the dated shelf solves
/// its grid for is the room its cards are placed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fit {
    /// The card columns the dated shelf's grid solves to.
    pub columns: usize,
}

/// Home's five shelves for one frame, borrowed from the library.
#[derive(Debug)]
pub struct Sections<'a> {
    /// The page's first shelf: what was saved most recently.
    pub dated: Shelf<'a>,
    pub stations: Section<stations::Band<'a>>,
    pub because: Section<because::Row<'a>>,
    pub counted: Section<counted::Tiles<'a>>,
    pub episodes: Section<episodes::List<'a>>,
}

impl Sections<'_> {
    /// What `slot` is doing this frame.
    pub fn presence(&self, slot: Slot) -> Presence {
        match slot {
            Slot::Dated => dated_presence(&self.dated),
            Slot::Stations => self.stations.presence(),
            Slot::Because => self.because.presence(),
            Slot::Counted => self.counted.presence(),
            Slot::Episodes => self.episodes.presence(),
        }
    }

    /// The four shelves below the dated one, in the page's order.
    pub fn others(&self) -> [Presence; 4] {
        [
            self.presence(Slot::Stations),
            self.presence(Slot::Because),
            self.presence(Slot::Counted),
            self.presence(Slot::Episodes),
        ]
    }
}

/// What the dated shelf says, in the presence the page reads off it.
fn dated_presence(shelf: &Shelf<'_>) -> Presence {
    match shelf {
        Shelf::Waiting => Presence::Waiting,
        Shelf::Unreadable(_) | Shelf::Incomplete(_) => Presence::Failed,
        Shelf::DayOne | Shelf::Quiet => Presence::Absent,
        Shelf::Cards(_) => Presence::Ready,
    }
}

/// Whether the dated shelf draws beside `others`.
pub fn dated_draws(shelf: &Shelf<'_>, others: [Presence; 4]) -> bool {
    match shelf {
        Shelf::Waiting | Shelf::Cards(_) => true,
        Shelf::Unreadable(_) => true,
        Shelf::Incomplete(_) => true,
        Shelf::Quiet => !others
            .iter()
            .any(|presence| matches!(presence, Presence::Ready | Presence::Waiting)),
        Shelf::DayOne => others.iter().all(|presence| *presence == Presence::Absent),
    }
}

/// The shelf's seed, the liked songs' labels, and one state per section.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Home {
    seed: Seed,
    /// How many times Home has been opened, so two opens draw different shelves.
    opens: u64,
    /// The label read's answer for each asked URI; `None` is a track the shelf
    /// cannot draw.
    labels: HashMap<String, Option<TrackEntry>>,
    /// How long each labelled track runs, by URI, for the rows that name a
    /// duration without needing a name.
    durations: HashMap<String, Option<u32>>,
    /// The failure that stopped the last label read, kept so the shelf can name
    /// it instead of calling the misses a quiet library.
    failed: Option<Failure>,
    /// The URIs a label read was sent for, so each one is sent once.
    asked: HashSet<String>,
    /// The URIs a lengths read was sent for. Kept apart from `asked`, because a
    /// lengths answer names no track the page can draw: it must not stop the
    /// dated shelf asking for the same song's label.
    lengths: HashSet<String>,
    /// The stations built from the library's own seeds.
    pub stations: stations::State,
    /// The artists related to one followed artist.
    pub because: because::State,
    /// The artists the library holds the most of.
    pub counted: counted::State,
    /// The newest episodes of the followed shows.
    pub episodes: episodes::State,
    /// The reads one open still allows.
    pub budget: Budget,
}

impl Home {
    /// Draws this open's seeds, keeping the count of opens, and refills the budget.
    pub fn open(&mut self, now_ms: u64) {
        self.opens = self.opens.saturating_add(1);
        self.seed = Seed::for_open(now_ms, self.opens);
        self.budget = Budget::default();
        self.stations.open(self.seed);
        self.because.open(self.seed);
        self.counted.open(self.seed);
        self.episodes.open(self.seed);
    }

    /// Folds one answer into the shelf that asked for it.
    pub fn land(&mut self, answer: &HomeAnswer) {
        if let HomeAnswer::Labelled { asked, named } = answer {
            // A read any shelf sent still says how long each track it named runs,
            // which is all a station row wants from it.
            self.fill_durations(asked, named);
        }
        if matches!(
            answer,
            HomeAnswer::Labelled { .. } | HomeAnswer::Portrait { .. }
        ) {
            self.counted.land(answer);
        }
        self.stations.land(answer);
        self.because.land(answer);
        self.episodes.land(answer);
    }

    /// Records that the read `ask` named failed.
    pub fn fail(&mut self, ask: &HomeAsk, failure: Failure) {
        if matches!(ask, HomeAsk::Labels { .. } | HomeAsk::Portrait { .. }) {
            self.counted.fail(ask, failure);
        }
        self.stations.fail(ask, failure);
        self.because.fail(ask, failure);
        self.episodes.fail(ask, failure);
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
        self.fill_durations(asked, named);
        for uri in asked {
            let usable = named
                .iter()
                .find(|track| track.uri == *uri)
                .filter(|track| track.named && track.album_uri.is_some())
                .cloned();
            self.labels.insert(uri.clone(), usable);
        }
    }

    /// How long each asked track runs, recorded from any label read.
    fn fill_durations(&mut self, asked: &[String], named: &[TrackEntry]) {
        for uri in asked {
            let answer = named.iter().find(|track| track.uri == *uri);
            self.durations
                .insert(uri.clone(), answer.and_then(|track| track.duration_ms));
        }
    }

    /// How long the track at `uri` runs, when a label read named it.
    pub fn duration(&self, uri: &str) -> Option<u32> {
        self.durations.get(uri).copied().flatten()
    }

    /// Records that the label read sent for `asked` failed.
    ///
    /// The URIs are asked and answered so nothing asks again on its own, and
    /// the failure is kept rather than stored as a miss, so the shelf can say
    /// what happened and offer the retry that reads them again.
    pub fn fail_labels(&mut self, asked: &[String], failure: Failure) {
        for uri in asked {
            self.labels.insert(uri.clone(), None);
        }
        self.failed = Some(failure);
    }

    /// Drops what `slot` read, so its retry asks again and the others keep their answers.
    pub fn forget_slot(&mut self, slot: Slot) {
        match slot {
            Slot::Dated => {
                self.labels.clear();
                self.asked.clear();
                self.failed = None;
            }
            Slot::Stations => self.stations.forget(),
            Slot::Because => self.because.forget(),
            Slot::Counted => self.counted.forget(),
            Slot::Episodes => self.episodes.forget(),
        }
    }

    /// Drops everything the last open read, so a retry asks for all of it again.
    pub fn forget(&mut self) {
        for slot in Slot::ORDER {
            self.forget_slot(slot);
        }
    }

    /// The failure that stopped the last label read, if one did.
    pub fn failed(&self) -> Option<Failure> {
        self.failed
    }

    /// Whether a label read was sent for `uri`.
    pub fn asked(&self, uri: &str) -> bool {
        self.asked.contains(uri)
    }

    /// Whether a lengths read was sent for `uri`.
    pub fn lengths_asked(&self, uri: &str) -> bool {
        self.lengths.contains(uri)
    }

    /// Records the URIs a lengths read was sent for.
    pub fn mark_lengths(&mut self, uris: &[String]) {
        self.lengths.extend(uris.iter().cloned());
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
