//! The artist portraits a search answer draws, and the one read that fills them.
//!
//! The menu asks once per current answer. A reply for a query the reader has
//! typed past changes nothing, so a newer answer never inherits an older one's
//! faces. The cache is keyed by the artist's own URI, so an artist met in an
//! earlier query draws on the frame the row appears.

use std::collections::{HashMap, HashSet, VecDeque};

use takofy_model::{ArtistPortrait, CoverUrl};

use crate::message::{Command, Update};

/// The most artist portraits the cache holds before it evicts a face the menu is
/// not showing.
pub const KNOWN_MAX: usize = 256;

/// What the view knows about one artist's portrait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Portrait<'a> {
    /// No read has answered for this artist yet.
    Unread,
    /// The record answered and carries no portrait group.
    Absent,
    /// The record's portrait, for the art store to fetch.
    Known(&'a CoverUrl),
}

/// The portraits held between frames, and the read that is filling them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Portraits {
    /// Artist URI → portrait; `None` means the record has no portrait group.
    known: HashMap<String, Option<CoverUrl>>,
    /// Insertion order, so an eviction can drop the oldest face first.
    order: VecDeque<String>,
    /// The query on the wire, if a read is out.
    inflight: Option<String>,
    /// The query last asked for, which blocks a retry until the key moves.
    asked: Option<String>,
    /// The artists that ask named, so an unanswered one can be recorded as
    /// absent when a successful reply omits it.
    asked_artists: Vec<String>,
    /// The artists the current answer's band shows, which eviction keeps.
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

    /// The artists in `shown` with no answer yet, in order.
    ///
    /// `None` while a read is out, when this key was already asked, or when
    /// every shown artist is known.
    pub fn due(&self, key: &str, shown: &[&str]) -> Option<Vec<String>> {
        if self.inflight.is_some() || self.asked.as_deref() == Some(key) {
            return None;
        }
        let missing: Vec<String> = shown
            .iter()
            .filter(|artist| !self.known.contains_key(**artist))
            .map(|artist| (*artist).to_owned())
            .collect();
        (!missing.is_empty()).then_some(missing)
    }

    /// Puts the read for `artists` on the wire under `key`.
    pub fn ask(&mut self, key: &str, artists: Vec<String>) -> Command {
        self.inflight = Some(key.to_owned());
        self.asked = Some(key.to_owned());
        self.asked_artists = artists.clone();
        Command::LoadPortraits {
            query: key.to_owned(),
            artists,
        }
    }

    /// Records the artists the current answer shows, so a full cache keeps them.
    pub fn set_shown(&mut self, shown: &[String]) {
        self.shown = shown.to_vec();
    }

    /// Clears the single ask slot when the reader has moved to another key.
    ///
    /// A failure on `ab`, a trip through `abc` and back to `ab` then asks again:
    /// the slot is about the current key, not the last failed one.
    pub fn note(&mut self, key: Option<&str>) {
        if self.inflight.is_none() && self.asked.as_deref() != key {
            self.asked = None;
            self.asked_artists.clear();
        }
    }

    /// Folds one portrait reply in, against the text the field stands for now.
    ///
    /// A stale reply changes nothing on screen. A current successful reply also
    /// records every artist that was asked for but did not answer as absent, so
    /// the row falls back to its album cover instead of staying a raised slot.
    pub fn apply(&mut self, update: &Update, current: Option<&str>) {
        let (query, portraits) = match update {
            Update::Portraits { query, portraits } => (query, Some(portraits.as_slice())),
            Update::PortraitsFailed { query } => (query, None),
            _ => return,
        };
        if self.inflight.as_deref() == Some(query.as_str()) {
            self.inflight = None;
        }
        if current != Some(query.as_str()) {
            if self.asked.as_deref() == Some(query.as_str()) {
                self.asked = None;
                self.asked_artists.clear();
            }
            return;
        }
        let Some(portraits) = portraits else {
            // The faces did not arrive: fall back to the album cover rather
            // than leave a raised slot that will never fill. `asked` keeps it
            // from asking the same key again.
            if self.asked.as_deref() == Some(query.as_str()) {
                for artist in std::mem::take(&mut self.asked_artists) {
                    self.remember(artist, None);
                }
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
        if self.asked.as_deref() == Some(query.as_str()) {
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
    }

    /// Inserts one artist, evicting the oldest face the menu is not showing.
    fn remember(&mut self, artist: String, portrait: Option<CoverUrl>) {
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
mod tests {
    use super::{KNOWN_MAX, Portrait, Portraits};
    use crate::message::{Command, Update};
    use takofy_model::{ArtistPortrait, CoverUrl};

    fn cover(tail: &str) -> CoverUrl {
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
    }

    fn portrait(artist: &str, cover: Option<CoverUrl>) -> ArtistPortrait {
        ArtistPortrait {
            artist: artist.to_owned(),
            portrait: cover,
        }
    }

    fn reply(query: &str, portraits: Vec<ArtistPortrait>) -> Update {
        Update::Portraits {
            query: query.to_owned(),
            portraits,
        }
    }

    fn failed(query: &str) -> Update {
        Update::PortraitsFailed {
            query: query.to_owned(),
        }
    }

    fn shown(artists: &[&str]) -> Vec<String> {
        artists.iter().map(|artist| (*artist).to_owned()).collect()
    }

    fn refs<'a>(artists: &'a [&'a str]) -> Vec<&'a str> {
        artists.to_vec()
    }

    #[test]
    fn get_reports_unread_absent_and_known() {
        let mut portraits = Portraits::default();
        assert_eq!(portraits.get("a"), Portrait::Unread);
        portraits.apply(
            &reply("q", vec![portrait("a", Some(cover("1")))]),
            Some("q"),
        );
        assert_eq!(
            portraits.get("a"),
            Portrait::Known(&cover("1")),
            "a known portrait hands back its url"
        );
        portraits.apply(&reply("q", vec![portrait("b", None)]), Some("q"));
        assert_eq!(portraits.get("b"), Portrait::Absent);
    }

    #[test]
    fn due_returns_only_unknown_artists_in_order() {
        let mut portraits = Portraits::default();
        portraits.apply(&reply("q", vec![portrait("a", None)]), Some("q"));
        assert_eq!(
            portraits.due("q2", &refs(&["b", "a", "c"])),
            Some(shown(&["b", "c"]))
        );
    }

    #[test]
    fn nothing_is_due_when_every_shown_artist_is_known() {
        let mut portraits = Portraits::default();
        portraits.apply(
            &reply("q", vec![portrait("a", None), portrait("b", None)]),
            Some("q"),
        );
        assert_eq!(portraits.due("q2", &refs(&["a", "b"])), None);
        assert_eq!(portraits.due("q2", &[]), None);
    }

    #[test]
    fn ask_puts_the_read_in_flight_and_a_second_due_waits() {
        let mut portraits = Portraits::default();
        assert_eq!(
            portraits.ask("q", shown(&["a"])),
            Command::LoadPortraits {
                query: "q".to_owned(),
                artists: shown(&["a"]),
            }
        );
        assert_eq!(portraits.due("q", &refs(&["b"])), None);
        portraits.apply(&failed("q"), Some("q"));
        assert_eq!(portraits.due("q", &refs(&["b"])), None);
    }

    #[test]
    fn a_current_reply_fills_known_including_absences() {
        let mut portraits = Portraits::default();
        portraits.ask("q", shown(&["a", "b"]));
        portraits.apply(
            &reply(
                "q",
                vec![portrait("a", Some(cover("1"))), portrait("b", None)],
            ),
            Some("q"),
        );
        assert_eq!(portraits.get("a"), Portrait::Known(&cover("1")));
        assert_eq!(portraits.get("b"), Portrait::Absent);
    }

    #[test]
    fn a_current_reply_records_an_unanswered_request_as_absent() {
        let mut portraits = Portraits::default();
        portraits.ask("q", shown(&["a", "b"]));
        portraits.apply(&reply("q", vec![portrait("a", None)]), Some("q"));
        assert_eq!(
            portraits.get("b"),
            Portrait::Absent,
            "an omitted artist falls back to its album cover, not a raised slot"
        );
    }

    #[test]
    fn a_reply_for_another_query_changes_nothing_but_frees_the_wire() {
        let mut portraits = Portraits::default();
        portraits.ask("old", shown(&["a"]));
        portraits.apply(
            &reply("old", vec![portrait("a", Some(cover("1")))]),
            Some("new"),
        );
        assert_eq!(portraits.get("a"), Portrait::Unread);
        assert_eq!(
            portraits.due("new", &refs(&["a"])),
            Some(shown(&["a"])),
            "the stale reply did not block the new key"
        );
    }

    #[test]
    fn a_failure_falls_back_to_the_album_cover() {
        let mut portraits = Portraits::default();
        portraits.ask("q", shown(&["a"]));
        portraits.apply(&failed("q"), Some("q"));
        assert_eq!(
            portraits.get("a"),
            Portrait::Absent,
            "a face that never came is the stand-in, not an empty slot"
        );
        assert_eq!(
            portraits.due("q", &refs(&["a"])),
            None,
            "and the same key does not ask again"
        );
    }

    #[test]
    fn a_key_change_frees_the_ask_slot_for_its_own_artists() {
        let mut portraits = Portraits::default();
        portraits.ask("ab", shown(&["a"]));
        portraits.apply(&failed("ab"), Some("ab"));
        assert_eq!(
            portraits.due("abc", &refs(&["b"])),
            Some(shown(&["b"])),
            "another key is not blocked by the failed one"
        );
        assert_eq!(portraits.due("ab", &refs(&["a"])), None);
    }

    #[test]
    fn a_stale_reply_clears_the_ask_slot_when_it_matches() {
        let mut portraits = Portraits::default();
        portraits.ask("old", shown(&["a"]));
        portraits.apply(&reply("old", vec![portrait("a", None)]), Some("new"));
        assert_eq!(
            portraits.due("old", &refs(&["a"])),
            Some(shown(&["a"])),
            "coming back to the old query asks again"
        );
    }

    #[test]
    fn known_max_evicts_a_face_the_menu_is_not_showing_and_keeps_the_shown_one() {
        let mut portraits = Portraits::default();
        let mut batch: Vec<ArtistPortrait> = (0..KNOWN_MAX)
            .map(|n| portrait(&format!("artist{n}"), None))
            .collect();
        batch.push(portrait("menu", None));
        portraits.set_shown(&shown(&["menu"]));
        portraits.apply(&reply("q", batch), Some("q"));

        assert_eq!(
            portraits.get("menu"),
            Portrait::Absent,
            "the artist the menu shows survives the eviction"
        );
        assert!(portraits.known.len() <= KNOWN_MAX);
        assert!(
            !portraits.known.contains_key("artist0"),
            "the oldest face that was not shown went"
        );
    }
}
