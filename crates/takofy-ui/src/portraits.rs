//! The artist portraits a search answer draws, and the one read that fills them.
//!
//! Two surfaces draw faces — the drop-down and the results page — and they ask
//! through one slot, so only one portrait read is ever on the wire. A reply is
//! current only while the surface that asked it is the surface on screen, so a
//! newer answer never inherits an older one's faces. The cache is keyed by the
//! artist's own URI, so an artist met in an earlier query draws on the frame the
//! row appears.

use std::collections::{HashMap, HashSet, VecDeque};

use takofy_model::{ArtistPortrait, CoverSet};

use crate::message::{Command, Update};

/// The most artist portraits the cache holds before it evicts a face neither
/// surface is showing.
pub const KNOWN_MAX: usize = 256;

/// The most artists one read may name. The core documents at most ten in one
/// request, so a window wider than that is filled by successive asks.
pub const ASK_MAX: usize = 10;

/// What the view knows about one artist's portrait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Portrait<'a> {
    /// No read has answered for this artist yet.
    Unread,
    /// The record answered and carries no portrait group.
    Absent,
    /// The record's portrait, for the art store to fetch.
    Known(&'a CoverSet),
}

/// Which surface a face was asked for: the drop-down, or the results page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Menu,
    Page,
}

/// What the single ask slot holds: who asked, for which query, and how far into
/// the window the ask reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub surface: Surface,
    pub key: String,
    /// The artist index just past the last one the ask named, so a grown window
    /// is a different slot.
    pub upto: usize,
}

/// One portrait read about to be sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub slot: Slot,
    pub artists: Vec<String>,
}

/// The portraits held between frames, and the read that is filling them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Portraits {
    /// Artist URI → portrait; `None` means the record has no portrait group.
    known: HashMap<String, Option<CoverSet>>,
    /// Insertion order, so an eviction can drop the oldest face first.
    order: VecDeque<String>,
    /// The query on the wire, if a read is out.
    inflight: Option<String>,
    /// The slot last asked for, which blocks the same ask until the key, the
    /// surface or the window moves.
    asked: Option<Slot>,
    /// The artists that ask named, so an unanswered one can be recorded as
    /// absent when a successful reply omits it.
    asked_artists: Vec<String>,
    /// The artists the surfaces are showing, which eviction keeps.
    shown: Vec<String>,
}

