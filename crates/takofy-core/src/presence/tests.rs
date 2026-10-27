use std::time::Duration;

use librespot::protocol::connect::DeviceInfo;
use librespot::protocol::player::PlayerState;
use takofy_model::{Report, Status, TrackEntry};

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
        queue_revision: String::new(),
        options: None,
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
fn a_pick_in_flight_ends_with_the_hold_it_left() {
    let presence = Presence::default();
    presence.activate(Baseline::After(BASELINE_MS));
    presence.pick_issued();
    assert!(presence.picking());

    assert_eq!(presence.release(), Owed::Held { epoch: 1 });
    assert!(
        !presence.picking(),
        "a hold that ended is not still leaving"
    );
}

#[test]
fn a_pick_in_flight_ends_with_the_view_naming_another_device() {
    let presence = Presence::default();
    presence.activate(Baseline::After(BASELINE_MS));
    presence.pick_issued();

    assert_eq!(
        presence.observe(&elsewhere(BASELINE_MS + 1), Instant::now()),
        Observation::Lost { epoch: 1 }
    );
    assert!(!presence.picking(), "the target taking the sound ends it");
}

#[test]
fn a_new_connection_id_ends_a_pick_in_flight() {
    let presence = Presence::default();
    presence.activate(Baseline::After(BASELINE_MS));
    presence.pick_issued();

    assert_eq!(presence.release_for_new_connection(), Some(1));
    assert!(!presence.picking());
}

#[test]
fn a_failed_pick_leaves_the_hold_it_did_not_move() {
    let presence = Presence::default();
    presence.activate(Baseline::After(BASELINE_MS));
    presence.pick_issued();
    presence.pick_settled();

    assert!(!presence.picking());
    assert!(presence.active(), "the sound never left this device");
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
