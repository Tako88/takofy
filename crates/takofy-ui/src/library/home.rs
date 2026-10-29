//! Home's view of the library: what each shelf's reads are doing, and the
//! shelves that follow from them.
//!
//! The dated shelf lives in [`dated`]; the four built shelves are one module
//! each. Both layers read state only: no page and no clock, so a constructed
//! `Library` can reach every state at once.

mod because;
mod counted;
mod dated;
mod episodes;
mod stations;

use crate::home::{Fit, Phase, Sections, Shelf, Slot};
use crate::message::Command;

use super::{Library, Load};

/// What one load is doing, as a source of a shelf.
pub(super) fn phase_of<T>(load: &Load<T>) -> Phase {
    match load {
        Load::Idle | Load::Loading => Phase::Pending,
        Load::Ready(_) => Phase::Arrived,
        Load::Failed(failure) => Phase::Failed(*failure),
    }
}

/// The phase of the list a shelf waits on before it can owe its own read.
///
/// A list nobody has asked for yet is not a read the shelf waits on: the page
/// asks for it before the shelf can owe anything, so until then the shelf has
/// nothing to draw and nothing to wait for. A list that is out, has answered or
/// has failed is the shelf's own prerequisite.
pub(super) fn prerequisite<T>(load: &Load<T>) -> Phase {
    match load {
        Load::Idle => Phase::Arrived,
        Load::Loading | Load::Ready(_) | Load::Failed(_) => phase_of(load),
    }
}

impl Library {
    /// Home's five shelves for one frame, borrowed from the library.
    pub fn home_sections(&self, fit: Fit) -> Sections<'_> {
        Sections {
            dated: self.home_shelf(fit.columns),
            stations: self.stations_section(),
            because: self.because_section(),
            counted: self.counted_section(),
            episodes: self.episodes_section(),
        }
    }

    /// The dated shelf, which is the page's first band.
    pub fn home_shelf(&self, cards: usize) -> Shelf<'_> {
        dated::home_shelf(self, cards)
    }

    /// The one read Home owes this frame, if any.
    ///
    /// One ask per frame, in the page's shelf order, so a session is asked for
    /// one thing at a time instead of five at once. Off Home nothing is owed:
    /// the shelves are not on screen to draw an answer.
    pub fn home_request(&mut self) -> Option<Command> {
        if !self.on_home() {
            return None;
        }
        self.counted_ask()
            .or_else(|| self.because_ask())
            .or_else(|| self.episodes_ask())
            .or_else(|| self.stations_ask())
    }

    /// Asks again for what the shelf in `slot` reads.
    ///
    /// A failed rootlist is the page's own Retry, because the reads behind it
    /// were never asked. Otherwise every failed read that shelf draws from is put
    /// back to idle so the next frame asks for it, the shelf's answers are
    /// dropped so nothing stale is drawn, and its allowance is returned.
    pub fn retry_home(&mut self, slot: Slot) -> Option<Command> {
        // A failed rootlist is the page's own Retry: every shelf's reads were
        // gated behind it, so the whole library is read again.
        if matches!(self.playlists, Load::Failed(_)) {
            return Some(self.reload());
        }
        self.reset_home_feeds(slot);
        self.home.forget_slot(slot);
        self.home.budget.refill(slot);
        None
    }

    /// Puts the failed reads `slot` draws from back to idle.
    ///
    /// Only the failed ones: a healthy read behind the same shelf keeps its
    /// answer, so one shelf's Retry never blanks another's.
    fn reset_home_feeds(&mut self, slot: Slot) {
        if matches!(slot, Slot::Dated | Slot::Stations | Slot::Counted) {
            if matches!(self.saved_albums, Load::Failed(_)) {
                self.saved_albums = Load::Idle;
            }
            // A healthy likes read is left alone: reloading it would re-read
            // every like and mark a good read stale.
            if matches!(self.likes.phase(), Phase::Failed(_)) {
                self.likes.reload();
            }
        }
        if matches!(slot, Slot::Stations | Slot::Because)
            && matches!(self.followed_artists, Load::Failed(_))
        {
            self.followed_artists = Load::Idle;
        }
        if slot == Slot::Episodes && matches!(self.followed, Load::Failed(_)) {
            self.followed = Load::Idle;
        }
    }
}
