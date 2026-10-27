use super::super::*;
use super::test_support::*;

fn failed(failure: Failure) -> Update {
    Update::Devices(DeviceUpdate::Failed(failure))
}

#[test]
fn the_watch_is_asked_for_once_and_only_once_the_library_is_ready() {
    let mut devices = Devices::default();
    assert_eq!(devices.watch_request(false), None);
    assert_eq!(devices.button(), Button::Off, "a watch not asked is off");
    assert_eq!(devices.watch_request(true), Some(Command::WatchDevices));
    assert_eq!(devices.watch_request(true), None, "asked twice");
}

#[test]
fn a_live_watch_is_never_asked_for_again() {
    let now = base();
    let mut devices = remote(now);
    assert_eq!(devices.watch_request(true), None);
}

#[test]
fn a_failed_update_while_asked_fails_the_watch_and_leaves_the_button_ready() {
    let now = base();
    let mut devices = Devices::default();
    devices.watch_request(true);
    assert!(devices.apply(&failed(Failure::Devices), now));
    assert_eq!(devices.watch, Watch::Failed(Failure::Devices));
    assert_eq!(
        devices.button(),
        Button::Ready,
        "a failure must still let a press retry"
    );
}

#[test]
fn an_unanswered_watch_fails_at_the_patience_deadline() {
    let now = base();
    let mut devices = Devices::default();
    devices.watch_request(true);
    assert!(!devices.expire(now), "the ask was just stamped");

    let just_before = now + WATCH_PATIENCE - Duration::from_millis(1);
    assert!(!devices.expire(just_before));
    assert_eq!(devices.button(), Button::Off);
    assert_eq!(devices.next_wake(now), Some(WATCH_PATIENCE));

    assert!(devices.expire(now + WATCH_PATIENCE));
    assert_eq!(devices.watch, Watch::Failed(Failure::Devices));
    assert_eq!(devices.button(), Button::Ready);
    assert_eq!(devices.next_wake(now + WATCH_PATIENCE), None);
}

#[test]
fn a_press_in_a_failed_watch_asks_for_a_refresh_and_a_roster_recovers() {
    let now = base();
    let mut devices = Devices::default();
    devices.watch_request(true);
    devices.apply(&failed(Failure::Devices), now);

    devices.list_opened();
    assert_eq!(devices.refresh_request(), Some(Command::RefreshDevices));
    assert_eq!(devices.refresh_request(), None, "one refresh per open");

    assert!(devices.apply(&roster(vec![phone()], Some("a")), now));
    assert_eq!(devices.watch, Watch::Live);
    assert_eq!(devices.button(), Button::Lit);
}

#[test]
fn a_roster_replaces_wholesale_and_an_identical_one_reports_unchanged() {
    let now = base();
    let mut devices = watching(now);
    assert!(
        !devices.apply(&roster(vec![phone(), laptop()], Some("a")), now),
        "an identical roster changed something"
    );

    assert!(devices.apply(&roster(vec![laptop()], Some("b")), now));
    let rows = devices.rows(&Transport::default(), false);
    let names: Vec<&str> = rows.iter().map(|row| row.device.name.as_str()).collect();
    assert_eq!(names, vec!["Laptop"]);
}

#[test]
fn an_empty_roster_is_live_with_a_ready_button() {
    let now = base();
    let mut devices = Devices::default();
    devices.watch_request(true);
    assert!(devices.apply(&roster(Vec::new(), None), now));
    assert_eq!(devices.watch, Watch::Live);
    assert_eq!(devices.button(), Button::Ready);
    assert!(devices.rows(&Transport::default(), false).is_empty());
    assert_eq!(
        devices.notes(now),
        vec![NO_DEVICES.to_owned(), SWITCH_HINT.to_owned()]
    );
}

#[test]
fn a_holder_that_leaves_the_roster_flashes_that_it_went_offline() {
    let now = base();
    let mut devices = remote(now);
    assert!(devices.apply(&roster(vec![laptop()], None), now));

    assert_eq!(devices.caption(now), "Phone went offline");
    assert_eq!(
        devices.source(&Transport::default()),
        Source::Local,
        "the bar must fall back to local"
    );
    assert!(devices.expire(now + FLASH_FOR));
    assert_eq!(devices.caption(now + FLASH_FOR), "");
}

