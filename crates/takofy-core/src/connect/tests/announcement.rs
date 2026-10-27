use librespot::protocol::connect::{
    Capabilities, Device as WireDevice, DeviceInfo, MemberType, PutStateReason, PutStateRequest,
};
use librespot::protocol::devices::DeviceType;
use librespot::protocol::media::AudioQuality;
use librespot::protocol::player::{ContextPlayerOptions, PlayOrigin, PlayerState, Suppressions};
use protobuf::{EnumOrUnknown, Message as _, MessageField};
use takofy_model::{Report, Status, TrackEntry};

use super::super::*;

#[test]
fn announcement_is_exactly_the_probed_controller_body() {
    let announcement = Announcement::controller("device-id", "client-id");
    let expected = PutStateRequest {
        member_type: EnumOrUnknown::new(MemberType::CONNECT_STATE),
        put_state_reason: EnumOrUnknown::new(PutStateReason::NEW_DEVICE),
        device: MessageField::some(WireDevice {
            device_info: MessageField::some(DeviceInfo {
                can_play: false,
                volume: 0,
                name: "Takofy".to_owned(),
                capabilities: MessageField::some(Capabilities {
                    can_be_player: false,
                    ..Default::default()
                }),
                device_software_version: env!("CARGO_PKG_VERSION").to_owned(),
                device_type: EnumOrUnknown::new(DeviceType::COMPUTER),
                spirc_version: "3.2.4".to_owned(),
                device_id: "device-id".to_owned(),
                client_id: "client-id".to_owned(),
                ..Default::default()
            }),
            ..Default::default()
        }),
        is_active: false,
        ..Default::default()
    };

    assert_eq!(announcement.message(), &expected);
    assert_eq!(
        announcement.message().write_to_bytes().expect("serialises"),
        expected.write_to_bytes().expect("serialises"),
    );
}

#[test]
fn announcement_is_inactive() {
    assert!(
        !Announcement::controller("device", "client")
            .message()
            .is_active
    );
}

#[test]
fn announcement_cannot_play() {
    let body = Announcement::controller("device", "client");
    let info = body
        .message()
        .device
        .as_ref()
        .expect("an announcement names a device")
        .device_info
        .as_ref()
        .expect("a device names its info");

    assert!(!info.can_play);
    assert_eq!(
        info.capabilities,
        MessageField::some(Capabilities {
            can_be_player: false,
            ..Default::default()
        })
    );
}

#[test]
fn announcement_has_no_player_state() {
    let body = Announcement::controller("device", "client");
    let device = body
        .message()
        .device
        .as_ref()
        .expect("an announcement names a device");

    assert!(device.player_state.is_none());
}

#[test]
fn announcement_is_a_new_connect_state() {
    let body = Announcement::controller("device", "client");

    assert_eq!(
        body.message().member_type.enum_value().expect("known"),
        MemberType::CONNECT_STATE
    );
    assert_eq!(
        body.message().put_state_reason.enum_value().expect("known"),
        PutStateReason::NEW_DEVICE
    );
}

#[test]
fn announcement_carries_the_ids_name_and_versions() {
    let body = Announcement::controller("device-id", "client-id");
    let info = body
        .message()
        .device
        .as_ref()
        .expect("an announcement names a device")
        .device_info
        .as_ref()
        .expect("a device names its info");

    assert_eq!(info.device_id, "device-id");
    assert_eq!(info.client_id, "client-id");
    assert_eq!(info.name, DEVICE_NAME);
    assert_eq!(info.volume, 0);
    assert_eq!(
        info.device_type.enum_value().expect("known"),
        DeviceType::COMPUTER
    );
    assert_eq!(info.spirc_version, "3.2.4");
    assert_eq!(info.device_software_version, env!("CARGO_PKG_VERSION"));
}

