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
mod tests {
    use super::{ASK_MAX, Ask, KNOWN_MAX, Portrait, Portraits, Slot, Surface};
    use crate::message::{Command, Update};
    use takofy_model::{ArtistPortrait, CoverSet, CoverUrl};

    fn cover(tail: &str) -> CoverSet {
        CoverSet::single(
            CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url"),
        )
    }

    fn portrait(artist: &str, cover: Option<CoverSet>) -> ArtistPortrait {
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

    fn menu(q: &str) -> Option<(Surface, &str)> {
        Some((Surface::Menu, q))
    }

    fn page(q: &str) -> Option<(Surface, &str)> {
        Some((Surface::Page, q))
    }

    fn slot(surface: Surface, key: &str, upto: usize) -> Slot {
        Slot {
            surface,
            key: key.to_owned(),
            upto,
        }
    }

    #[test]
    fn get_reports_unread_absent_and_known() {
        let mut portraits = Portraits::default();
        assert_eq!(portraits.get("a"), Portrait::Unread);
        portraits.apply(
            &reply("q", vec![portrait("a", Some(cover("1")))]),
            menu("q"),
        );
        assert_eq!(
            portraits.get("a"),
            Portrait::Known(&cover("1")),
            "a known portrait hands back its url"
        );
        portraits.apply(&reply("q", vec![portrait("b", None)]), menu("q"));
        assert_eq!(portraits.get("b"), Portrait::Absent);
    }

    #[test]
    fn due_returns_only_unknown_artists_in_order() {
        let mut portraits = Portraits::default();
        portraits.apply(&reply("q", vec![portrait("a", None)]), menu("q"));
        let ask = portraits
            .due(Surface::Page, "q2", &refs(&["b", "a", "c"]))
            .expect("two are unread");
        assert_eq!(ask.artists, shown(&["b", "c"]));
        assert_eq!(ask.slot, slot(Surface::Page, "q2", 3));
    }

    #[test]
    fn nothing_is_due_when_every_shown_artist_is_known() {
        let mut portraits = Portraits::default();
        portraits.apply(
            &reply("q", vec![portrait("a", None), portrait("b", None)]),
            menu("q"),
        );
        assert_eq!(portraits.due(Surface::Menu, "q2", &refs(&["a", "b"])), None);
        assert_eq!(portraits.due(Surface::Menu, "q2", &[]), None);
    }

    #[test]
    fn ask_puts_the_read_in_flight_and_a_second_due_waits() {
        let mut portraits = Portraits::default();
        let ask = portraits
            .due(Surface::Menu, "q", &refs(&["a"]))
            .expect("a is unread");
        assert_eq!(
            portraits.ask(ask),
            Command::LoadPortraits {
                query: "q".to_owned(),
                artists: shown(&["a"]),
            }
        );
        assert_eq!(
            portraits.due(Surface::Menu, "q", &refs(&["b"])),
            None,
            "one read at a time"
        );
        portraits.apply(&failed("q"), menu("q"));
        assert_eq!(portraits.due(Surface::Menu, "q", &refs(&["a"])), None);
    }

    #[test]
    fn a_current_reply_fills_known_including_absences() {
        let mut portraits = Portraits::default();
        let ask = portraits
            .due(Surface::Menu, "q", &refs(&["a", "b"]))
            .expect("both are unread");
        portraits.ask(ask);
        portraits.apply(
            &reply(
                "q",
                vec![portrait("a", Some(cover("1"))), portrait("b", None)],
            ),
            menu("q"),
        );
        assert_eq!(portraits.get("a"), Portrait::Known(&cover("1")));
        assert_eq!(portraits.get("b"), Portrait::Absent);
    }

    #[test]
    fn a_current_reply_records_an_unanswered_request_as_absent() {
        let mut portraits = Portraits::default();
        let ask = portraits
            .due(Surface::Menu, "q", &refs(&["a", "b"]))
            .expect("both are unread");
        portraits.ask(ask);
        portraits.apply(&reply("q", vec![portrait("a", None)]), menu("q"));
        assert_eq!(
            portraits.get("b"),
            Portrait::Absent,
            "an omitted artist falls back to its album cover, not a raised slot"
        );
    }

    #[test]
    fn a_reply_for_another_query_changes_nothing_but_frees_the_wire() {
        let mut portraits = Portraits::default();
        let ask = portraits
            .due(Surface::Menu, "old", &refs(&["a"]))
            .expect("a is unread");
        portraits.ask(ask);
        portraits.apply(
            &reply("old", vec![portrait("a", Some(cover("1")))]),
            menu("new"),
        );
        assert_eq!(portraits.get("a"), Portrait::Unread);
        assert_eq!(
            portraits.due(Surface::Menu, "new", &refs(&["a"])),
            Some(Ask {
                slot: slot(Surface::Menu, "new", 1),
                artists: shown(&["a"]),
            }),
            "the stale reply did not block the new key"
        );
    }

    #[test]
    fn a_failure_falls_back_to_the_album_cover() {
        let mut portraits = Portraits::default();
        let ask = portraits
            .due(Surface::Menu, "q", &refs(&["a"]))
            .expect("a is unread");
        portraits.ask(ask);
        portraits.apply(&failed("q"), menu("q"));
        assert_eq!(
            portraits.get("a"),
            Portrait::Absent,
            "a face that never came is the stand-in, not an empty slot"
        );
        assert_eq!(
            portraits.due(Surface::Menu, "q", &refs(&["a"])),
            None,
            "and the same key does not ask again"
        );
    }

    #[test]
    fn a_key_change_frees_the_ask_slot_for_its_own_artists() {
        let mut portraits = Portraits::default();
        let ask = portraits
            .due(Surface::Menu, "ab", &refs(&["a"]))
            .expect("a is unread");
        portraits.ask(ask);
        portraits.apply(&failed("ab"), menu("ab"));
        assert_eq!(
            portraits.due(Surface::Menu, "abc", &refs(&["b"])),
            Some(Ask {
                slot: slot(Surface::Menu, "abc", 1),
                artists: shown(&["b"]),
            }),
            "another key is not blocked by the failed one"
        );
        assert_eq!(portraits.due(Surface::Menu, "ab", &refs(&["a"])), None);
    }

    #[test]
    fn a_stale_reply_clears_the_ask_slot_when_it_matches() {
        let mut portraits = Portraits::default();
        let ask = portraits
            .due(Surface::Menu, "old", &refs(&["a"]))
            .expect("a is unread");
        portraits.ask(ask);
        portraits.apply(&reply("old", vec![portrait("a", None)]), menu("new"));
        assert_eq!(
            portraits.due(Surface::Menu, "old", &refs(&["a"])),
            Some(Ask {
                slot: slot(Surface::Menu, "old", 1),
                artists: shown(&["a"]),
            }),
            "coming back to the old query asks again"
        );
    }

    #[test]
    fn known_max_evicts_a_face_no_surface_is_showing_and_keeps_the_shown_ones() {
        let mut portraits = Portraits::default();
        let mut batch: Vec<ArtistPortrait> = (0..KNOWN_MAX)
            .map(|n| portrait(&format!("artist{n}"), None))
            .collect();
        batch.push(portrait("menu", None));
        batch.push(portrait("page", None));
        portraits.set_shown(&shown(&["menu", "page"]));
        portraits.apply(&reply("q", batch), menu("q"));

        assert_eq!(
            portraits.get("menu"),
            Portrait::Absent,
            "both windows survive the eviction"
        );
        assert_eq!(portraits.get("page"), Portrait::Absent);
        assert!(portraits.known.len() <= KNOWN_MAX);
        assert!(
            !portraits.known.contains_key("artist0"),
            "the oldest face that was not shown went"
        );
    }

    #[test]
    fn a_menu_ask_under_a_query_does_not_block_the_pages_ask_under_it() {
        let mut portraits = Portraits::default();
        let ask = portraits
            .due(Surface::Menu, "q", &refs(&["a", "b"]))
            .expect("the menu's two");
        portraits.ask(ask);
        portraits.apply(
            &reply("q", vec![portrait("a", None), portrait("b", None)]),
            menu("q"),
        );
        assert_eq!(
            portraits.due(Surface::Menu, "q", &refs(&["a", "b"])),
            None,
            "the menu already has both"
        );
        let ask = portraits
            .due(Surface::Page, "q", &refs(&["a", "b", "c"]))
            .expect("the page's own third artist");
        assert_eq!(
            ask,
            Ask {
                slot: slot(Surface::Page, "q", 3),
                artists: shown(&["c"]),
            }
        );
    }

    #[test]
    fn one_ask_names_at_most_ten_artists_in_window_order() {
        let artists: Vec<String> = (0..25).map(|n| format!("a{n}")).collect();
        let window: Vec<&str> = artists.iter().map(String::as_str).collect();
        let portraits = Portraits::default();
        let ask = portraits
            .due(Surface::Page, "q", &window)
            .expect("all are unread");
        assert_eq!(ask.artists.len(), ASK_MAX);
        assert_eq!(ask.artists, artists[..ASK_MAX]);
        assert_eq!(ask.slot.upto, ASK_MAX);
    }

    #[test]
    fn a_grown_window_is_a_new_slot_and_the_next_ask_takes_the_rest() {
        let artists: Vec<String> = (0..25).map(|n| format!("a{n}")).collect();
        let window: Vec<&str> = artists.iter().map(String::as_str).collect();
        let ten: Vec<&str> = artists[..10].iter().map(String::as_str).collect();
        let mut portraits = Portraits::default();

        let first = portraits
            .due(Surface::Page, "q", &ten)
            .expect("the ten on screen");
        assert_eq!(first.slot, slot(Surface::Page, "q", 10));
        portraits.ask(first);
        portraits.apply(
            &reply(
                "q",
                artists[..10]
                    .iter()
                    .map(|artist| portrait(artist, Some(cover("face"))))
                    .collect(),
            ),
            page("q"),
        );

        let next = portraits
            .due(Surface::Page, "q", &window)
            .expect("the rest of the revealed window");
        assert_eq!(next.slot, slot(Surface::Page, "q", 20));
        assert_eq!(next.artists, artists[10..20]);
    }

    #[test]
    fn the_same_slot_is_not_asked_twice() {
        let mut portraits = Portraits::default();
        let ask = portraits
            .due(Surface::Page, "q", &refs(&["a"]))
            .expect("a is unread");
        portraits.ask(ask);
        // The wire, not the slot, is what normally blocks the second ask; this
        // isolates the slot's own guard.
        portraits.inflight = None;
        assert_eq!(portraits.due(Surface::Page, "q", &refs(&["a"])), None);
        assert!(portraits.due(Surface::Menu, "q", &refs(&["a"])).is_some());
        assert!(
            portraits
                .due(Surface::Page, "q", &refs(&["a", "b"]))
                .is_some()
        );
        assert!(
            portraits
                .due(Surface::Page, "other", &refs(&["a"]))
                .is_some()
        );
    }

    #[test]
    fn a_reply_is_current_only_for_the_surface_that_asked() {
        let mut portraits = Portraits::default();
        let ask = portraits
            .due(Surface::Page, "q", &refs(&["a"]))
            .expect("a is unread");
        portraits.ask(ask);
        portraits.apply(
            &reply("q", vec![portrait("a", Some(cover("1")))]),
            menu("q"),
        );
        assert_eq!(
            portraits.get("a"),
            Portrait::Unread,
            "the menu is on screen, so the page's reply changes nothing"
        );
        assert_eq!(
            portraits.due(Surface::Menu, "q", &refs(&["a"])),
            Some(Ask {
                slot: slot(Surface::Menu, "q", 1),
                artists: shown(&["a"]),
            }),
            "and the wire was freed for the surface on screen"
        );
    }

    #[test]
    fn a_reply_for_the_page_while_the_page_is_on_screen_is_current() {
        let mut portraits = Portraits::default();
        let ask = portraits
            .due(Surface::Page, "q", &refs(&["a"]))
            .expect("a is unread");
        portraits.ask(ask);
        portraits.apply(
            &reply("q", vec![portrait("a", Some(cover("1")))]),
            page("q"),
        );
        assert_eq!(portraits.get("a"), Portrait::Known(&cover("1")));
    }

    #[test]
    fn a_menu_reply_is_current_when_its_page_is_the_surface_on_screen() {
        let mut portraits = Portraits::default();
        let ask = portraits
            .due(Surface::Menu, "q", &refs(&["a", "b"]))
            .expect("the menu's two");
        portraits.ask(ask);
        // Enter opened the page under the same key before the reply landed. The
        // cache is keyed by artist URI, so the menu's faces are the page's.
        portraits.apply(
            &reply(
                "q",
                vec![portrait("a", Some(cover("1"))), portrait("b", None)],
            ),
            page("q"),
        );
        assert_eq!(portraits.get("a"), Portrait::Known(&cover("1")));
        assert_eq!(portraits.get("b"), Portrait::Absent);
        assert_eq!(
            portraits.due(Surface::Page, "q", &refs(&["a", "b"])),
            None,
            "the page draws the menu's two, so it asks nothing again for them"
        );
    }

    #[test]
    fn note_clears_the_slot_only_for_another_surface_or_key() {
        let mut portraits = Portraits::default();
        let ask = portraits
            .due(Surface::Page, "q", &refs(&["a"]))
            .expect("a is unread");
        portraits.ask(ask);
        portraits.inflight = None;

        portraits.note(page("q"));
        assert_eq!(
            portraits.due(Surface::Page, "q", &refs(&["a"])),
            None,
            "kept"
        );
        portraits.note(menu("q"));
        assert!(
            portraits.due(Surface::Page, "q", &refs(&["a"])).is_some(),
            "the other surface freed it"
        );
    }

    #[test]
    fn note_none_clears_the_slot_and_an_empty_slot_is_never_cleared() {
        let mut portraits = Portraits::default();
        assert_eq!(portraits.asked, None);
        portraits.note(None);
        assert_eq!(portraits.asked, None);

        let ask = portraits
            .due(Surface::Menu, "q", &refs(&["a"]))
            .expect("a is unread");
        portraits.ask(ask);
        portraits.inflight = None;
        portraits.note(None);
        assert_eq!(
            portraits.asked, None,
            "nothing on screen frees the slot for when it returns"
        );
        assert_eq!(portraits.asked_artists, Vec::<String>::new());
    }

    #[test]
    fn an_in_flight_ask_keeps_the_slot_through_a_note() {
        let mut portraits = Portraits::default();
        let ask = portraits
            .due(Surface::Page, "q", &refs(&["a"]))
            .expect("a is unread");
        portraits.ask(ask);
        portraits.note(menu("q"));
        assert_eq!(portraits.asked, Some(slot(Surface::Page, "q", 1)));
    }
}