#[test]
fn a_holder_that_stops_holding_but_stays_signed_in_clears_silently() {
    let now = base();
    let mut devices = remote(now);
    assert!(devices.apply(&roster(vec![phone(), laptop()], Some("b")), now));

    assert!(devices.flash.is_none(), "a hand-over is not an offline");
    assert_eq!(devices.caption(now), "Laptop");
}

#[test]
fn a_later_flash_replaces_an_earlier_one() {
    let now = base();
    let mut devices = remote(now);
    devices.apply(&roster(vec![laptop()], Some("b")), now);
    assert_eq!(devices.caption(now), "Phone went offline");

    let later = now + Duration::from_secs(1);
    devices.apply(&roster(vec![device("c", "Tablet")], Some("c")), later);
    assert_eq!(devices.caption(later), "Laptop went offline");
}

#[test]
fn a_roster_that_only_changes_who_holds_is_applied() {
    let now = base();
    let mut devices = Devices::default();
    devices.watch_request(true);
    assert!(devices.apply(&roster_of(vec![phone(), laptop()], Some("a"), false), now));

    assert!(
        devices.apply(&roster_of(vec![phone(), laptop()], Some("a"), true), now),
        "a change of who holds the sound went unseen"
    );
}

#[test]
fn a_failed_update_while_live_keeps_the_roster_and_the_playback() {
    let now = base();
    let mut devices = remote(now);
    assert!(!devices.apply(&failed(Failure::Devices), now));

    assert_eq!(devices.watch, Watch::Live);
    assert_eq!(devices.caption(now), "Phone");
    assert_eq!(devices.source(&Transport::default()), Source::Remote);
    assert_eq!(devices.button(), Button::Lit);
    assert!(devices.flash.is_none(), "a dead pump is not an event");
}

#[test]
fn the_button_is_lit_only_while_a_holder_holds_the_sound() {
    let now = base();
    assert_eq!(Devices::default().button(), Button::Off);

    let mut devices = watching(now);
    assert_eq!(devices.button(), Button::Lit);

    devices.apply(&roster(vec![phone(), laptop()], None), now);
    assert_eq!(devices.button(), Button::Ready);
}

#[test]
fn the_notes_name_the_failure_or_the_empty_list() {
    let now = base();
    let mut devices = Devices::default();
    devices.watch_request(true);
    devices.apply(&failed(Failure::DevicesOff), now);
    assert_eq!(
        devices.notes(now),
        vec![
            failure_message(Failure::DevicesOff).to_owned(),
            SWITCH_HINT.to_owned()
        ]
    );

    devices.apply(&roster(Vec::new(), None), now);
    assert_eq!(
        devices.notes(now),
        vec![NO_DEVICES.to_owned(), SWITCH_HINT.to_owned()]
    );
}

#[test]
fn the_next_wake_is_the_earliest_of_the_watch_and_the_flash() {
    let now = base();
    let mut devices = Devices::default();
    devices.watch_request(true);
    devices.expire(now);
    devices.apply(&Update::Devices(DeviceUpdate::SteerFailed), now);

    assert_eq!(devices.next_wake(now), Some(FLASH_FOR));
    assert!(devices.expire(now + FLASH_FOR));
    assert_eq!(
        devices.next_wake(now + FLASH_FOR),
        Some(WATCH_PATIENCE - FLASH_FOR)
    );
    assert_eq!(devices.button(), Button::Off);
}

#[test]
fn an_update_about_anything_else_is_ignored() {
    let now = base();
    let mut devices = Devices::default();
    assert!(!devices.apply(&Update::SignedIn, now));
    assert!(!devices.apply(
        &Update::Transport(crate::message::TransportUpdate::Loading),
        now
    ));
    assert_eq!(devices.watch, Watch::Idle);
}
