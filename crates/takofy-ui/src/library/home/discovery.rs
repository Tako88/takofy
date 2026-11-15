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

use crate::home::discovery::fresh::{self, Owned};
use crate::home::discovery::hidden::Hidden;
use crate::home::discovery::{Band, RowView};
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
        let owned = owned_of(self);
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
    match library.discovery_inputs() {
        Inputs::Failed => Band::Absent,
        Inputs::Waiting => Band::Waiting,
        Inputs::Ready => {
            let owned = owned_of(library);
            match library.home.discovery.band() {
                Band::Rows(rows) => Band::Rows(
                    rows.into_iter()
                        .map(|row| kept(row, &owned, &library.hidden))
                        .collect(),
                ),
                other => other,
            }
        }
    }
}

/// One row's cards, without the artists the library holds, the row's own seed,
/// or anyone the user hid.
fn kept<'a>(row: RowView<'a>, owned: &Owned, hidden: &Hidden) -> RowView<'a> {
    let anchor = row.seed.anchor.as_ref();
    let cards = row
        .cards
        .into_iter()
        .filter(|card| fresh::keeps(card, anchor, owned, hidden))
        .collect();
    RowView { cards, ..row }
}

/// The artists the library already holds.
fn owned_of(library: &Library) -> Owned {
    match (&library.followed_artists, &library.saved_albums) {
        (Load::Ready(artists), Load::Ready(albums)) => Owned::new(artists, albums),
        _ => Owned::default(),
    }
}

#[cfg(test)]
mod tests;
