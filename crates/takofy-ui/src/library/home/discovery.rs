//! Discovery's three rows between frames: the read one owes, and the band the
//! page draws.
//!
//! The day's pools are drawn from the rootlist, the followed artists and the
//! saved albums, so nothing is asked before all three have answered: a pool
//! drawn from half a library would reshuffle when the rest landed. A list that
//! failed leaves the band absent rather than empty, because a lost read is not
//! a library with nothing in it.
//!
//! The day is the binary's, handed in each frame, and the rows keep their
//! positions so a restart on the same day carries on where the last run
//! stopped.

use takofy_model::home::HomeAsk;

use crate::home::discovery::Band;
use std::sync::Arc;

use crate::home::discovery::fresh::Owned;
use crate::library::{Library, Load};
use crate::message::Command;

/// What the three lists a day's pools are drawn from are doing, together.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Inputs {
    /// A list has not answered yet, so a pool drawn now would not hold.
    Waiting,
    /// A list failed, so there is no pool to draw.
    Failed,
    /// Every list has answered.
    Ready,
}

/// What one list the pools are drawn from is doing.
fn inputs_of<T>(load: &Load<T>) -> Inputs {
    match load {
        Load::Idle | Load::Loading => Inputs::Waiting,
        Load::Failed(_) => Inputs::Failed,
        Load::Ready(_) => Inputs::Ready,
    }
}

impl Library {
    /// Whether every list a day's pools are drawn from has answered.
    fn discovery_inputs(&self) -> Inputs {
        let inputs = [
            inputs_of(&self.playlists),
            inputs_of(&self.followed_artists),
            inputs_of(&self.saved_albums),
        ];
        if inputs.contains(&Inputs::Failed) {
            Inputs::Failed
        } else if inputs.contains(&Inputs::Waiting) {
            Inputs::Waiting
        } else {
            Inputs::Ready
        }
    }

    /// The read Discovery owes, if one is due.
    ///
    /// Whether `artist` was followed since Home was opened, which is what turns
    /// a card's Follow tile into the undo of that follow.
    pub(crate) fn followed_here(&self, artist: &str) -> bool {
        self.home.discovery.followed_here(artist)
    }

    /// One ask per call, and the row machine never asks the same seed twice, so
    /// a lookup that never answers costs one ask rather than one a frame.
    pub(super) fn discovery_ask(&mut self) -> Option<Command> {
        if !self.discovery_enabled() {
            return None;
        }
        let day = self.home.discovery.latched()?;
        if self.discovery_inputs() != Inputs::Ready {
            return None;
        }
        if !self.home.discovery.chosen(day) {
            let (Load::Ready(playlists), Load::Ready(artists), Load::Ready(albums)) =
                (&self.playlists, &self.followed_artists, &self.saved_albums)
            else {
                return None;
            };
            let positions = self.hidden.positions_for(day);
            self.home
                .discovery
                .choose(day, playlists, artists, albums, positions);
        }
        let owned = self.owned();
        let fresh = self.home.discovery.fresh_counts(&owned, &self.hidden);
        self.home.discovery.settle(fresh);
        // Recorded whatever moved the rows, and only a change is written.
        let positions = self.home.discovery.positions();
        self.hidden.set_positions(day, positions);
        let seed = self.home.discovery.next_ask()?;
        Some(Command::LoadHome(HomeAsk::Discovery { seed }))
    }
}

/// What Discovery's section draws this frame.
pub(in crate::library) fn band(library: &Library) -> Band<'_> {
    if !library.discovery_enabled() {
        return Band::Off;
    }
    if library.home.discovery.latched().is_none() {
        return Band::Waiting;
    }
    let owned = library.owned();
    let band = library.home.discovery.band(&owned, &library.hidden);
    match library.discovery_inputs() {
        Inputs::Failed => Band::Absent,
        // A list reloading is the band's own write settling, so what has already
        // landed keeps drawing until the lists answer again: the rows must not
        // blink out under the click that asked for them.
        Inputs::Waiting => match band {
            Band::Rows(_) => band,
            _ => Band::Waiting,
        },
        Inputs::Ready => band,
    }
}

/// This frame's owned set, held so the asks and the draw share one build.
///
/// A cache is not part of the library's value: two libraries are equal whether or
/// not either has built one, and a clone starts empty rather than carrying
/// another frame's answer.
#[derive(Debug, Default)]
pub(crate) struct OwnedCache(std::cell::RefCell<Option<Arc<Owned>>>);

impl Clone for OwnedCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for OwnedCache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for OwnedCache {}

impl OwnedCache {
    fn take(&self) -> Option<Arc<Owned>> {
        self.0.borrow().as_ref().map(Arc::clone)
    }

    fn hold(&self, owned: Owned) {
        *self.0.borrow_mut() = Some(Arc::new(owned));
    }
}

impl Library {
    /// Builds this frame's set of artists the library already holds.
    ///
    /// Called once from the frame's ask, so a page that keeps drawing re-uses
    /// the one the asks already paid for.
    pub(crate) fn refresh_owned(&self) {
        self.owned.hold(owned_of(self));
    }

    /// This frame's set, built on first use when no ask ran.
    fn owned(&self) -> Arc<Owned> {
        match self.owned.take() {
            Some(owned) => owned,
            None => Arc::new(owned_of(self)),
        }
    }
}

#[cfg(test)]
thread_local! {
    /// How many times the owned set has been built, for the test that counts
    /// them.
    static BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_owned_builds() {
    BUILDS.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn owned_builds() -> usize {
    BUILDS.with(std::cell::Cell::get)
}

/// The artists the library already holds.
///
/// An artist followed since this page opened is left out, so its card keeps its
/// place and the tile that would undo it stays reachable. The next open rebuilds
/// the band without it.
fn owned_of(library: &Library) -> Owned {
    #[cfg(test)]
    BUILDS.with(|count| count.set(count.get() + 1));
    let mut owned = match (&library.followed_artists, &library.saved_albums) {
        (Load::Ready(artists), Load::Ready(albums)) => Owned::new(artists, albums),
        _ => Owned::default(),
    };
    for artist in library.home.discovery.just_followed() {
        owned.forget(artist);
    }
    owned
}

#[cfg(test)]
mod tests;
