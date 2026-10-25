//! The account's devices: what this client announces and what the cluster says.
//!
//! The announcement is the safety boundary: every constructor builds an
//! inactive body, and the receiver's carries no track, so announcing can never
//! take the account's sound. Parsing only reads; nothing here hands the sound
//! anywhere.

use std::time::Duration;

use librespot::protocol::connect::{
    Capabilities, Cluster, ClusterUpdate, Device as WireDevice, DeviceInfo, MemberType,
    PutStateReason, PutStateRequest,
};
use librespot::protocol::devices::DeviceType;
use librespot::protocol::media::AudioQuality;
use librespot::protocol::player::{
    ContextPlayerOptions, PlayOrigin, PlayerState, ProvidedTrack, Suppressions,
};
use protobuf::{EnumOrUnknown, MessageField};
use takofy_model::{Device, DeviceKind, RemotePlayback, RemoteState, TrackEntry};

use crate::playstate::{ActiveState, player_state_of};
use crate::registry::Ack;

/// The name this client announces itself under.
pub const DEVICE_NAME: &str = "Takofy";

/// The spirc version the picker reads on every body this client announces.
const SPIRC_VERSION: &str = "3.2.4";

/// The ceiling on a push's elapsed time when the cluster names no duration.
///
/// A push's `timestamp` is the playing device's last state change, not the
/// push's own arrival, so a phone two minutes into a track legitimately reports
/// two minutes of elapsed time. This only stops a wildly wrong clock from
/// saturating the position; a known duration is the real bound.
pub const MAX_EXTRAPOLATION_MS: i64 = 12 * 60 * 60 * 1_000;

/// The bound on the exit's delete and close.
pub const WITHDRAW_TIMEOUT: Duration = Duration::from_secs(3);

/// This client's own entry on the account.
///
/// The request is private so no other body can be sent through the wire seam.
#[derive(Debug, Clone, PartialEq)]
pub struct Announcement(PutStateRequest);

impl Announcement {
    /// A controller that cannot play: it exists to read the list and steer.
    pub fn controller(device_id: &str, client_id: &str) -> Self {
        let info = DeviceInfo {
            can_play: false,
            volume: 0,
            name: DEVICE_NAME.to_owned(),
            capabilities: MessageField::some(Capabilities {
                can_be_player: false,
                ..Default::default()
            }),
            device_software_version: env!("CARGO_PKG_VERSION").to_owned(),
            device_type: EnumOrUnknown::new(DeviceType::COMPUTER),
            spirc_version: SPIRC_VERSION.to_owned(),
            device_id: device_id.to_owned(),
            client_id: client_id.to_owned(),
            ..Default::default()
        };
        Self(PutStateRequest {
            member_type: EnumOrUnknown::new(MemberType::CONNECT_STATE),
            put_state_reason: EnumOrUnknown::new(PutStateReason::NEW_DEVICE),
            device: MessageField::some(WireDevice {
                device_info: MessageField::some(info),
                ..Default::default()
            }),
            is_active: false,
            ..Default::default()
        })
    }

    /// The body of a device that can play: a receiver waiting to be picked.
    ///
    /// The capabilities are librespot's own receiver set, because that is the
    /// shape the picker has been proven to accept. Volume is disabled rather
    /// than obeyed, and the player state is idle and trackless, so this body
    /// cannot take the sound until a landing fills it in.
    pub fn ready(device_id: &str, client_id: &str, session_id: &str) -> Self {
        Self(PutStateRequest {
            member_type: EnumOrUnknown::new(MemberType::CONNECT_STATE),
            put_state_reason: EnumOrUnknown::new(PutStateReason::NEW_DEVICE),
            device: MessageField::some(WireDevice {
                device_info: MessageField::some(receiver_info(device_id, client_id)),
                player_state: MessageField::some(idle_player_state(session_id)),
                ..Default::default()
            }),
            is_active: false,
            ..Default::default()
        })
    }

