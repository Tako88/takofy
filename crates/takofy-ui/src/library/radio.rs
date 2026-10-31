//! A radio a menu started, and the rows it waits on.

use takofy_model::TrackEntry;
use takofy_model::home::StationSeed;

use crate::message::Command;

use super::{Library, SidebarWrite};

impl Library {
    /// Sends a radio read for `seed` and holds it as the answer to wait for.
    ///
    /// A repeat press sends again rather than being refused: a read the worker
    /// dropped would otherwise lock the seed out for the rest of the session,
    /// and a wasted read is the cheaper mistake. The first answer takes the
    /// seed, so a second one is dropped as stale.
    ///
    /// The seed lives beside the shelf play rather than in it, so a page change
    /// between the press and the answer does not drop the radio.
    pub fn start_radio(&mut self, seed: StationSeed) -> Command {
        self.radio = Some(seed.uri.clone());
        if self.sidebar_notice == Some(SidebarWrite::Radio) {
            self.sidebar_notice = None;
        }
        Command::StartRadio { seed }
    }

    /// Starts the queue on the rows `seed`'s read answered, if they are the
    /// ones this window is still waiting for.
    pub(super) fn land_radio(&mut self, seed: &str, rows: Vec<TrackEntry>) {
        if self.radio.as_deref() != Some(seed) {
            return;
        }
        self.radio = None;
        if self.sidebar_notice == Some(SidebarWrite::Radio) {
            self.sidebar_notice = None;
        }
        // A radio plays without an origin: its rows are a flat list this
        // client cannot page, so there is no context another device could
        // resume. Empty rows are the worker's own failure to send, and land
        // here as a sentence rather than as a queue with nothing in it.
        if !self.want_radio_rows(rows) {
            self.sidebar_notice = Some(SidebarWrite::Radio);
        }
    }

    /// Drops the wait for `seed`'s rows and says the radio did not start.
    pub(super) fn fail_radio(&mut self, seed: &str) {
        if self.radio.as_deref() != Some(seed) {
            return;
        }
        self.radio = None;
        self.sidebar_notice = Some(SidebarWrite::Radio);
    }
}
