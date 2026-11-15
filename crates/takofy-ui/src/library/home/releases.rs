//! The release shelf's read, and the section the page draws it from.
//!
//! The shelf asks once an open for every followed artist it does not hold, at
//! most [`SHELF_ARTISTS`] of them in the list's own order (newest follow first),
//! and it asks from the window's floor. No card is drawn until that answer
//! lands.
//!
//! Nothing here reads the discovery setting: the shelf is the library's own, not
//! something looked up outside, so turning discovery off changes nothing.

use takofy_model::home::{Day, HomeAsk};

use crate::home::Section;
use crate::home::releases::{DRAWN, List, WINDOW_DAYS};
use crate::message::Command;

use super::super::{Library, Load};

impl Library {
    /// What the release shelf draws.
    ///
    /// The window is built from the followed list, so an unfollow takes effect
    /// as soon as the list is ready again. While the list is unready through the
    /// reload a follow causes, the shelf keeps drawing what it holds, unfiltered:
    /// it neither blinks to a skeleton nor loses its rule mid-reload.
    pub(super) fn releases_section(&self) -> Section<List<'_>> {
        if self.home.releases.day().is_none() || self.home.releases.failure().is_some() {
            return Section::Absent;
        }
        match &self.followed_artists {
            Load::Ready(followed) => {
                let list = self.home.releases.pick(followed, self.utc_offset, DRAWN);
                if !list.cards.is_empty() {
                    return Section::Ready(list);
                }
                if self.home.releases.owed(followed).is_empty() && !self.home.releases.pending() {
                    Section::Absent
                } else {
                    Section::Waiting
                }
            }
            Load::Idle | Load::Loading => {
                let unfiltered = self.home.releases.held_list(DRAWN);
                if !unfiltered.cards.is_empty() {
                    return Section::Ready(unfiltered);
                }
                if matches!(self.followed_artists, Load::Loading) {
                    Section::Waiting
                } else {
                    Section::Absent
                }
            }
            Load::Failed(_) => Section::Absent,
        }
    }

    /// The one read the shelf owes this open, if it is due.
    ///
    /// It asks from the floor of the window; each artist's own follow date
    /// narrows it again when the cards are picked.
    pub(super) fn releases_ask(&mut self) -> Option<Command> {
        let day = self.home.releases.day()?;
        if self.home.releases.failure().is_some() {
            return None;
        }
        let owed = {
            let Load::Ready(followed) = &self.followed_artists else {
                return None;
            };
            self.home.releases.owed(followed)
        };
        if owed.is_empty() {
            return None;
        }
        self.home.releases.ask(&owed);
        Some(Command::LoadHome(HomeAsk::Releases {
            artists: owed,
            since: Day::from_days(day.days() - WINDOW_DAYS),
        }))
    }
}

#[cfg(test)]
mod tests;