    /// The body of a device that holds the sound.
    ///
    /// The track comes from [`ActiveState`], which cannot be built without one,
    /// so an active body is never empty. It keeps the ready capabilities: the
    /// picker must go on offering this device while it plays.
    pub(crate) fn active(
        device_id: &str,
        client_id: &str,
        session_id: &str,
        state: &ActiveState,
        ack: Option<Ack>,
        started_at_ms: u64,
    ) -> Self {
        // Built when the sound is taken, so one instant stamps the start, the
        // clock and the state it carries.
        let now_ms = i64::try_from(started_at_ms).unwrap_or(i64::MAX);
        let (message_id, sender) =
            ack.map_or((0, String::new()), |ack| (ack.message_id, ack.sender));
        Self(PutStateRequest {
            member_type: EnumOrUnknown::new(MemberType::CONNECT_STATE),
            put_state_reason: EnumOrUnknown::new(PutStateReason::PLAYER_STATE_CHANGED),
            device: MessageField::some(WireDevice {
                device_info: MessageField::some(receiver_info(device_id, client_id)),
                player_state: MessageField::some(player_state_of(
                    state.report(),
                    state.age_ms(),
                    now_ms,
                    session_id,
                )),
                ..Default::default()
            }),
            is_active: true,
            last_command_message_id: message_id,
            last_command_sent_by_device_id: sender,
            started_playing_at: started_at_ms,
            client_side_timestamp: started_at_ms,
            ..Default::default()
        })
    }

    /// The body the wire sends.
    pub fn message(&self) -> &PutStateRequest {
        &self.0
    }
}

/// The device info of a device that can play, with volume disabled.
fn receiver_info(device_id: &str, client_id: &str) -> DeviceInfo {
    DeviceInfo {
        can_play: true,
        volume: 0,
        name: DEVICE_NAME.to_owned(),
        capabilities: MessageField::some(receiver_capabilities()),
        device_software_version: env!("CARGO_PKG_VERSION").to_owned(),
        device_type: EnumOrUnknown::new(DeviceType::COMPUTER),
        spirc_version: SPIRC_VERSION.to_owned(),
        device_id: device_id.to_owned(),
        client_id: client_id.to_owned(),
        ..Default::default()
    }
}

/// Librespot's receiver capabilities, with volume disabled.
fn receiver_capabilities() -> Capabilities {
    Capabilities {
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
    }
}

/// The player state librespot resets to: nothing loaded, nothing playing.
fn idle_player_state(session_id: &str) -> PlayerState {
    PlayerState {
        session_id: session_id.to_owned(),
        is_system_initiated: true,
        playback_speed: 1.0,
        play_origin: MessageField::some(PlayOrigin::new()),
        suppressions: MessageField::some(Suppressions::new()),
        options: MessageField::some(ContextPlayerOptions::new()),
        ..Default::default()
    }
}

/// What one cluster says about the account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterView {
    /// The roster, absent when the cluster carried no device map.
    pub devices: Option<Vec<Device>>,
    /// The device holding the sound, when one is named and it is not us.
    pub holder: Option<String>,
    /// Whether the cluster names this client as the one holding the sound.
    ///
    /// `holder` cannot answer this: it is `None` both for "nobody" and for us.
    pub held_here: bool,
    /// The cluster's own state-change instant, in unix milliseconds.
    pub changed_ms: i64,
    pub playback: Option<RemotePlayback>,
}

/// One answer from the cluster: a view, or a push that could not be read.
///
/// Boxed because the roster dwarfs the failure and the channel moves one at a
/// time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClusterEvent {
    View(Box<ClusterView>),
    Failed,
    /// The sound was taken away, naming the activation that ended.
    Lost {
        epoch: u64,
    },
}

/// Why a connect call failed.
#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("{0}")]
    Session(#[from] librespot::core::Error),
    #[error("no connection id yet")]
    NoConnectionId,
    #[error("could not parse a cluster: {0}")]
    Cluster(#[from] protobuf::Error),
    #[error("the devices are already being watched")]
    AlreadyWatching,
    #[error("the device has been withdrawn")]
    Withdrawn,
    #[error("the wire stayed busy")]
    Busy,
    #[error("a device id cannot be a path segment")]
    InvalidTarget,
    #[error("the dealer cannot be started again")]
    DealerSpent,
    #[error("the withdraw did not finish in time")]
    WithdrawTimedOut,
    #[error("the state put did not answer in time")]
    PutTimedOut,
    #[error("the request expired before it could be answered")]
    Expired,
    #[error("the landing has no track to play")]
    NoTrack,
    #[error("the device cannot play")]
    NotPlayable,
}

impl ConnectError {
    /// The one word a log may carry. A hand-over that fails silently is the
    /// hardest failure to read, and the wire's own error text is not ours to log.
    pub fn word(&self) -> &'static str {
        match self {
            Self::Session(_) => "session",
            Self::NoConnectionId => "no connection id",
            Self::Cluster(_) => "cluster parse",
            Self::AlreadyWatching => "already watching",
            Self::Withdrawn => "withdrawn",
            Self::Busy => "wire busy",
            Self::InvalidTarget => "invalid target",
            Self::DealerSpent => "dealer spent",
            Self::WithdrawTimedOut => "withdraw timed out",
            Self::PutTimedOut => "put timed out",
            Self::Expired => "request expired",
            Self::NoTrack => "no track",
            Self::NotPlayable => "not playable",
        }
    }
}

