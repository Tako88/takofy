use super::super::*;
use super::test_support::*;

#[test]
fn a_playing_update_without_a_holder_is_ignored() {
    let now = base();
    let mut devices = Devices::default();
    assert!(!devices.apply(
        &playing(Some(playback(
            RemoteState::Playing,
            "spotify:track:a",
            Some(10_000)
        ))),
        now
    ));
    assert_eq!(devices.playback, None);
    assert_eq!(devices.watch, Watch::Idle, "an ignored push answered");
    assert_eq!(devices.source(&Transport::default()), Source::Local);
}

#[test]
fn a_playing_update_maps_the_remote_state_onto_the_display_transport() {
    let now = base();
    for (remote_state, state) in [
        (RemoteState::Playing, TransportState::Playing),
        (RemoteState::Paused, TransportState::Paused),
        (RemoteState::Loading, TransportState::Loading),
    ] {
        let mut devices = watching(now);
        devices.apply(
            &playing(Some(playback(
                remote_state,
                "spotify:track:a",
                Some(10_000),
            ))),
            now,
        );
        assert_eq!(
            devices.remote.state, state,
            "{remote_state:?} mapped to the wrong state"
        );
    }
}

#[test]
fn a_playing_update_anchors_the_position_and_the_length_at_the_push() {
    let now = base();
    let local = Transport::default();
    let mut devices = watching(now);
    // The preview a finished drag left, waiting for the push that confirms it.
    devices.remote.scrub = Some(42);

    assert!(devices.apply(
        &playing(Some(playback(
            RemoteState::Playing,
            "spotify:track:a",
            Some(10_000)
        ))),
        now
    ));

    assert_eq!(devices.shown_position(now), 1_000);
    assert_eq!(
        devices.shown_position(now + Duration::from_millis(500)),
        1_500
    );
    assert_eq!(devices.remote.duration_ms, Some(10_000));
    assert_eq!(devices.remote.scrub, None, "a push ends a stale preview");
    assert!(!devices.remote.dragging);
    assert_eq!(local, Transport::default(), "the local transport moved");
    assert_eq!(devices.source(&local), Source::Remote);
}

#[test]
fn a_push_mid_drag_keeps_the_seek_the_release_asks_for() {
    let now = base();
    let mut devices = remote(now);
    devices.remote.scrub = Some(4_000);
    devices.remote.dragging = true;

    assert!(devices.apply(
        &playing(Some(playback(
            RemoteState::Playing,
            "spotify:track:a",
            Some(10_000)
        ))),
        now
    ));

    assert_eq!(
        devices.remote.scrub,
        Some(4_000),
        "a push dropped the pointer's preview"
    );
    assert!(devices.remote.dragging, "a push ended the drag");
    assert_eq!(
        devices.shown_position(now),
        4_000,
        "the bar draws the push over the drag"
    );
    assert_eq!(
        devices.remote.scrub_end(),
        Some(Command::Seek { position_ms: 4_000 }),
        "the release no longer asks for the place it landed on"
    );
}

#[test]
fn a_playing_update_that_names_nothing_returns_the_transport_to_idle() {
    let now = base();
    let mut devices = remote(now);
    assert!(devices.apply(&playing(None), now));
    assert_eq!(devices.playback, None);
    assert_eq!(devices.remote.state, TransportState::Idle);
    assert_eq!(devices.remote.position_ms, 0);
    assert_eq!(devices.remote.duration_ms, None);
    assert_eq!(devices.source(&Transport::default()), Source::Local);
    assert!(
        !devices.apply(&playing(None), now),
        "nothing playing changed nothing"
    );
}

#[test]
fn steer_is_refused_without_a_holder_or_a_steerable_device() {
    let now = base();
    assert_eq!(Devices::default().steer(Steer::Pause, now), None);

    let mut devices = watching(now);
    assert_eq!(
        devices.steer(Steer::Pause, now),
        None,
        "nothing is known about what the holder plays"
    );

    let forbidden = RemotePlayback {
        steerable: false,
        ..playback(RemoteState::Playing, "spotify:track:a", Some(10_000))
    };
    devices.apply(&playing(Some(forbidden)), now);
    for steer in [
        Steer::Pause,
        Steer::Resume,
        Steer::SkipNext,
        Steer::SkipPrev,
        Steer::SeekTo(1_000),
    ] {
        assert_eq!(devices.steer(steer, now), None, "{steer:?} was steered");
    }
}

