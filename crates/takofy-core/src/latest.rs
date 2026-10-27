//! The newest report this window published, with the instant it was taken.
//!
//! The age is what makes a playing position travel correctly: the wire is told
//! where the report started, not where an interpolation happens to be. A
//! repeated report keeps its original stamp, so a frame that changed nothing
//! cannot make an old position look fresh.

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use takofy_model::Report;
use tokio::sync::Notify;
use tokio::sync::futures::Notified;

struct Stamped {
    report: Report,
    at: Instant,
}

#[derive(Default)]
pub(crate) struct Latest {
    slot: Mutex<Option<Stamped>>,
    changed: Notify,
}

impl Latest {
    /// Replaces the report, answering whether it differs from the one held.
    pub(crate) fn set(&self, report: Option<Report>) -> bool {
        let mut slot = self.slot();
        let unchanged = match (&*slot, &report) {
            (Some(held), Some(report)) => held.report == *report,
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return false;
        }
        *slot = report.map(|report| Stamped {
            report,
            at: Instant::now(),
        });
        drop(slot);
        self.changed.notify_one();
        true
    }

    /// The report and how long ago it was taken.
    pub(crate) fn report(&self) -> Option<(Report, Duration)> {
        self.slot()
            .as_ref()
            .map(|held| (held.report.clone(), held.at.elapsed()))
    }

    pub(crate) fn has_report(&self) -> bool {
        self.slot().is_some()
    }

    pub(crate) fn changed(&self) -> Notified<'_> {
        self.changed.notified()
    }

    /// A panic elsewhere must not make this unusable: the slot is one optional
    /// report, and refusing to read it would freeze every announce.
    fn slot(&self) -> MutexGuard<'_, Option<Stamped>> {
        self.slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests;
