//! The episodes shelf's read, and the section the page draws it from.
//!
//! The shelf asks one show at a time, in the followed list's own order (newest
//! follow first), so an open reads at most the newest [`SHOWS`] shows. It waits
//! whole: no row is drawn until every show this open asked has answered or
//! failed. A show that fails costs only its own episodes; only an open whose
//! every asked show failed settles as a failure.

use takofy_model::home::HomeAsk;

use crate::home::episodes::{List, SHOWS};
use crate::home::{Budget, Phase, Section, Slot, settle};
use crate::message::Command;

use super::super::{Library, Load};
use super::phase_of;

impl Library {
    /// What the episodes shelf draws.
    ///
    /// A followed list nobody has asked for yet is not a read this shelf waits
    /// on: the page asks for it before the shelf can owe a show, so until then
    /// the shelf has nothing to draw and nothing to wait for.
    pub(super) fn episodes_section(&self) -> Section<List<'_>> {
        let prerequisite = match self.followed {
            Load::Idle => Phase::Arrived,
            Load::Loading | Load::Ready(_) | Load::Failed(_) => phase_of(&self.followed),
        };
        settle(
            prerequisite,
            &[self.episodes_phase()],
            self.home.episodes.list(),
        )
    }

    /// What the shelf's own reads are doing.
    ///
    /// It waits while a show this open asked for has not answered, and while
    /// there is still a followed show the open may ask for and the allowance to
    /// ask with. Once every show it may ask has answered or failed, a show that
    /// landed is the section, an open that failed everywhere is a failure, and an
    /// open with nothing followed is absent.
    fn episodes_phase(&self) -> Phase {
        let state = &self.home.episodes;
        let owed = state.asked().len() < self.episodes_target()
            && self.home.budget.left(Slot::Episodes) > 0;
        if state.pending() || owed {
            return Phase::Pending;
        }
        if state.landed().is_empty()
            && let Some(failure) = state.failure()
        {
            return Phase::Failed(failure);
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