#[test]
fn a_seek_is_refused_without_a_known_length() {
    let now = base();
    let mut devices = watching(now);
    devices.apply(
        &playing(Some(playback(
            RemoteState::Playing,
            "spotify:track:a",
            None,
        ))),
        now,
    );

    assert_eq!(devices.steer(Steer::SeekTo(1_000), now), None);
    assert_eq!(
        devices.steer(Steer::Pause, now),
        Some(Command::Steer {
            device: "a".to_owned(),
            steer: Steer::Pause,
        })
    );
}

#[test]
fn steerable_names_a_known_holder_that_allows_remote_control() {
    let now = base();
    assert!(!Devices::default().steerable(), "nothing is known yet");

    let mut devices = watching(now);
    assert!(
        !devices.steerable(),
        "nothing is known about what the holder plays"
    );

    devices.apply(
        &playing(Some(playback(
            RemoteState::Playing,
            "spotify:track:a",
            Some(10_000),
        ))),
        now,
    );
    assert!(devices.steerable());

    devices.apply(&playing(None), now);
    assert!(
        !devices.steerable(),
        "a holder with no playback is not a transport"
    );
}

#[test]
fn a_seek_is_allowed_while_the_holder_is_still_loading() {
    let now = base();
    let mut devices = watching(now);
    devices.apply(
        &playing(Some(playback(
            RemoteState::Loading,
            "spotify:track:a",
            Some(10_000),
        ))),
        now,
    );

    assert_eq!(
        devices.steer(Steer::SeekTo(4_000), now),
        Some(Command::Steer {
            device: "a".to_owned(),
            steer: Steer::SeekTo(4_000),
        }),
        "the bar enables a seek on the length, so the app must act on that"
    );
}

#[test]
fn steer_names_the_holder_it_was_pressed_against() {
    let now = base();
    let mut devices = remote(now);
    let pressed = devices.steer(Steer::SkipNext, now);

    devices.apply(&roster(vec![phone(), laptop()], Some("b")), now);
    assert_eq!(
        pressed,
        Some(Command::Steer {
            device: "a".to_owned(),
            steer: Steer::SkipNext,
        }),
        "a holder change after the press must not retarget the command"
    );
}

#[test]
fn a_steer_moves_the_bar_to_the_seeked_place() {
    let now = base();
    let mut devices = remote(now);
    assert!(devices.apply(
        &playing(Some(playback(
            RemoteState::Playing,
            "spotify:track:a",
            Some(10_000)
        ))),
        now
    ));
    assert_eq!(
        devices.steer(Steer::SeekTo(6_000), now),
        Some(Command::Steer {
            device: "a".to_owned(),
            steer: Steer::SeekTo(6_000),
        })
    );
    assert_eq!(devices.shown_position(now), 6_000);
    assert_eq!(
        devices.shown_position(now + Duration::from_millis(500)),
        6_500
    );
}

#[test]
fn a_failed_steer_flashes_its_copy() {
    let now = base();
    let mut devices = remote(now);
    assert!(devices.apply(&Update::Devices(DeviceUpdate::SteerFailed), now));
    assert_eq!(devices.caption(now), failure_message(Failure::Steer));
    assert!(devices.expire(now + FLASH_FOR));
    assert_eq!(devices.caption(now + FLASH_FOR), "Phone");
}

#[test]
fn a_failed_transfer_flashes_its_copy() {
    let now = base();
    let mut devices = remote(now);
    assert!(devices.apply(
        &Update::Devices(DeviceUpdate::TransferFailed(Failure::Transfer)),
        now
    ));
    assert_eq!(devices.caption(now), failure_message(Failure::Transfer));
}

#[test]
fn the_play_pause_steer_follows_the_holders_state() {
    let now = base();
    let devices = Devices::default();
    assert_eq!(devices.toggle(), None, "nothing holds the sound yet");

    let mut devices = watching(now);
    assert_eq!(
        devices.toggle(),
        None,
        "a holder with no state toggles nothing"
    );

    for (state, steer) in [
        (RemoteState::Playing, Some(Steer::Pause)),
        (RemoteState::Loading, Some(Steer::Pause)),
        (RemoteState::Paused, Some(Steer::Resume)),
    ] {
        devices.apply(&playing(Some(playback(state, "t1", Some(180_000)))), now);
        assert_eq!(devices.toggle(), steer, "{state:?}");
    }
}
