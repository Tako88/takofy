//! What this device says about itself: whether it can play, whether it holds
//! the sound, and the stage that follows from both.
//!
//! A flip wakes the publisher instead of announcing here, so every announcement
//! goes through one owner and one gate.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use tokio::sync::Notify;
use tokio::sync::futures::Notified;

use crate::connect::ClusterView;
use crate::loss::{Baseline, Judgement, judge};

/// What the account is told this device is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    /// It exists to read the list and steer.
    Controller,
    /// It can play, and is waiting to be picked.
    Ready,
    /// It holds the sound.
    Active,
}

/// What one pushed view means for the sound this window holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Observation {
    /// No activation to judge against.
    NotActive,
    /// The cluster names this window.
    Held,
    /// It names another device, but the push predates our activation.
    Stale,
    /// The sound is someone else's, and this activation is over.
    Lost { epoch: u64 },
}

/// One activation: what names it, and which pushes it may ignore.
#[derive(Debug, Clone, Copy)]
struct Active {
    epoch: u64,
    baseline: Baseline,
}

/// What letting go of the sound still owes the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Owed {
    /// Nothing was put, so the account never named this device.
    Nothing,
    /// An active put may have landed without an answer.
    Maybe,
    /// The activation that holds it, named so the caller can report the loss.
    Held { epoch: u64 },
}

impl Owed {
    /// Whether the account may still name this device as the holder.
    pub(crate) fn any(self) -> bool {
        !matches!(self, Self::Nothing)
    }
}

#[derive(Default)]
struct Slot {
    /// Counts activations, so a loss that crosses a landing names the right one.
    epoch: u64,
    active: Option<Active>,
    /// An active put that no answer or give-back has settled yet: a quit can
    /// drop the future mid-put, and the account may still have applied it.
    maybe_held: bool,
}

#[derive(Default)]
pub(crate) struct Presence {
    playable: AtomicBool,
    slot: Mutex<Slot>,
    changed: Notify,
}

impl Presence {
    /// A panic elsewhere must not make this unusable: the stage is a few words
    /// of state, and refusing to read it would leave the account unpickable.
    fn slot(&self) -> MutexGuard<'_, Slot> {
        self.slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

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

    /// Whether this window currently holds the account's sound.
    pub(crate) fn active(&self) -> bool {
        self.slot().active.is_some()
    }

    /// Records that an active put is going on the wire.
    ///
    /// The answer can be dropped with the future that awaited it, so the
    /// account may have taken the put with no activation recorded here; the
    /// hold is then still owed a give-back.
    pub(crate) fn put_issued(&self) {
        self.slot().maybe_held = true;
    }

    /// Records the account giving the sound back.
    ///
    /// A give-back the wire refused settles nothing: the account may still
    /// name this device, so the debt survives for the exit path.
    pub(crate) fn gave_back(&self, confirmed: bool) {
        if confirmed {
            self.slot().maybe_held = false;
        }
    }

    /// Starts an activation and returns the epoch that names it.
    ///
    /// Only a put the server took calls this: the hold is reported after the
    /// account has it, never before.
    pub(crate) fn activate(&self, baseline: Baseline) -> u64 {
        let mut slot = self.slot();
        slot.maybe_held = false;
        slot.epoch = slot.epoch.wrapping_add(1);
        slot.active = Some(Active {
            epoch: slot.epoch,
            baseline,
        });
        slot.epoch
    }

    /// Gives the sound back, saying what the account is still owed.
    ///
    /// A put nobody answered left no activation, so the epoch alone cannot say
    /// whether the account names this device.
    pub(crate) fn release(&self) -> Owed {
        let mut slot = self.slot();
        match slot.active.take() {
            // The activation carries the debt now, so the marker is redundant.
            Some(active) => {
                slot.maybe_held = false;
                Owed::Held {
                    epoch: active.epoch,
                }
            }
            // A give-back the wire refused settles nothing, so the marker stays
            // until one is confirmed.
            None if slot.maybe_held => Owed::Maybe,
            None => Owed::Nothing,
        }
    }

    /// Gives the sound back because the registration lapsed.
    ///
    /// The device may have dropped out of the cluster, so re-asserting could
    /// take the sound from whoever picked it up; the caller reports a loss.
    pub(crate) fn release_for_new_connection(&self) -> Option<u64> {
        match self.release() {
            Owed::Held { epoch } => Some(epoch),
            Owed::Nothing | Owed::Maybe => None,
        }
    }

