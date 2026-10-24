//! How much of each results block the reader has asked for.
//!
//! A block starts at the page's caps; taking its reveal lifts the cap for good.
//! The reveal belongs to the query it was taken for, so another query never
//! inherits it.

use crate::search_index::{Caps, Kind};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reveal {
    query: Option<String>,
    /// Bumped every time [`Self::point`] starts a query over, so a surface
    /// keyed by the visit starts fresh exactly when the reveal does.
    visit: u32,
    tracks: bool,
    albums: bool,
    artists: bool,
}

impl Reveal {
    /// Aims the reveal at `query`: a different query starts every block over.
    pub fn point(&mut self, query: &str) {
        if self.query.as_deref() != Some(query) {
            let visit = self.visit.wrapping_add(1);
            *self = Self {
                query: Some(query.to_owned()),
                visit,
                ..Self::default()
            };
        }
    }

    /// Which visit to the query this is: the same query keeps it, a new one
    /// bumps it.
    pub fn visit(&self) -> u32 {
        self.visit
    }

    /// Asks for all of `kind`. There is no way back.
    pub fn take(&mut self, kind: Kind) {
        match kind {
            Kind::Tracks => self.tracks = true,
            Kind::Albums => self.albums = true,
            Kind::Artists => self.artists = true,
        }
    }

    /// `base`, with every taken kind unbounded; `base` itself for another query.
    pub fn caps(&self, query: &str, base: Caps) -> Caps {
        if self.query.as_deref() != Some(query) {
            return base;
        }
        let lift = |taken, cap| if taken { usize::MAX } else { cap };
        Caps {
            tracks: lift(self.tracks, base.tracks),
            albums: lift(self.albums, base.albums),
            artists: lift(self.artists, base.artists),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search_index::{Index, PAGE};

    fn pointed(query: &str) -> Reveal {
        let mut reveal = Reveal::default();
        reveal.point(query);
        reveal
    }

    #[test]
    fn a_fresh_reveal_leaves_the_caps_alone() {
        assert_eq!(pointed("q").caps("q", PAGE), PAGE);
        assert_eq!(Reveal::default().caps("q", PAGE), PAGE);
    }

    #[test]
    fn taking_a_kind_unbounds_only_that_kind() {
        let mut reveal = pointed("q");
        reveal.take(Kind::Tracks);
        let caps = reveal.caps("q", PAGE);
        assert_eq!(caps.tracks, usize::MAX);
        assert_eq!((caps.albums, caps.artists), (10, 10));
    }

    #[test]
    fn taking_twice_is_taking_once() {
        let mut reveal = pointed("q");
        reveal.take(Kind::Albums);
        let once = reveal.clone();
        reveal.take(Kind::Albums);
        assert_eq!(reveal, once);
    }

    #[test]
    fn the_same_query_keeps_the_reveal_and_another_resets_it() {
        let mut reveal = pointed("q");
        reveal.take(Kind::Artists);
        reveal.point("q");
        assert_eq!(reveal.caps("q", PAGE).artists, usize::MAX);
        reveal.point("other");
        assert_eq!(reveal.caps("other", PAGE), PAGE);
        reveal.point("q");
        assert_eq!(reveal.caps("q", PAGE), PAGE);
    }

    #[test]
    fn a_new_query_bumps_the_visit_and_a_reentry_keeps_it() {
        let mut reveal = Reveal::default();
        assert_eq!(reveal.visit(), 0);
        reveal.point("a");
        let first = reveal.visit();
        assert_eq!(first, 1);
        reveal.point("a");
        assert_eq!(reveal.visit(), first, "the same query is not a new visit");
        reveal.point("b");
        let second = reveal.visit();
        assert_ne!(second, first, "another query is a new visit");
        reveal.point("a");
        assert_ne!(
            reveal.visit(),
            first,
            "A→B→A starts over rather than landing on A's old visit"
        );
        assert_ne!(reveal.visit(), second);
    }

    #[test]
    fn a_reveal_never_applies_to_another_query() {
        let mut reveal = pointed("q");
        reveal.take(Kind::Tracks);
        assert_eq!(reveal.caps("other", PAGE), PAGE);
    }

    #[test]
    fn a_taken_block_reads_whole_and_the_others_do_not_move() {
        let index = Index {
            tracks: 25,
            ..Index::default()
        };
        let mut reveal = pointed("q");
        let before = index.shape(reveal.caps("q", PAGE));
        assert_eq!(before.bands[0].share(), "10 of 25");
        reveal.take(Kind::Tracks);
        let after = index.shape(reveal.caps("q", PAGE));
        assert_eq!(after.bands[0].shown, 25);
        assert_eq!(after.bands[0].share(), "25");
    }
}
