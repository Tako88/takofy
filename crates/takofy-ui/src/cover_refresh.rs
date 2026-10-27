//! Which playlist covers must be asked for again after a row edit.
//!
//! A derived cover is a picture the server took from the playlist's first track,
//! so removing, moving or appending that track can make it wrong while the row
//! edit itself says nothing about it. A cover is *due* once such an edit lands,
//! and a playlist is *awaiting* while one or more reads that can answer it are
//! in flight. Reads come back in the order they were issued, so only the last
//! reply for an awaiting playlist is allowed to overwrite the cover.
//!
//! It imports no I/O. Two maps, keyed by playlist URI.

use std::collections::{HashMap, HashSet};

use takofy_model::TrackEntry;

/// The derived covers that must be asked for again, and the asks in flight.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoverRefresh {
    /// Playlists whose derived cover a row edit may have invalidated.
    due: HashSet<String>,
    /// Playlists with reads in flight, and how many are still unanswered.
    awaiting: HashMap<String, u32>,
}

impl CoverRefresh {
    /// Marks the derived cover of `uri` due, so it is asked for again.
    ///
    /// This only queues the ask. Clearing the entry's cover and the cached
    /// image belongs to the caller that drops them.
    pub fn drop_for(&mut self, uri: &str) {
        self.due.insert(uri.to_owned());
    }

    /// Takes one due playlist that is listed and is not the kept page.
    ///
    /// The returned URI gains an ask in [`Self::awaiting`], so the reply to the
    /// read that follows may write its cover. A due playlist that is not listed
    /// stays due until [`Self::retain`] prunes it.
    ///
    /// `keep` is for the page whose read is already in flight: the caller passes
    /// it only while that page's tracks are loading, because that read answers
    /// its cover. A page that is merely on screen and settled is not kept, or its
    /// cover would never be asked for again.
    pub fn take_due(&mut self, present: &[String], keep: Option<&str>) -> Option<String> {
        if self.due.is_empty() {
            return None;
        }
        let uri = present
            .iter()
            .find(|uri| Some(uri.as_str()) != keep && self.due.contains(*uri))?
            .clone();
        self.due.remove(&uri);
        self.count_ask(&uri);
        Some(uri)
    }

    /// Marks `uri`'s re-read as issued, if it was due.
    pub fn asked(&mut self, uri: &str) {
        if self.due.remove(uri) {
            self.count_ask(uri);
        }
    }

    /// Whether `uri`'s reply is the last one in flight, consuming that ask.
    ///
    /// A playlist asked twice is answered twice: the earlier reply is
    /// superseded, and only the one that empties the count is authoritative.
    pub fn answer(&mut self, uri: &str) -> bool {
        let Some(count) = self.awaiting.get_mut(uri) else {
            return false;
        };
        *count -= 1;
        let last = *count == 0;
        if last {
            self.awaiting.remove(uri);
        }
        last
    }

    /// Forgets every playlist that is no longer listed.
    pub fn retain(&mut self, present: &[String]) {
        self.due.retain(|uri| present.contains(uri));
        self.awaiting.retain(|uri, _| present.contains(uri));
    }

    /// Forgets one playlist entirely, so a gone page is never asked for.
    pub fn forget(&mut self, uri: &str) {
        self.due.remove(uri);
        self.awaiting.remove(uri);
    }

    /// Records one more read for `uri` in flight.
    fn count_ask(&mut self, uri: &str) {
        *self.awaiting.entry(uri.to_owned()).or_insert(0) += 1;
    }
}

/// The URI of the first row that is a track a playlist can hold.
///
/// An episode or a local file in front is not a track, so it cannot be the one a
/// derived cover is taken from.
pub fn first_track_uri(rows: &[TrackEntry]) -> Option<&str> {
    rows.iter()
        .find(|row| row.is_track())
        .map(|row| row.uri.as_str())
}

#[cfg(test)]
mod tests;
