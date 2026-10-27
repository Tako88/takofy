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
    #[error("the hand-over did not finish in time")]
    HandOverTimedOut,
    #[error("a hand-over is already in flight")]
    HandOverPending,
    #[error("the request expired before it could be answered")]
    Expired,
    #[error("the landing has no track to play")]
    NoTrack,
    #[error("there is no page to hand over")]
    NoContext,
    #[error("nothing is playing here to hand over")]
    NotPlaying,
    #[error("the device cannot play")]
    NotPlayable,
}

impl ConnectError {
    /// The one line a log may carry. A hand-over that fails silently is the
    /// hardest failure to read, and the wire's own error text is not ours to
    /// log — but its kind is a category, and it says which arm of the wire
    /// refused without quoting anything.
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
            Self::HandOverTimedOut => "hand-over timed out",
            Self::HandOverPending => "hand-over pending",
            Self::Expired => "request expired",
            Self::NoTrack => "no track",
            Self::NoContext => "no context",
            Self::NotPlaying => "nothing playing",
            Self::NotPlayable => "not playable",
        }
    }

    /// A log line narrow enough to keep: the word, and for a wire failure the
    /// kind that refused it.
    pub fn log_line(&self) -> String {
        match self {
            Self::Session(error) => format!("{} ({})", self.word(), error.kind),
            _ => self.word().to_owned(),
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
mod tests;
