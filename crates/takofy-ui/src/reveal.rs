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
mod tests;
