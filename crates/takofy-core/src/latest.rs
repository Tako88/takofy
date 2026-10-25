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
mod tests {
    use std::time::Duration;

    use takofy_model::{Options, Status, TrackEntry};

    use super::*;

    const WAIT: Duration = Duration::from_millis(20);

    fn report(uri: &str) -> Report {
        Report {
            status: Status::Playing,
            track: Some(TrackEntry {
                uri: uri.to_owned(),
                title: uri.to_owned(),
                artists: String::new(),
                album: None,
                album_uri: None,
                artist_uris: Vec::new(),
                artist_names: Vec::new(),
                added_at: None,
                duration_ms: None,
                named: true,
                cover: None,
            }),
            prev: Vec::new(),
            next: Vec::new(),
            context: None,
            options: Options::default(),
            position_ms: 1_000,
            duration_ms: Some(200_000),
        }
    }

    #[test]
    fn a_report_is_kept_with_its_age() {
        let latest = Latest::default();
        assert!(latest.set(Some(report("a"))));
        std::thread::sleep(WAIT);

        let (kept, age) = latest.report().expect("the report is held");
        assert_eq!(kept, report("a"));
        assert!(
            age >= WAIT,
            "the age is the time since it was taken: {age:?}"
        );
        assert!(latest.has_report());
    }

    #[tokio::test]
    async fn setting_the_same_report_wakes_nobody() {
        let latest = Latest::default();
        assert!(latest.set(Some(report("a"))));
        let _ = latest.changed().await;

        std::thread::sleep(WAIT);
        let (_, before) = latest.report().expect("the report is held");
        assert!(
            !latest.set(Some(report("a"))),
            "the same report is no change"
        );
        let (_, after) = latest.report().expect("the report is held");
        assert!(after >= before, "a repeated report keeps its stamp");

        assert!(
            tokio::time::timeout(Duration::from_millis(20), latest.changed())
                .await
                .is_err(),
            "a repeated report wakes nobody"
        );

        assert!(latest.set(Some(report("b"))));
        assert!(
            tokio::time::timeout(Duration::from_millis(20), latest.changed())
                .await
                .is_ok(),
            "a different report wakes the publisher"
        );
    }

    #[tokio::test]
    async fn none_clears() {
        let latest = Latest::default();
        assert!(latest.set(Some(report("a"))));
        let _ = latest.changed().await;

        assert!(latest.set(None));
        assert!(latest.report().is_none());
        assert!(!latest.has_report());
        assert!(
            tokio::time::timeout(Duration::from_millis(20), latest.changed())
                .await
                .is_ok(),
            "clearing is a change"
        );
        assert!(!latest.set(None), "clearing what is empty is no change");
    }
}
