use super::super::*;
use super::test_support::*;

#[test]
fn the_rows_put_the_holder_first_then_by_name() {
    let now = base();
    let mut devices = Devices::default();
    devices.apply(
        &roster(
            vec![
                device("z", "Zed"),
                device("a", "Alpha"),
                device("m", "Beta"),
            ],
            Some("z"),
        ),
        now,
    );
    let rows = devices.rows(&Transport::default(), false);
    let names: Vec<&str> = rows.iter().map(|row| row.device.name.as_str()).collect();
    assert_eq!(names, vec!["Zed", "Alpha", "Beta"]);
    let holders: Vec<bool> = rows.iter().map(|row| row.holder).collect();
    assert_eq!(holders, vec![true, false, false]);
}

#[test]
fn a_row_is_pickable_while_this_window_holds_the_sound() {
    let now = base();
    let mut devices = Devices::default();
    devices.watch_request(true);
    devices.apply(&roster_of(vec![phone(), laptop()], None, true), now);

    let local = Transport::default();
    assert!(devices.can_pick(&local, true, false));
    let pickable: Vec<bool> = devices
        .rows(&local, false)
        .iter()
        .map(|row| row.pickable)
        .collect();
    assert_eq!(pickable, vec![true, true]);
}

#[test]
fn a_row_is_pickable_while_this_window_plays_locally() {
    let now = base();
    // Nothing plays anywhere else, so the account reports no player at all.
    // This window is the one making sound, and that is enough to carry it.
    let devices = watching(now);
    let local = Transport {
        state: TransportState::Playing,
        ..Transport::default()
    };

    assert!(devices.can_pick(&local, false, true));
    let pickable: Vec<bool> = devices
        .rows(&local, true)
        .iter()
        .map(|row| row.pickable)
        .collect();
    assert_eq!(pickable, vec![true, true]);
}

#[test]
fn a_row_is_not_pickable_without_a_current_row() {
    let now = base();
    // The transport plays but the queue holds no current entry, so there is
    // no state of this window's own to hand over.
    let devices = watching(now);
    let local = Transport {
        state: TransportState::Playing,
        ..Transport::default()
    };

    assert!(!devices.can_pick(&local, false, false));
    assert!(devices.rows(&local, false).iter().all(|row| !row.pickable));
}

#[test]
fn a_remote_source_makes_no_row_pickable() {
    let now = base();
    let devices = remote(now);

    for state in [
        TransportState::Idle,
        TransportState::Paused,
        TransportState::Stopped,
    ] {
        let local = Transport {
            state,
            ..Transport::default()
        };
        assert!(!devices.can_pick(&local, false, true), "{state:?}");
        assert!(
            devices.rows(&local, true).iter().all(|row| !row.pickable),
            "{state:?}"
        );
    }
}

#[test]
fn pick_returns_the_transfer_command_for_a_known_device() {
    let now = base();
    let mut devices = remote(now);

    assert_eq!(
        devices.pick("b", true),
        Some(Command::Transfer { to: "b".to_owned() })
    );
    assert_eq!(devices.pick_request(), None, "a sent pick was kept too");
}

#[test]
fn pick_of_an_unknown_device_returns_nothing() {
    let now = base();
    let mut devices = remote(now);

    assert_eq!(devices.pick("z", true), None);
    assert_eq!(devices.pick("z", false), None);
    assert_eq!(
        devices.pick_request(),
        None,
        "an unknown device was kept for a later frame"
    );
}

#[test]
fn a_pick_in_a_taken_frame_is_delayed_not_dropped() {
    let now = base();
    let mut devices = remote(now);

    assert_eq!(devices.pick("b", false), None, "a taken frame sent anyway");
    assert_eq!(
        devices.pick_request(),
        Some(Command::Transfer { to: "b".to_owned() })
    );
    assert_eq!(devices.pick_request(), None, "one pick was sent twice");
}

#[test]
fn every_kind_has_a_word() {
    let labels = [
        DeviceKind::Computer,
        DeviceKind::Phone,
        DeviceKind::Tablet,
        DeviceKind::Speaker,
        DeviceKind::Tv,
        DeviceKind::Car,
        DeviceKind::Other,
    ]
    .map(kind_label);
    assert_eq!(
        labels,
        [
            "Computer", "Phone", "Tablet", "Speaker", "TV", "Car", "Other"
        ]
    );
}
