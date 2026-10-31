//! A radio's drift: the ask that keeps it going, and the guards around it.
//!
//! The threshold and the guards live beside the rows they count, so the window
//! sends an ask and learns nothing else about when one is owed.

use std::collections::HashSet;

use takofy_model::TrackEntry;
use takofy_model::uri::{UriKind, parse};

use crate::message::Command;
use crate::player::Transport;

use super::Queue;

/// Where a radio's drift stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Drift {
    /// The queue is a list, so it never asks.
    #[default]
    Off,
    /// The queue is a radio with room to ask.
    Ready,
    /// An ask is out, and this is the token it carried.
    Asked(u64),
    /// An answer held nothing fresh, so this radio has nothing more to give.
    Dry,
}

/// How many rows ahead of the current one a radio runs down to before it asks.
const DRIFT_LOW: usize = 5;

/// How many played rows an ask offers the station to continue from.
const DRIFT_RECENT: usize = 50;

impl Queue {
    /// Replaces the queue with a radio's rows, starting on the first of them.
    ///
    /// A radio that starts nothing is not marked, so an ask never follows a
    /// queue with no row to continue from.
    pub fn start_radio(
        &mut self,
        tracks: &[TrackEntry],
        transport: &mut Transport,
    ) -> Option<Command> {
        let command = self.start(tracks, 0, None, transport)?;
        self.drift = Drift::Ready;
        Some(command)
    }

    /// The ask a radio owes, when its rows have run short and none is out.
    ///
    /// Only a track is continued from: a row that names a show or a local file
    /// has no station behind it, so the ask waits for a row that has one.
    pub fn drift_wants(&mut self) -> Option<Command> {
        if self.drift != Drift::Ready || self.remaining() >= DRIFT_LOW {
            return None;
        }
        let context = self.current()?.uri.clone();
        if !track_uri(&context) {
            return None;
        }
        self.drift_asks += 1;
        self.drift = Drift::Asked(self.drift_asks);
        let mut recent: Vec<String> = self
            .history(DRIFT_RECENT)
            .filter(|entry| track_uri(&entry.uri))
            .map(|entry| entry.uri.clone())
            .collect();
        // The nearest row is the one the station should continue from, and the
        // history reads oldest first.
        recent.reverse();
        Some(Command::ContinueRadio {
            ask: self.drift_asks,
            context,
            recent,
        })
    }

    /// Folds a radio's answer in, appending the rows the queue does not hold.
    ///
    /// The only rows it can tell apart are the ones still here, so a row the
    /// user removed from the upcoming list can come back.
    pub fn land_drift(&mut self, ask: u64, rows: &[TrackEntry]) {
        if self.drift != Drift::Asked(ask) {
            return;
        }
        let mut known: HashSet<String> =
            self.tracks.iter().map(|entry| entry.uri.clone()).collect();
        let fresh: Vec<TrackEntry> = rows
            .iter()
            .filter(|entry| known.insert(entry.uri.clone()))
            .cloned()
            .collect();
        if fresh.is_empty() {
            self.drift = Drift::Dry;
            return;
        }
        self.append(fresh);
        self.drift = Drift::Ready;
    }

    /// Says these rows are no longer a radio's.
    pub(super) fn leave_drift(&mut self) {
        self.drift = Drift::Off;
    }

    /// Puts an ask the link may have dropped back within reach of the next tick.
    pub(super) fn rearm_drift(&mut self) {
        if matches!(self.drift, Drift::Asked(_)) {
            self.drift = Drift::Ready;
        }
    }

    /// How many rows are ahead of the current one.
    fn remaining(&self) -> usize {
        self.tracks.len().saturating_sub(self.first_upcoming())
    }
}

/// Whether `uri` names a track, which is all a radio continues from.
fn track_uri(uri: &str) -> bool {
    parse(uri).is_some_and(|parsed| parsed.kind == UriKind::Track)
}