    /// Judges one pushed view, ending the activation it says is over.
    ///
    /// Clearing under the same lock that judged is what makes a loss report
    /// once: every later push finds nothing left to lose.
    pub(crate) fn observe(&self, view: &ClusterView, now: Instant) -> Observation {
        let mut slot = self.slot();
        let Some(active) = slot.active else {
            return Observation::NotActive;
        };
        match judge(view, active.baseline, now) {
            Judgement::Held => Observation::Held,
            Judgement::Stale => Observation::Stale,
            Judgement::Lost => {
                slot.active = None;
                Observation::Lost {
                    epoch: active.epoch,
                }
            }
        }
    }

    /// What the account should be told this device is.
    ///
    /// A stage that cannot hold the sound gives it back: neither an unplayable
    /// player nor a missing report is a loss, but neither can announce active.
    pub(crate) fn stage(&self, has_report: bool) -> Stage {
        let mut slot = self.slot();
        if slot.active.is_some() && !(self.playable() && has_report) {
            slot.active = None;
        }
        if slot.active.is_some() {
            Stage::Active
        } else if self.playable() {
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

    use librespot::protocol::connect::DeviceInfo;
    use librespot::protocol::player::PlayerState;
    use takofy_model::{Options, Report, Status, TrackEntry};

    use super::*;
    use crate::connect::Announcement;
    use crate::playstate::ActiveState;

    /// Long enough for a wake that must not come to have come.
    const WAKE_WAIT: Duration = Duration::from_millis(50);

    /// The instant an activation reply stamped, and a push after it.
    const BASELINE_MS: i64 = 5_000;

    fn view(held_here: bool, changed_ms: i64) -> ClusterView {
        ClusterView {
            devices: None,
            holder: (!held_here).then(|| "phone".to_owned()),
            held_here,
            changed_ms,
            playback: None,
        }
    }

    fn held() -> ClusterView {
        view(true, 9_000)
    }

    fn elsewhere(changed_ms: i64) -> ClusterView {
        view(false, changed_ms)
    }

    fn entry(uri: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: uri.to_owned(),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            artist_names: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: false,
            cover: None,
        }
    }

    fn report() -> Report {
        Report {
            status: Status::Playing,
            track: Some(entry("spotify:track:current")),
            prev: Vec::new(),
            next: Vec::new(),
            context: Some("spotify:album:context".to_owned()),
            options: Options::default(),
            position_ms: 10_000,
            duration_ms: Some(200_000),
        }
    }

    fn active_state() -> ActiveState {
        ActiveState::new(report(), 0).expect("the fixture names a track")
    }

    fn announce(stage: Stage) -> Announcement {
        match stage {
            Stage::Controller => Announcement::controller("device-id", "client-id"),
            Stage::Ready => Announcement::ready("device-id", "client-id", "session-id"),
            Stage::Active => Announcement::active(
                "device-id",
                "client-id",
                "session-id",
                &active_state(),
                None,
                1_700_000_000_000,
            ),
        }
    }

    fn info(announcement: &Announcement) -> &DeviceInfo {
        announcement
            .message()
            .device
            .as_ref()
            .and_then(|device| device.device_info.as_ref())
            .expect("an announcement names its info")
    }

    fn player(announcement: &Announcement) -> &PlayerState {
        announcement
            .message()
            .device
            .as_ref()
            .and_then(|device| device.player_state.as_ref())
            .expect("a receiver names its player state")
    }

    #[test]
    fn default_stage_is_the_controller() {
        let presence = Presence::default();

        assert_eq!(presence.stage(false), Stage::Controller);
        assert!(!presence.playable());
        assert!(!presence.active());
    }

    #[test]
    fn playable_stage_is_ready() {
        let presence = Presence::default();

        assert!(presence.set_playable(true));
        assert_eq!(presence.stage(false), Stage::Ready);
        assert!(presence.playable());
    }

    #[test]
    fn not_active_observes_nothing() {
        let presence = Presence::default();
        presence.set_playable(true);

        for view in [held(), elsewhere(BASELINE_MS + 1)] {
            assert_eq!(
                presence.observe(&view, Instant::now()),
                Observation::NotActive
            );
        }
    }

    #[test]
    fn a_push_naming_us_is_held() {
        let presence = Presence::default();
        presence.activate(Baseline::After(BASELINE_MS));

        assert_eq!(presence.observe(&held(), Instant::now()), Observation::Held);
        assert!(presence.active(), "a held push leaves the sound with us");
    }

    #[test]
    fn a_fresh_push_naming_another_device_is_lost_once() {
        let presence = Presence::default();
        presence.activate(Baseline::After(BASELINE_MS));

        assert_eq!(
            presence.observe(&elsewhere(BASELINE_MS + 1), Instant::now()),
            Observation::Lost { epoch: 1 }
        );
        assert!(!presence.active());
        assert_eq!(
            presence.observe(&elsewhere(BASELINE_MS + 2), Instant::now()),
            Observation::NotActive,
            "a loss is reported once"
        );
    }

    #[test]
    fn a_stale_push_naming_another_device_is_ignored() {
        let presence = Presence::default();
        presence.activate(Baseline::After(BASELINE_MS));

        assert_eq!(
            presence.observe(&elsewhere(BASELINE_MS - 1), Instant::now()),
            Observation::Stale
        );
        assert!(presence.active());
    }

    #[test]
    fn activate_increments_the_epoch_and_a_loss_names_it() {
        let presence = Presence::default();
        let now = Instant::now();

        assert_eq!(
            presence.activate(Baseline::Until(now + Duration::from_millis(50))),
            1
        );
        assert_eq!(
            presence.observe(&elsewhere(BASELINE_MS), now),
            Observation::Stale
        );
        assert_eq!(presence.activate(Baseline::After(BASELINE_MS)), 2);
        assert_eq!(
            presence.observe(&elsewhere(0), now),
            Observation::Lost { epoch: 2 }
        );
        assert_eq!(
            presence.activate(Baseline::After(BASELINE_MS)),
            3,
            "the epoch counts activations, not live holds"
        );
    }

    #[test]
    fn a_new_connection_id_while_active_is_a_loss_and_clears_active() {
        let presence = Presence::default();
        assert_eq!(presence.activate(Baseline::After(BASELINE_MS)), 1);

        assert_eq!(presence.release_for_new_connection(), Some(1));
        assert!(!presence.active());
        assert_eq!(presence.release_for_new_connection(), None);
        assert_eq!(
            presence.observe(&held(), Instant::now()),
            Observation::NotActive
        );
    }

    #[test]
    fn release_names_the_activation_it_ended() {
        let presence = Presence::default();
        assert_eq!(presence.release(), Owed::Nothing);
        presence.activate(Baseline::After(BASELINE_MS));

        assert_eq!(presence.release(), Owed::Held { epoch: 1 });
        assert!(!presence.active());
    }

    #[test]
    fn a_put_nobody_answered_is_still_owed_a_give_back() {
        let presence = Presence::default();
        presence.put_issued();

        assert!(!presence.active(), "no answer means no activation");
        assert_eq!(
            presence.release(),
            Owed::Maybe,
            "the account may name this device all the same"
        );
        presence.gave_back(true);
        assert_eq!(
            presence.release(),
            Owed::Nothing,
            "a confirmed put settles it"
        );
    }

    #[test]
    fn a_refused_give_back_leaves_the_debt_standing() {
        let presence = Presence::default();
        presence.put_issued();
        presence.gave_back(false);

        assert_eq!(presence.release(), Owed::Maybe);

        presence.put_issued();
        presence.gave_back(true);
        assert_eq!(presence.release(), Owed::Nothing);
    }

    #[test]
    fn an_activation_settles_the_put_that_earned_it() {
        let presence = Presence::default();
        presence.put_issued();
        assert_eq!(presence.activate(Baseline::After(BASELINE_MS)), 1);

        assert_eq!(presence.release(), Owed::Held { epoch: 1 });
    }

    #[test]
    fn stage_is_active_only_with_a_report() {
        let presence = Presence::default();
        presence.set_playable(true);

        assert_eq!(presence.stage(false), Stage::Ready, "nothing to hold");
        presence.activate(Baseline::After(BASELINE_MS));
        assert_eq!(presence.stage(true), Stage::Active);
        assert!(presence.active());
    }

    #[test]
    fn active_without_a_report_clears_and_falls_back() {
        let presence = Presence::default();
        presence.set_playable(true);
        presence.activate(Baseline::After(BASELINE_MS));

        assert_eq!(presence.stage(false), Stage::Ready);
        assert!(!presence.active(), "an empty report cannot hold the sound");
    }

    #[test]
    fn playable_false_while_active_falls_back_and_clears() {
        let presence = Presence::default();
        presence.set_playable(true);
        presence.activate(Baseline::After(BASELINE_MS));
        presence.set_playable(false);

        assert_eq!(presence.stage(true), Stage::Controller);
        assert!(!presence.active());
    }

    #[test]
    fn stage_properties() {
        let controller = announce(Stage::Controller);
        assert!(!info(&controller).can_play);
        assert!(!controller.message().is_active);

        let ready = announce(Stage::Ready);
        assert!(info(&ready).can_play);
        assert!(!ready.message().is_active);
        assert!(player(&ready).track.is_none(), "a ready body holds nothing");

        let presence = Presence::default();
        presence.set_playable(true);
        presence.activate(Baseline::After(BASELINE_MS));
        let active = announce(presence.stage(true));
        assert!(active.message().is_active);
        let track = player(&active)
            .track
            .as_ref()
            .expect("an active body names the track it holds");
        assert_eq!(track.uri, "spotify:track:current");
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