impl Portraits {
    /// What is known about `artist` this frame.
    pub fn get(&self, artist: &str) -> Portrait<'_> {
        match self.known.get(artist) {
            Some(Some(cover)) => Portrait::Known(cover),
            Some(None) => Portrait::Absent,
            None => Portrait::Unread,
        }
    }

    /// The read `surface` needs for `window`, or `None` when there is nothing
    /// to ask.
    ///
    /// The read names at most [`ASK_MAX`] unknown artists, in window order, so a
    /// window wider than one request is filled ask by ask. `None` while a read is
    /// out, when every artist in the window is answered, and when this very slot
    /// is already asked.
    pub fn due(&self, surface: Surface, key: &str, window: &[&str]) -> Option<Ask> {
        if self.inflight.is_some() {
            return None;
        }
        let mut artists = Vec::new();
        let mut upto = 0;
        for (index, artist) in window.iter().enumerate() {
            if self.known.contains_key(*artist) {
                continue;
            }
            if artists.len() == ASK_MAX {
                break;
            }
            artists.push((*artist).to_owned());
            upto = index + 1;
        }
        if artists.is_empty() {
            return None;
        }
        let slot = Slot {
            surface,
            key: key.to_owned(),
            upto,
        };
        if self.asked.as_ref() == Some(&slot) {
            return None;
        }
        Some(Ask { slot, artists })
    }

    /// Puts the read for `ask` on the wire.
    pub fn ask(&mut self, ask: Ask) -> Command {
        self.inflight = Some(ask.slot.key.clone());
        self.asked = Some(ask.slot.clone());
        self.asked_artists = ask.artists.clone();
        Command::LoadPortraits {
            query: ask.slot.key,
            artists: ask.artists,
        }
    }

    /// Records the artists the surfaces are showing, so a full cache keeps them.
    pub fn set_shown(&mut self, shown: &[String]) {
        self.shown = shown.to_vec();
    }

    /// Clears the ask slot when the surface on screen is not the one that asked.
    ///
    /// A failure on `ab`, a trip through `abc` and back to `ab` then asks again:
    /// the slot is about the current surface and key, not the last failed one.
    /// Nothing is cleared while a read is out, so an in-flight ask still owns it.
    pub fn note(&mut self, active: Option<(Surface, &str)>) {
        if self.inflight.is_some() {
            return;
        }
        let current = match (&self.asked, active) {
            (None, _) => true,
            (Some(slot), Some((surface, key))) => slot.surface == surface && slot.key == key,
            (Some(_), None) => false,
        };
        if !current {
            self.asked = None;
            self.asked_artists.clear();
        }
    }

    /// Folds one portrait reply in, against the surface on screen this frame.
    ///
    /// A reply for any other surface or query changes nothing on screen. A
    /// current successful reply also records every artist that was asked for but
    /// did not answer as absent, so the row falls back to its album cover instead
    /// of staying a raised slot.
    pub fn apply(&mut self, update: &Update, current: Option<(Surface, &str)>) {
        let (query, portraits) = match update {
            Update::Portraits { query, portraits } => (query, Some(portraits.as_slice())),
            Update::PortraitsFailed { query } => (query, None),
            _ => return,
        };
        if self.inflight.as_deref() == Some(query.as_str()) {
            self.inflight = None;
        }
        // The surface that asked is the slot's own, while the slot is this
        // query's; a reply with no slot is judged by its query alone.
        let asked = self
            .asked
            .as_ref()
            .filter(|slot| &slot.key == query)
            .map(|slot| slot.surface);
        let is_current = match (asked, current) {
            (Some(surface), Some((active, key))) => {
                key == query
                    && (surface == active
                        // Enter on a menu row opens the page under the same key
                        // before the menu's reply lands. The cache is keyed by
                        // the artist URI, so the faces are the page's faces.
                        // The reverse does not hold: the menu draws two rows,
                        // and a page's wider reply is not about them.
                        || (surface == Surface::Menu && active == Surface::Page))
            }
            (None, Some((_, key))) => key == query,
            _ => false,
        };
        if !is_current {
            if asked.is_some() {
                self.asked = None;
                self.asked_artists.clear();
            }
            return;
        }
        let Some(portraits) = portraits else {
            // The faces did not arrive: fall back to the album cover rather
            // than leave a raised slot that will never fill. `asked` keeps it
            // from asking the same slot again.
            for artist in std::mem::take(&mut self.asked_artists) {
                self.remember(artist, None);
            }
            return;
        };

        let answered: HashSet<&str> = portraits
            .iter()
            .map(|portrait| portrait.artist.as_str())
            .collect();
        for ArtistPortrait { artist, portrait } in portraits {
            self.remember(artist.clone(), portrait.clone());
        }
        let unanswered: Vec<String> = self
            .asked_artists
            .iter()
            .filter(|artist| !answered.contains(artist.as_str()))
            .cloned()
            .collect();
        for artist in unanswered {
            self.remember(artist, None);
        }
        self.asked_artists.clear();
    }

    /// Inserts one artist, evicting the oldest face neither surface is showing.
    fn remember(&mut self, artist: String, portrait: Option<CoverSet>) {
        if let Some(slot) = self.known.get_mut(&artist) {
            *slot = portrait;
            return;
        }
        self.known.insert(artist.clone(), portrait);
        self.order.push_back(artist);
        while self.known.len() > KNOWN_MAX {
            let shown: HashSet<&str> = self.shown.iter().map(String::as_str).collect();
            let Some(index) = self
                .order
                .iter()
                .position(|artist| !shown.contains(artist.as_str()))
            else {
                break;
            };
            if let Some(evicted) = self.order.remove(index) {
                self.known.remove(&evicted);
            }
        }
    }
}

#[cfg(test)]
mod tests;