/// A transport action sent to the device that holds the sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Steer {
    Pause,
    Resume,
    SkipNext,
    SkipPrev,
    SeekTo(u32),
}

/// The JSON body the command endpoint expects for one steer.
pub fn steer_body(steer: &Steer) -> String {
    match steer {
        Steer::Pause => r#"{"command":{"endpoint":"pause"}}"#.to_owned(),
        Steer::Resume => r#"{"command":{"endpoint":"resume"}}"#.to_owned(),
        Steer::SkipNext => r#"{"command":{"endpoint":"skip_next"}}"#.to_owned(),
        Steer::SkipPrev => r#"{"command":{"endpoint":"skip_prev"}}"#.to_owned(),
        Steer::SeekTo(position_ms) => format!(
            r#"{{"command":{{"endpoint":"seek_to","value":{position_ms},"position":{position_ms}}}}}"#
        ),
    }
}

/// Whether a lost reply makes it safe to send the same steer again.
///
/// A seek, pause or resume names the state it wants, so a duplicate lands where
/// it already is; a skip is relative and a duplicate skips a second track.
pub fn repeatable(steer: &Steer) -> bool {
    matches!(steer, Steer::Pause | Steer::Resume | Steer::SeekTo(_))
}

/// Reads one cluster, leaving out this client and anything it cannot pick.
pub fn view_of(cluster: &Cluster, own_id: &str) -> ClusterView {
    ClusterView {
        devices: roster(cluster, own_id),
        holder: holder(cluster, own_id),
        held_here: held_here(cluster, own_id),
        changed_ms: cluster.changed_timestamp_ms,
        playback: playback(cluster),
    }
}

/// Reads a pushed update, when it carries a cluster at all.
pub fn view_of_update(update: &ClusterUpdate, own_id: &str) -> Option<ClusterView> {
    update
        .cluster
        .as_ref()
        .map(|cluster| view_of(cluster, own_id))
}

fn roster(cluster: &Cluster, own_id: &str) -> Option<Vec<Device>> {
    if cluster.device.is_empty() {
        return None;
    }
    let mut devices: Vec<Device> = cluster
        .device
        .iter()
        .filter(|(id, _)| id.as_str() != own_id)
        .filter_map(|(id, info)| {
            let capabilities = info.capabilities.as_ref()?;
            if !capabilities.can_be_player || capabilities.hidden || info.is_offline {
                return None;
            }
            Some(Device {
                id: id.clone(),
                name: info.name.clone(),
                kind: kind_of(info.device_type.enum_value_or_default()),
            })
        })
        .collect();
    devices.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    Some(devices)
}

fn kind_of(device_type: DeviceType) -> DeviceKind {
    match device_type {
        DeviceType::COMPUTER => DeviceKind::Computer,
        DeviceType::TABLET => DeviceKind::Tablet,
        DeviceType::SMARTPHONE => DeviceKind::Phone,
        DeviceType::SPEAKER => DeviceKind::Speaker,
        DeviceType::TV => DeviceKind::Tv,
        DeviceType::AUTOMOBILE => DeviceKind::Car,
        _ => DeviceKind::Other,
    }
}

fn held_here(cluster: &Cluster, own_id: &str) -> bool {
    let id = cluster.active_device_id.as_str();
    !id.is_empty() && id == own_id
}

fn holder(cluster: &Cluster, own_id: &str) -> Option<String> {
    let id = cluster.active_device_id.as_str();
    if id.is_empty() || id == own_id {
        return None;
    }
    Some(id.to_owned())
}

fn playback(cluster: &Cluster) -> Option<RemotePlayback> {
    let player = cluster.player_state.as_ref()?;
    let track = player.track.as_ref()?;
    if track.uri.is_empty() {
        return None;
    }
    let state = state_of(player);
    let duration = duration_of(player);
    Some(RemotePlayback {
        track: placeholder(track),
        state,
        position_ms: position_of(cluster, player, state, duration),
        duration_ms: duration,
        steerable: player
            .restrictions
            .as_ref()
            .is_none_or(|restrictions| restrictions.disallow_remote_control_reasons.is_empty()),
    })
}

