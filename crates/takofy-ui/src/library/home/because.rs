//! The because shelf's read, and the state the page draws it from.
//!
//! One followed artist is asked at a time, in a drawn order, and an answer with
//! no related artists lets the next one be tried. The open stops at the shelf's
//! allowance, and an open that spends all of it on empty answers has nothing to
//! draw: that is a fact about the artists, not a failure. A read that failed
//! costs only its own artist, and only an open that drew nothing while some read
//! failed settles as a failure.

use takofy_model::home::HomeAsk;

use crate::home::because::Row;
use crate::home::{Budget, Phase, Section, Slot, settle};
use crate::library::{Library, Load};
use crate::message::Command;

use super::phase_of;

impl Library {
    /// What the because shelf draws.
    pub(super) fn because_section(&self) -> Section<Row<'_>> {
        let prerequisite = phase_of(&self.followed_artists);
        let own = self.because_phase();
        settle(prerequisite, &[prerequisite, own], self.home.because.row())
    }

    /// What the shelf's own read is doing.
    ///
    /// It waits while an answer is outstanding, and while there is still a
    /// followed artist the open may try. Once the open has spent its allowance,
    /// a row it drew is an answer, a read that failed with nothing drawn is a
    /// failure, and an open whose reads all came back empty is a quiet library.
    fn because_phase(&self) -> Phase {
        let state = &self.home.because;
        if state.has_cards() {
            return Phase::Arrived;
        }
        if state.asked().is_some() {
            return Phase::Pending;
        }
        let followed = match &self.followed_artists {
            Load::Ready(artists) => artists.len(),
            Load::Idle | Load::Loading | Load::Failed(_) => 0,
        };
        let allowed = followed.min(Budget::per_open(Slot::Because));
        if state.tried() < allowed {
            return Phase::Pending;
        }
        match state.failure() {
            Some(failure) => Phase::Failed(failure),
            None => Phase::Arrived,
        }
    }

    /// The read the because shelf owes, if one is due.
    ///
    /// It asks one artist at a time, so an artist's answer names the row's own
    /// seed rather than whichever artist was asked last. A read that came back
    /// empty or failed leaves the next ask reading the next artist, so the
    /// allowance is spent on the section rather than on its first artist.
    pub(super) fn because_ask(&mut self) -> Option<Command> {
        if self.home.because.has_cards() || self.home.because.asked().is_some() {
            return None;
        }
        let Load::Ready(artists) = &self.followed_artists else {
            return None;
        };
        let at = self.home.because.next_artist(artists.len())?;
        let artist = artists.get(at)?;
        if !self.home.budget.take(Slot::Because) {
            return None;
        }
        self.home.because.ask(artist);
        Some(Command::LoadHome(HomeAsk::Related {
            artist: artist.uri.clone(),
        }))
    }
}

#[cfg(test)]
mod tests;
