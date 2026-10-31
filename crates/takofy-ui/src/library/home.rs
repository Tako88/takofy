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

use std::time::{Duration, Instant};

use crate::home::{Fit, Phase, Sections, Shelf, Slot};
use crate::message::Command;

use super::{Library, Load};

/// How long the built shelves yield to the dated shelf's feeds.
///
/// The wait is a read-scheduling bound rather than a token: a feed that hangs
/// costs the page this much and no more.
pub const HOME_HOLD: Duration = Duration::from_secs(1);

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

    /// Everything Home asks for this frame, in the order the page wants it.
    ///
    /// The dated shelf's own label read goes out first. The built shelves' asks
    /// follow it, and wait while the dated feeds are still outstanding for at
    /// most [`HOME_HOLD`]: a feed that hangs costs the page that bound and no
    /// more. Off Home nothing is asked, because nothing is on screen to draw an
    /// answer.
    pub fn home_asks(&mut self, now: Instant) -> Vec<Command> {
        if !self.on_home() {
            self.held_since = None;
            self.hold_spent = false;
            return Vec::new();
        }
        // Home draws the copy, so its own reads start from live data: while a
        // list a shelf builds from is still last session's, every ask would
        // read against rows this session has not been told.
        if self.restored_waiting() {
            self.held_since = None;
            self.hold_spent = false;
            return Vec::new();
        }
        let mut asks: Vec<Command> = self.home_labels_request().into_iter().collect();
        if self.home_intake().settled() || self.hold_spent {
            asks.extend(self.home_requests());
            self.held_since = None;
            self.hold_spent = true;
            return asks;
        }
        if !self.dated_label_due() {
            asks.extend(self.home_requests());
            return asks;
        }
        match self.held_since {
            None => self.held_since = Some(now),
            Some(since) if now.saturating_duration_since(since) >= HOME_HOLD => {
                self.held_since = None;
                self.hold_spent = true;
                asks.extend(self.home_requests());
            }
            Some(_) => {}
        }
        asks
    }

    /// Whether the wait for the dated feeds is what is holding the built asks.
    pub fn home_held(&self) -> bool {
        self.held_since.is_some()
            && !self.hold_spent
            && !self.home_intake().settled()
            && self.dated_label_due()
    }

    /// Whether the dated shelf may still owe a label read worth yielding to.
    ///
    /// A failed rootlist has no feeds left to wait for, and a likes read that
    /// answered with nothing will never ask for a label.
    fn dated_label_due(&self) -> bool {
        !matches!(phase_of(&self.playlists), Phase::Failed(_))
            && (self.likes.any() || matches!(self.likes.phase(), Phase::Pending))
    }

    /// The built shelves' one ask for this frame, if any.
    ///
    /// The order is the page's own, so the first shelf the reader sees is the
    /// first read started.
    pub(crate) fn home_request(&mut self) -> Option<Command> {
        if !self.on_home() {
            return None;
        }
        self.stations_ask()
            .or_else(|| self.because_ask())
            .or_else(|| self.counted_ask())
            .or_else(|| self.episodes_ask())
            // Faces and lengths are what the shelves decorate with, so they are
            // asked for last: no shelf waits on one to say what it holds.
            .or_else(|| self.stations_lengths_ask())
            .or_else(|| self.counted_portrait_ask())
    }

    /// Every built ask this open still owes.
    ///
    /// They all go out in the frame that owes them: the worker bounds the reads
    /// in flight, so holding some back for a later frame would only leave its
    /// places idle. Each ask spends its shelf's allowance, so the loop ends even
    /// when a shelf's answer never arrives.
    fn home_requests(&mut self) -> Vec<Command> {
        let mut asks = Vec::new();
        while let Some(ask) = self.home_request() {
            asks.push(ask);
        }
        asks
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