/// The one place the three wire flags become a state.
///
/// A pause arrives as `(is_playing, is_paused, is_buffering) = (true, true,
/// false)`, so paused is read before buffering; keying on buffering first would
/// draw a pause as a load.
fn state_of(player: &PlayerState) -> RemoteState {
    if player.is_paused {
        RemoteState::Paused
    } else if player.is_buffering {
        RemoteState::Loading
    } else if player.is_playing {
        RemoteState::Playing
    } else {
        RemoteState::Paused
    }
}

fn duration_of(player: &PlayerState) -> Option<u32> {
    (player.duration > 0).then(|| u32::try_from(player.duration).unwrap_or(u32::MAX))
}

fn position_of(
    cluster: &Cluster,
    player: &PlayerState,
    state: RemoteState,
    duration: Option<u32>,
) -> u32 {
    let base = player.position_as_of_timestamp.max(0);
    let elapsed = if state == RemoteState::Playing {
        let elapsed = elapsed_of(cluster.server_timestamp_ms, player.timestamp);
        match duration {
            Some(duration) => elapsed.min(i64::from(duration)),
            None => elapsed.min(MAX_EXTRAPOLATION_MS),
        }
    } else {
        0
    };
    let position = match duration {
        Some(duration) => base.saturating_add(elapsed).min(i64::from(duration)),
        None => base.saturating_add(elapsed),
    };
    u32::try_from(position).unwrap_or(u32::MAX)
}

/// The server-clock gap between the push and the position it carries.
///
/// Both operands are the server's, so the local clock's skew never enters the
/// arithmetic. A missing instant, a negative one or a push from the future
/// gives no elapsed time rather than a guess.
fn elapsed_of(server_timestamp_ms: i64, player_timestamp: i64) -> i64 {
    if player_timestamp <= 0 || server_timestamp_ms <= 0 {
        return 0;
    }
    server_timestamp_ms.saturating_sub(player_timestamp).max(0)
}