#[test]
fn ready_announcement_is_the_receiver_shape_but_inactive() {
    let announcement = Announcement::ready("device-id", "client-id", "session-id");
    let expected = PutStateRequest {
        member_type: EnumOrUnknown::new(MemberType::CONNECT_STATE),
        put_state_reason: EnumOrUnknown::new(PutStateReason::NEW_DEVICE),
        device: MessageField::some(WireDevice {
            device_info: MessageField::some(DeviceInfo {
                can_play: true,
                volume: 0,
                name: "Takofy".to_owned(),
                capabilities: MessageField::some(Capabilities {
                    volume_steps: 0,
                    disable_volume: true,
                    gaia_eq_connect_id: true,
                    can_be_player: true,
                    needs_full_player_state: true,
                    is_observable: true,
                    is_controllable: true,
                    hidden: false,
                    supports_gzip_pushes: true,
                    supports_logout: false,
                    supported_types: vec![
                        "audio/episode".to_owned(),
                        "audio/track".to_owned(),
                        "audio/local".to_owned(),
                    ],
                    supports_playlist_v2: true,
                    supports_transfer_command: true,
                    supports_command_request: true,
                    supports_set_options_command: true,
                    is_voice_enabled: false,
                    restrict_to_local: false,
                    connect_disabled: false,
                    supports_rename: false,
                    supports_external_episodes: false,
                    supports_set_backend_metadata: false,
                    supports_hifi: MessageField::none(),
                    supports_dj: false,
                    supports_rooms: false,
                    supported_audio_quality: EnumOrUnknown::new(AudioQuality::VERY_HIGH),
                    command_acks: true,
                    ..Default::default()
                }),
                device_software_version: env!("CARGO_PKG_VERSION").to_owned(),
                device_type: EnumOrUnknown::new(DeviceType::COMPUTER),
                spirc_version: "3.2.4".to_owned(),
                device_id: "device-id".to_owned(),
                client_id: "client-id".to_owned(),
                ..Default::default()
            }),
            player_state: MessageField::some(PlayerState {
                session_id: "session-id".to_owned(),
                is_system_initiated: true,
                playback_speed: 1.0,
                play_origin: MessageField::some(PlayOrigin::new()),
                suppressions: MessageField::some(Suppressions::new()),
                options: MessageField::some(ContextPlayerOptions::new()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        is_active: false,
        ..Default::default()
    };

    assert_eq!(announcement.message(), &expected);
    assert_eq!(
        announcement.message().write_to_bytes().expect("serialises"),
        expected.write_to_bytes().expect("serialises"),
    );
}

fn ready_body() -> PutStateRequest {
    Announcement::ready("device-id", "client-id", "session-id")
        .message()
        .clone()
}

fn ready_device(body: &PutStateRequest) -> &WireDevice {
    body.device
        .as_ref()
        .expect("an announcement names a device")
}

fn ready_info(body: &PutStateRequest) -> &DeviceInfo {
    ready_device(body)
        .device_info
        .as_ref()
        .expect("a device names its info")
}

fn ready_capabilities(body: &PutStateRequest) -> &Capabilities {
    ready_info(body)
        .capabilities
        .as_ref()
        .expect("a receiver names its capabilities")
}

fn body_player(body: &PutStateRequest) -> &PlayerState {
    ready_device(body)
        .player_state
        .as_ref()
        .expect("a receiver names its player state")
}

/// The report an active body publishes: one row behind and one ahead.
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
        prev: vec![entry("spotify:track:before")],
        next: vec![entry("spotify:track:after")],
        context: Some("spotify:album:context".to_owned()),
        queue_revision: String::new(),
        options: None,
        position_ms: 10_000,
        duration_ms: Some(200_000),
    }
}

/// When this window took the sound.
const STARTED_AT_MS: u64 = 1_700_000_000_000;

/// How old the report was when the body was built.
const AGE_MS: i64 = 3_000;

fn active_state(report: Report) -> ActiveState {
    ActiveState::new(report, AGE_MS).expect("the fixture names a track")
}

fn active_body(ack: Option<Ack>) -> PutStateRequest {
    Announcement::active(
        "device-id",
        "client-id",
        "session-id",
        &active_state(report()),
        ack,
        STARTED_AT_MS,
    )
    .message()
    .clone()
}

#[test]
fn ready_is_inactive_and_carries_no_track() {
    let body = ready_body();

    assert!(!body.is_active);
    assert!(body_player(&body).track.is_none());
}

#[test]
fn ready_claims_to_play_and_be_commanded() {
    let body = ready_body();
    let capabilities = ready_capabilities(&body);

    assert!(ready_info(&body).can_play);
    assert!(capabilities.can_be_player);
    assert!(capabilities.supports_transfer_command);
    assert!(capabilities.supports_command_request);
    assert!(capabilities.supports_set_options_command);
    assert!(capabilities.command_acks);
}

#[test]
fn ready_disables_volume() {
    let body = ready_body();

    assert!(ready_capabilities(&body).disable_volume);
    assert_eq!(ready_capabilities(&body).volume_steps, 0);
    assert_eq!(ready_info(&body).volume, 0);
}

#[test]
fn ready_carries_the_session_id() {
    assert_eq!(body_player(&ready_body()).session_id, "session-id");
}

#[test]
fn ready_player_state_has_the_idle_shape() {
    let body = ready_body();
    let player = body_player(&body);

    assert!(player.is_system_initiated);
    assert_eq!(player.playback_speed, 1.0);
    assert_eq!(player.play_origin, MessageField::some(PlayOrigin::new()));
    assert_eq!(player.suppressions, MessageField::some(Suppressions::new()));
    assert_eq!(
        player.options,
        MessageField::some(ContextPlayerOptions::new())
    );
}

#[test]
fn active_announcement_is_active_and_carries_the_player_state() {
    let body = active_body(None);
    let player = body_player(&body);

    assert!(body.is_active);
    assert_eq!(
        body.member_type.enum_value().expect("known"),
        MemberType::CONNECT_STATE
    );
    assert_eq!(
        body.put_state_reason.enum_value().expect("known"),
        PutStateReason::PLAYER_STATE_CHANGED
    );
    assert_eq!(body.started_playing_at, STARTED_AT_MS);
    assert_eq!(body.client_side_timestamp, STARTED_AT_MS);

    assert_eq!(player.session_id, "session-id");
    assert_eq!(
        player.timestamp,
        i64::try_from(STARTED_AT_MS).expect("an instant fits an i64")
    );
    assert_eq!(
        (player.is_playing, player.is_paused, player.is_buffering),
        (true, false, false)
    );
    assert_eq!((player.prev_tracks.len(), player.next_tracks.len()), (1, 1));
    assert_eq!(
        player
            .track
            .as_ref()
            .expect("an active body names its track")
            .uri,
        "spotify:track:current"
    );
    assert_eq!(player.index.as_ref().expect("an index").track, 1);
    assert_eq!(player.context_uri, "spotify:album:context");
    assert_eq!(
        player.position_as_of_timestamp,
        10_000 + AGE_MS,
        "the position advances with the age the caller measured"
    );
    assert_eq!(player.duration, 200_000);
}

#[test]
fn active_keeps_the_ready_capabilities_and_cannot_be_trackless() {
    let body = active_body(None);

    assert!(ready_info(&body).can_play);
    assert_eq!(ready_capabilities(&body), &receiver_capabilities());

    let mut trackless = report();
    trackless.track = None;
    assert!(
        ActiveState::new(trackless, AGE_MS).is_none(),
        "a report with no track cannot hold the sound"
    );
    let mut unnamed = report();
    unnamed.track = Some(entry(""));
    assert!(ActiveState::new(unnamed, AGE_MS).is_none());
}

#[test]
fn active_carries_the_last_command_ack() {
    let body = active_body(Some(Ack {
        message_id: 42,
        sender: "phone".to_owned(),
    }));

    assert_eq!(body.last_command_message_id, 42);
    assert_eq!(body.last_command_sent_by_device_id, "phone");
}

#[test]
fn active_without_an_ack_carries_no_command_ids() {
    let body = active_body(None);

    assert_eq!(body.last_command_message_id, 0);
    assert!(body.last_command_sent_by_device_id.is_empty());
}

#[test]
fn each_announcement_kind_has_its_own_flags() {
    let controller = Announcement::controller("device", "client")
        .message()
        .clone();
    assert!(!ready_info(&controller).can_play);
    assert!(!controller.is_active);
    assert!(
        controller
            .device
            .as_ref()
            .expect("a device")
            .player_state
            .is_none()
    );

    let ready = ready_body();
    assert!(ready_info(&ready).can_play);
    assert!(!ready.is_active);
    assert!(body_player(&ready).track.is_none());

    let active = active_body(None);
    assert!(active.is_active);
    assert!(
        body_player(&active)
            .track
            .as_ref()
            .is_some_and(|track| !track.uri.is_empty()),
        "an active body always names a track"
    );
}
