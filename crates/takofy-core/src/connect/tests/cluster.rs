use librespot::protocol::connect::{Capabilities, Cluster, ClusterUpdate, DeviceInfo};
use librespot::protocol::devices::DeviceType;
use protobuf::{EnumOrUnknown, MessageField};
use takofy_model::DeviceKind;

use super::super::*;

fn device_info(name: &str, device_type: DeviceType) -> DeviceInfo {
    DeviceInfo {
        can_play: true,
        name: name.to_owned(),
        device_type: EnumOrUnknown::new(device_type),
        capabilities: MessageField::some(Capabilities {
            can_be_player: true,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn cluster(devices: Vec<(&str, DeviceInfo)>, active: &str) -> Cluster {
    Cluster {
        active_device_id: active.to_owned(),
        device: devices
            .into_iter()
            .map(|(id, info)| (id.to_owned(), info))
            .collect(),
        ..Default::default()
    }
}

fn ids(view: &ClusterView) -> Vec<&str> {
    view.devices
        .as_ref()
        .expect("the view has a roster")
        .iter()
        .map(|device| device.id.as_str())
        .collect()
}

#[test]
fn held_here_is_true_only_for_our_own_id() {
    let cases = [
        ("own", "own", true),
        ("phone", "own", false),
        ("", "own", false),
    ];

    for (active, own, expected) in cases {
        let view = view_of(&cluster(Vec::new(), active), own);
        assert_eq!(view.held_here, expected, "active={active:?} own={own:?}");
    }
}

#[test]
fn every_error_has_its_own_word() {
    let words = [
        ConnectError::NoConnectionId,
        ConnectError::AlreadyWatching,
        ConnectError::Withdrawn,
        ConnectError::Busy,
        ConnectError::InvalidTarget,
        ConnectError::DealerSpent,
        ConnectError::WithdrawTimedOut,
        ConnectError::PutTimedOut,
        ConnectError::HandOverTimedOut,
        ConnectError::HandOverPending,
        ConnectError::Expired,
        ConnectError::NoTrack,
        ConnectError::NoContext,
        ConnectError::NotPlaying,
        ConnectError::NotPlayable,
    ]
    .map(|error| error.word());
    let mut distinct = words.to_vec();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), words.len(), "{words:?}");
    for word in words {
        assert!(!word.is_empty());
    }
}

#[test]
fn a_wire_failure_logs_the_kind_that_refused_it() {
    let refused = ConnectError::Session(librespot::core::Error::new(
        librespot::core::error::ErrorKind::InvalidArgument,
        std::io::Error::other("a refusal"),
    ));
    let line = refused.log_line();
    assert!(line.starts_with(refused.word()), "{line}");
    assert!(line.contains("invalid argument"), "{line}");
    assert!(
        !line.contains("a refusal"),
        "the wire's text leaked: {line}"
    );

    // Every other error logs its word alone, with nothing appended.
    assert_eq!(ConnectError::NotPlaying.log_line(), "nothing playing");
}

#[test]
fn nobody_is_not_held_here() {
    assert!(!view_of(&Cluster::default(), "own").held_here);
    assert!(!view_of(&cluster(Vec::new(), ""), "own").held_here);
}

#[test]
fn changed_ms_is_the_clusters() {
    let mut cluster = cluster(Vec::new(), "phone");
    cluster.changed_timestamp_ms = 1_234;

    assert_eq!(view_of(&cluster, "own").changed_ms, 1_234);
    assert_eq!(view_of(&Cluster::default(), "own").changed_ms, 0);
}

#[test]
fn own_device_is_not_listed() {
    let view = view_of(
        &cluster(
            vec![
                ("own", device_info("Mine", DeviceType::COMPUTER)),
                ("phone", device_info("Phone", DeviceType::SMARTPHONE)),
            ],
            "own",
        ),
        "own",
    );

    assert_eq!(ids(&view), ["phone"]);
}

#[test]
fn the_map_key_is_the_device_id() {
    let mut missing = device_info("Phone", DeviceType::SMARTPHONE);
    missing.device_id = String::new();
    let mut differing = device_info("Tablet", DeviceType::TABLET);
    differing.device_id = "some-other-id".to_owned();

    let view = view_of(
        &cluster(vec![("phone", missing), ("tablet", differing)], ""),
        "own",
    );
    let devices = view.devices.expect("a roster");

    assert_eq!(devices[0].id, "phone");
    assert_eq!(devices[1].id, "tablet");
}

#[test]
fn a_device_that_cannot_play_is_not_listed() {
    let mut cannot = device_info("Phone", DeviceType::SMARTPHONE);
    cannot.capabilities = MessageField::some(Capabilities {
        can_be_player: false,
        ..Default::default()
    });

    let view = view_of(&cluster(vec![("phone", cannot)], ""), "own");

    assert!(ids(&view).is_empty());
}

#[test]
fn a_hidden_device_is_not_listed() {
    let mut hidden = device_info("Phone", DeviceType::SMARTPHONE);
    hidden.capabilities = MessageField::some(Capabilities {
        can_be_player: true,
        hidden: true,
        ..Default::default()
    });

    let view = view_of(&cluster(vec![("phone", hidden)], ""), "own");

    assert!(ids(&view).is_empty());
}

#[test]
fn an_offline_device_is_not_listed() {
    let mut offline = device_info("Phone", DeviceType::SMARTPHONE);
    offline.is_offline = true;

    let view = view_of(&cluster(vec![("phone", offline)], ""), "own");

    assert!(ids(&view).is_empty());
}

#[test]
fn the_roster_is_sorted_by_name_then_id() {
    let view = view_of(
        &cluster(
            vec![
                ("z", device_info("Beta", DeviceType::COMPUTER)),
                ("b", device_info("Alpha", DeviceType::COMPUTER)),
                ("a", device_info("Alpha", DeviceType::TABLET)),
            ],
            "",
        ),
        "own",
    );

    assert_eq!(ids(&view), ["a", "b", "z"]);
}

#[test]
fn every_device_type_maps() {
    let cases = [
        (DeviceType::UNKNOWN, DeviceKind::Other),
        (DeviceType::COMPUTER, DeviceKind::Computer),
        (DeviceType::TABLET, DeviceKind::Tablet),
        (DeviceType::SMARTPHONE, DeviceKind::Phone),
        (DeviceType::SPEAKER, DeviceKind::Speaker),
        (DeviceType::TV, DeviceKind::Tv),
        (DeviceType::AVR, DeviceKind::Other),
        (DeviceType::STB, DeviceKind::Other),
        (DeviceType::AUDIO_DONGLE, DeviceKind::Other),
        (DeviceType::GAME_CONSOLE, DeviceKind::Other),
        (DeviceType::CAST_VIDEO, DeviceKind::Other),
        (DeviceType::CAST_AUDIO, DeviceKind::Other),
        (DeviceType::AUTOMOBILE, DeviceKind::Car),
        (DeviceType::SMARTWATCH, DeviceKind::Other),
        (DeviceType::CHROMEBOOK, DeviceKind::Other),
        (DeviceType::UNKNOWN_SPOTIFY, DeviceKind::Other),
        (DeviceType::CAR_THING, DeviceKind::Other),
        (DeviceType::OBSERVER, DeviceKind::Other),
        (DeviceType::HOME_THING, DeviceKind::Other),
    ];

    for (wire, expected) in cases {
        assert_eq!(kind_of(wire), expected, "{wire:?}");
    }
}

#[test]
fn an_empty_device_map_has_no_roster() {
    let view = view_of(&Cluster::default(), "own");

    assert!(view.devices.is_none());
}

#[test]
fn an_update_without_a_cluster_has_no_view() {
    assert!(view_of_update(&ClusterUpdate::default(), "own").is_none());
}

#[test]
fn an_update_carries_the_cluster_view() {
    let update = ClusterUpdate {
        cluster: MessageField::some(cluster(
            vec![("phone", device_info("Phone", DeviceType::SMARTPHONE))],
            "phone",
        )),
        ..Default::default()
    };

    let view = view_of_update(&update, "own").expect("the update carries a cluster");

    assert_eq!(ids(&view), ["phone"]);
    assert_eq!(view.holder.as_deref(), Some("phone"));
}

#[test]
fn no_holder_for_an_empty_active_id() {
    let view = view_of(
        &cluster(
            vec![("phone", device_info("Phone", DeviceType::SMARTPHONE))],
            "",
        ),
        "own",
    );

    assert!(view.holder.is_none());
}

#[test]
fn no_holder_for_our_own_device() {
    let view = view_of(
        &cluster(
            vec![("phone", device_info("Phone", DeviceType::SMARTPHONE))],
            "own",
        ),
        "own",
    );

    assert!(view.holder.is_none());
}

#[test]
fn a_holder_missing_from_the_roster_is_still_the_holder() {
    let view = view_of(
        &cluster(
            vec![("phone", device_info("Phone", DeviceType::SMARTPHONE))],
            "ghost",
        ),
        "own",
    );

    assert_eq!(view.holder.as_deref(), Some("ghost"));
    assert_eq!(ids(&view), ["phone"]);
}