/// The track as the cluster names it, before a label fills in the rest.
fn placeholder(track: &ProvidedTrack) -> TrackEntry {
    TrackEntry {
        uri: track.uri.clone(),
        title: track.uri.clone(),
        artists: String::new(),
        album: None,
        album_uri: (!track.album_uri.is_empty()).then(|| track.album_uri.clone()),
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: false,
        cover: None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use librespot::protocol::connect::{
        Capabilities, Cluster, ClusterUpdate, Device as WireDevice, DeviceInfo, MemberType,
        PutStateReason, PutStateRequest,
    };
    use librespot::protocol::devices::DeviceType;
    use librespot::protocol::media::AudioQuality;
    use librespot::protocol::player::{
        ContextPlayerOptions, PlayOrigin, PlayerState, ProvidedTrack, Restrictions, Suppressions,
    };
    use protobuf::{EnumOrUnknown, Message as _, MessageField};
    use takofy_model::{DeviceKind, Options, RemoteState, Report, Status, TrackEntry};

    use super::*;

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

    fn player(uri: &str, playing: bool, paused: bool, buffering: bool) -> PlayerState {
        PlayerState {
            track: MessageField::some(ProvidedTrack {
                uri: uri.to_owned(),
                ..Default::default()
            }),
            is_playing: playing,
            is_paused: paused,
            is_buffering: buffering,
            ..Default::default()
        }
    }

    fn cluster_with_player(player: PlayerState) -> Cluster {
        Cluster {
            player_state: MessageField::some(player),
            ..Default::default()
        }
    }

    fn playback_at(position: i64, timestamp: i64, server: i64, duration: i64) -> RemotePlayback {
        let mut player = player("spotify:track:1", true, false, false);
        player.position_as_of_timestamp = position;
        player.timestamp = timestamp;
        player.duration = duration;
        let mut cluster = cluster_with_player(player);
        cluster.server_timestamp_ms = server;
        view_of(&cluster, "own")
            .playback
            .expect("a view of a playing cluster has playback")
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
            options: Options::default(),
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
            ConnectError::Expired,
            ConnectError::NoTrack,
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

    fn rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
        let entries = std::fs::read_dir(dir).expect("a crate sources directory");
        for entry in entries {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                rust_files(&path, files);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }

    #[test]
    fn only_one_module_speaks_the_device_wire() {
        let put_state = concat!("put_connect_", "state");
        let delete_state = concat!("delete_connect_", "state");
        let state_path = concat!("connect-state/v1/", "devices");
        let transfer = concat!(".", "transfer(");
        let transfer_path = concat!("connect/", "transfer");
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let skipped = crates.join("takofy-core").join("src").join("watch.rs");

        let mut files = Vec::new();
        for entry in std::fs::read_dir(&crates).expect("the crates directory") {
            let crate_dir = entry.expect("a crate directory").path();
            for root in ["src", "tests", "examples", "benches"] {
                let source = crate_dir.join(root);
                if source.is_dir() {
                    rust_files(&source, &mut files);
                }
            }
        }

        let mut scanned = 0;
        for path in files {
            if path == skipped {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("a readable source file");
            scanned += 1;
            for (pattern, reason) in [
                (put_state, "may send its own device state"),
                (delete_state, "may delete the account's device"),
                (state_path, "may write a device state directly"),
                (transfer, "may move playback itself"),
                (transfer_path, "may move playback itself"),
            ] {
                assert!(!source.contains(pattern), "{} {reason}", path.display());
            }
        }

        assert!(
            scanned > 20,
            "scanned only {scanned} files, so the walk missed the crates"
        );
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

    #[test]
    fn playback_needs_a_track_uri() {
        let mut no_track = player("spotify:track:1", true, false, false);
        no_track.track = MessageField::none();
        assert!(
            view_of(&cluster_with_player(no_track), "own")
                .playback
                .is_none()
        );

        assert!(
            view_of(&cluster_with_player(player("", true, false, false)), "own")
                .playback
                .is_none()
        );
    }

    #[test]
    fn the_measured_triples_decide_the_state() {
        let cases = [
            (true, false, false, RemoteState::Playing),
            (true, true, false, RemoteState::Paused),
            (true, true, true, RemoteState::Paused),
            (true, false, true, RemoteState::Loading),
            (false, false, false, RemoteState::Paused),
        ];

        for (playing, paused, buffering, expected) in cases {
            let cluster =
                cluster_with_player(player("spotify:track:1", playing, paused, buffering));
            let playback = view_of(&cluster, "own")
                .playback
                .expect("a track is loaded");
            assert_eq!(
                playback.state, expected,
                "playing={playing} paused={paused} buffering={buffering}"
            );
        }
    }

    #[test]
    fn position_advances_on_the_server_clock() {
        assert_eq!(playback_at(10_000, 1_000, 2_500, 0).position_ms, 11_500);
    }

    #[test]
    fn the_bare_position_field_is_ignored() {
        let mut player = player("spotify:track:1", true, false, false);
        player.position = 99_999;
        player.position_as_of_timestamp = 0;
        player.timestamp = 0;

        let playback = view_of(&cluster_with_player(player), "own")
            .playback
            .expect("a track is loaded");

        assert_eq!(playback.position_ms, 0);
    }

    #[test]
    fn a_negative_position_reads_as_zero() {
        assert_eq!(playback_at(-5_000, 0, 0, 0).position_ms, 0);
    }

    #[test]
    fn elapsed_needs_both_server_instants() {
        assert_eq!(playback_at(1_000, 0, 5_000, 0).position_ms, 1_000);
        assert_eq!(playback_at(1_000, -10, 5_000, 0).position_ms, 1_000);
        assert_eq!(playback_at(1_000, 1_000, 0, 0).position_ms, 1_000);
        assert_eq!(playback_at(1_000, 1_000, -10, 0).position_ms, 1_000);
    }

    #[test]
    fn elapsed_is_zero_when_the_server_is_behind_the_player() {
        assert_eq!(playback_at(1_000, 5_000, 4_000, 0).position_ms, 1_000);
    }

    #[test]
    fn a_three_minute_old_anchor_advances_three_minutes() {
        let playback = playback_at(0, 1_000, 1_000 + 180_000, 0);

        assert_eq!(playback.position_ms, 180_000);
    }

    #[test]
    fn a_missing_duration_still_has_a_generous_ceiling() {
        let playback = playback_at(0, 1_000, 1_000 + MAX_EXTRAPOLATION_MS * 10, 0);

        assert_eq!(playback.position_ms, MAX_EXTRAPOLATION_MS as u32);
    }

    #[test]
    fn a_known_duration_wins_over_the_ceiling() {
        let playback = playback_at(0, 1_000, 1_000 + MAX_EXTRAPOLATION_MS * 10, 240_000);

        assert_eq!(playback.position_ms, 240_000);
        assert_eq!(playback.duration_ms, Some(240_000));

        let long = playback_at(0, 1_000, 1_000 + MAX_EXTRAPOLATION_MS * 10, 72_000_000);
        assert_eq!(long.position_ms, 72_000_000);
    }

    #[test]
    fn position_saturates_at_the_u32_ceiling() {
        assert_eq!(
            playback_at(i64::from(u32::MAX), 0, 0, 0).position_ms,
            u32::MAX
        );
        assert_eq!(
            playback_at(i64::from(u32::MAX) - 10, 1_000, 5_000, 0).position_ms,
            u32::MAX
        );
    }

    #[test]
    fn a_duration_must_be_positive() {
        assert_eq!(playback_at(0, 0, 0, 0).duration_ms, None);
        assert_eq!(playback_at(0, 0, 0, -1).duration_ms, None);
        assert_eq!(playback_at(0, 0, 0, 30_000).duration_ms, Some(30_000));
    }

    #[test]
    fn position_never_passes_the_duration() {
        assert_eq!(playback_at(50_000, 0, 0, 30_000).position_ms, 30_000);
        assert_eq!(
            playback_at(10_000, 1_000, 11_000, 15_000).position_ms,
            15_000
        );
        assert_eq!(
            playback_at(10_000, 1_000, 11_000, 15_000).duration_ms,
            Some(15_000)
        );
    }

    #[test]
    fn steerable_follows_the_remote_control_reasons() {
        let mut unrestricted = player("spotify:track:1", true, false, false);
        unrestricted.restrictions = MessageField::none();
        assert!(
            view_of(&cluster_with_player(unrestricted), "own")
                .playback
                .expect("a track is loaded")
                .steerable
        );

        let mut allowed = player("spotify:track:1", true, false, false);
        allowed.restrictions = MessageField::some(Restrictions::default());
        assert!(
            view_of(&cluster_with_player(allowed), "own")
                .playback
                .expect("a track is loaded")
                .steerable
        );

        let mut refused = player("spotify:track:1", true, false, false);
        refused.restrictions = MessageField::some(Restrictions {
            disallow_remote_control_reasons: vec!["not-controllable".to_owned()],
            ..Default::default()
        });
        assert!(
            !view_of(&cluster_with_player(refused), "own")
                .playback
                .expect("a track is loaded")
                .steerable
        );
    }

    #[test]
    fn steer_bodies_are_pinned() {
        let cases = [
            (Steer::Pause, r#"{"command":{"endpoint":"pause"}}"#),
            (Steer::Resume, r#"{"command":{"endpoint":"resume"}}"#),
            (Steer::SkipNext, r#"{"command":{"endpoint":"skip_next"}}"#),
            (Steer::SkipPrev, r#"{"command":{"endpoint":"skip_prev"}}"#),
            (
                Steer::SeekTo(60_000),
                r#"{"command":{"endpoint":"seek_to","value":60000,"position":60000}}"#,
            ),
            (
                Steer::SeekTo(0),
                r#"{"command":{"endpoint":"seek_to","value":0,"position":0}}"#,
            ),
        ];

        for (steer, expected) in cases {
            assert_eq!(steer_body(&steer), expected);
        }
    }

    #[test]
    fn repeatable_is_true_only_for_absolute_commands() {
        assert!(repeatable(&Steer::Pause));
        assert!(repeatable(&Steer::Resume));
        assert!(repeatable(&Steer::SeekTo(60_000)));
        assert!(!repeatable(&Steer::SkipNext));
        assert!(!repeatable(&Steer::SkipPrev));
    }

    #[test]
    fn every_body_carries_the_command_envelope() {
        let steers = [
            Steer::Pause,
            Steer::Resume,
            Steer::SkipNext,
            Steer::SkipPrev,
            Steer::SeekTo(0),
            Steer::SeekTo(60_000),
        ];

        for steer in steers {
            let body = steer_body(&steer);
            assert!(body.starts_with(r#"{"command":{"#), "{body}");
            assert!(body.ends_with("}}"), "{body}");
            assert!(!body.starts_with(r#"{"endpoint""#), "{body}");
        }
    }
}
