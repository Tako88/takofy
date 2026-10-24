//! What this device says about itself: whether it can play, and the stage that
//! follows from it.
//!
//! A flip wakes the publisher instead of announcing here, so every announcement
//! goes through one owner and one gate.

use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Notify;
use tokio::sync::futures::Notified;

/// What the account is told this device is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    /// It exists to read the list and steer.
    Controller,
    /// It can play, and is waiting to be picked.
    Ready,
}

#[derive(Default)]
pub(crate) struct Presence {
    playable: AtomicBool,
    changed: Notify,
}

impl Presence {
    /// Returns whether the flag moved.
    pub(crate) fn set_playable(&self, playable: bool) -> bool {
        if self.playable.swap(playable, Ordering::SeqCst) == playable {
            return false;
        }
        // A stored permit, so a flip that lands while the publisher is putting
        // is not lost; the cost is one more put of the latest stage.
        self.changed.notify_one();
        true
    }

    pub(crate) fn playable(&self) -> bool {
        self.playable.load(Ordering::SeqCst)
    }

    pub(crate) fn stage(&self) -> Stage {
        if self.playable() {
            Stage::Ready
        } else {
            Stage::Controller
        }
    }

    pub(crate) fn changed(&self) -> Notified<'_> {
        self.changed.notified()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// Long enough for a wake that must not come to have come.
    const WAKE_WAIT: Duration = Duration::from_millis(50);

    #[test]
    fn default_stage_is_the_controller() {
        let presence = Presence::default();

        assert_eq!(presence.stage(), Stage::Controller);
        assert!(!presence.playable());
    }

    #[test]
    fn playable_stage_is_ready() {
        let presence = Presence::default();

        assert!(presence.set_playable(true));
        assert_eq!(presence.stage(), Stage::Ready);
        assert!(presence.playable());
    }

    #[tokio::test]
    async fn setting_the_same_value_reports_no_change_and_wakes_nobody() {
        let presence = Presence::default();
        assert!(
            !presence.set_playable(false),
            "the default is already false"
        );

        assert!(presence.set_playable(true));
        // The flip left a permit; draining it makes the absence below mean
        // something.
        presence.changed().await;

        assert!(!presence.set_playable(true));
        assert!(
            tokio::time::timeout(WAKE_WAIT, presence.changed())
                .await
                .is_err(),
            "nothing moved, so nobody may be woken"
        );
    }

    #[tokio::test]
    async fn a_flip_wakes_the_publisher() {
        let presence = Presence::default();

        assert!(presence.set_playable(true));
        tokio::time::timeout(WAKE_WAIT, presence.changed())
            .await
            .expect("a flip wakes the publisher");
    }
}
