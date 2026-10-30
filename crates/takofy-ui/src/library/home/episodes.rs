//! The episodes shelf's read, and the section the page draws it from.
//!
//! The shelf asks for the newest [`SHOWS`] shows in the followed list's own order
//! (newest follow first), all in one read, so an open costs one ask however many
//! shows it draws from. No row is drawn until that read answers. A show the read
//! could not get costs only its own episodes; only an open with nothing drawable
//! settles as a failure — because every asked show failed, or because the
//! episodes that landed named none of themselves.

use takofy_model::home::HomeAsk;

use crate::home::episodes::{List, SHOWS};
use crate::home::{Phase, Section, Slot, settle};
use crate::message::{Command, Failure};

use super::super::{Library, Load};
use super::prerequisite;

impl Library {
    /// What the episodes shelf draws.
    pub(super) fn episodes_section(&self) -> Section<List<'_>> {
        settle(
            prerequisite(&self.followed),
            &[self.episodes_phase()],
            self.home.episodes.list(),
        )
    }

    /// What the shelf's own read is doing.
    ///
    /// It waits while the one read is still owed or a show it named has not
    /// answered. Once it has, what landed decides: an open with nothing
    /// drawable is a failure when a read failed or when the episodes it did get
    /// named none of themselves, and an open with nothing followed is absent.
    fn episodes_phase(&self) -> Phase {
        let state = &self.home.episodes;
        if state.pending() || self.episodes_owed() {
            return Phase::Pending;
        }
        // The payload decides, not how many shows answered: one new show that
        // landed with no episodes beside a failed read drew nothing, and must
        // say why instead of reading as a library that follows nothing.
        if self.home.episodes.list().is_none()
            && let Some(failure) = state.failure()
        {
            return Phase::Failed(failure);
        }
        // An answer whose episodes all fell back to their URIs names nothing
        // the pool can date, so it is a read that failed quietly rather than an
        // empty following list.
        if !state.landed().is_empty() && !state.readable() {
            return Phase::Failed(Failure::Load);
        }
        Phase::Arrived
    }

    /// Whether this open still owes its one read.
    ///
    /// An open that already asked owes nothing more, and an open with nothing
    /// followed has nothing to ask for: it settles absent rather than waiting on
    /// a read it will never send.
    fn episodes_owed(&self) -> bool {
        if !self.home.episodes.asked().is_empty() {
            return false;
        }
        let Load::Ready(shows) = &self.followed else {
            return false;
        };
        !shows.is_empty() && self.home.budget.left(Slot::Episodes) > 0
    }

    /// The one read the episodes shelf owes, if it is due.
    ///
    /// It names every show the shelf draws from, so the pool costs one ask and
    /// one label read. The allowance is checked before the ask is recorded, so a
    /// spent open waits for no read it cannot send.
    pub(super) fn episodes_ask(&mut self) -> Option<Command> {
        if !self.episodes_owed() {
            return None;
        }
        let shows: Vec<String> = {
            let Load::Ready(shows) = &self.followed else {
                return None;
            };
            shows
                .iter()
                .take(SHOWS)
                .map(|show| show.uri.clone())
                .collect()
        };
        if !self.home.budget.take(Slot::Episodes) {
            return None;
        }
        self.home.episodes.ask(&shows);
        Some(Command::LoadHome(HomeAsk::Episodes { shows }))
    }
}

#[cfg(test)]
mod tests;
