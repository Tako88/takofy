//! The episodes shelf's read, and the section the page draws it from.
//!
//! The shelf asks one show at a time, in the followed list's own order (newest
//! follow first), so an open reads at most the newest [`SHOWS`] shows. It waits
//! whole: no row is drawn until every show this open asked has answered or
//! failed. A show that fails costs only its own episodes; only an open with
//! nothing drawable settles as a failure — because every asked show failed, or
//! because the answers that landed named no episode.

use takofy_model::home::HomeAsk;

use crate::home::episodes::{List, SHOWS};
use crate::home::{Budget, Phase, Section, Slot, settle};
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

    /// What the shelf's own reads are doing.
    ///
    /// It waits while a show this open asked for has not answered, and while
    /// there is still a followed show the open may ask for and the allowance to
    /// ask with. Once every show it may ask has answered or failed, what landed
    /// decides: an open with nothing drawable is a failure when a read failed or
    /// when the answers it did get named no episode, and an open with nothing
    /// followed is absent.
    fn episodes_phase(&self) -> Phase {
        let state = &self.home.episodes;
        let owed = state.asked().len() < self.episodes_target()
            && self.home.budget.left(Slot::Episodes) > 0;
        if state.pending() || owed {
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

    /// How many shows this open reads: the newest followed ones, within the
    /// shelf's allowance.
    fn episodes_target(&self) -> usize {
        match &self.followed {
            Load::Ready(shows) => shows.len().min(SHOWS).min(Budget::per_open(Slot::Episodes)),
            Load::Idle | Load::Loading | Load::Failed(_) => 0,
        }
    }

    /// The read the episodes shelf owes, if one is due.
    ///
    /// It asks one show at a time, so an answer names the show it was read for
    /// rather than the last one asked. A show already asked this open is never
    /// asked again, so the allowance is spent on distinct shows.
    pub(super) fn episodes_ask(&mut self) -> Option<Command> {
        let show = {
            let Load::Ready(shows) = &self.followed else {
                return None;
            };
            shows
                .iter()
                .take(SHOWS)
                .map(|show| show.uri.as_str())
                .find(|uri| !self.home.episodes.has_asked(uri))
                .map(str::to_owned)
        };
        let show = show?;
        if !self.home.budget.take(Slot::Episodes) {
            return None;
        }
        self.home.episodes.ask(&show);
        Some(Command::LoadHome(HomeAsk::Show { show }))
    }
}

#[cfg(test)]
mod tests;
