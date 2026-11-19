//! The JSON wire between this crate and Soloist's WebSocket.
//!
//! Encoding is a whitelist of the control commands this client needs. Queue
//! mirroring commands are absent on purpose: Takofy's queue stays in charge.
//! Decoding tolerates unknown types and extra fields, and never panics.

use serde_json::{Value, json};

use super::position::Anchor;

/// One client-to-server command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Play { uri: String },
    Resume,
    Pause,
    Seek { position_ms: u32 },
    SetVolume { volume: u8 },
    SetShuffle { enabled: bool },
    SetRepeatContext { enabled: bool },
    SetRepeatTrack { enabled: bool },
    GetState,
    GetAuthState,
    Activate,
}

impl Command {
    /// The `command` field this command carries.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Play { .. } | Self::Resume => "play",
            Self::Pause => "pause",
            Self::Seek { .. } => "seek",
            Self::SetVolume { .. } => "set_volume",
            Self::SetShuffle { .. } => "set_shuffle",
            Self::SetRepeatContext { .. } => "set_repeat_context",
            Self::SetRepeatTrack { .. } => "set_repeat_track",
            Self::GetState => "get_state",
            Self::GetAuthState => "get_auth_state",
            Self::Activate => "activate",
        }
    }

    /// The text frame for this command.
    pub fn encode(&self) -> String {
        let value = match self {
            Self::Play { uri } => json!({"type": "command", "command": "play", "uri": uri}),
            Self::Resume => json!({"type": "command", "command": "play"}),
            Self::Pause => json!({"type": "command", "command": "pause"}),
            Self::Seek { position_ms } => {
                json!({"type": "command", "command": "seek", "position_ms": position_ms})
            }
            Self::SetVolume { volume } => json!({
                "type": "command",
                "command": "set_volume",
                "volume": (*volume).min(100),
            }),
            Self::SetShuffle { enabled } => {
                json!({"type": "command", "command": "set_shuffle", "enabled": enabled})
            }
            Self::SetRepeatContext { enabled } => json!({
                "type": "command",
                "command": "set_repeat_context",
                "enabled": enabled,
            }),
            Self::SetRepeatTrack { enabled } => json!({
                "type": "command",
                "command": "set_repeat_track",
                "enabled": enabled,
            }),
            Self::GetState => json!({"type": "command", "command": "get_state"}),
            Self::GetAuthState => json!({"type": "command", "command": "get_auth_state"}),
            Self::Activate => json!({"type": "command", "command": "activate"}),
        };
        value.to_string()
    }
}

/// A playback status as Soloist spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Idle,
    Playing,
    Paused,
    Buffering,
    Other,
}

impl Status {
    fn parse(text: &str) -> Self {
        match text {
            "idle" => Self::Idle,
            "playing" => Self::Playing,
            "paused" => Self::Paused,
            "buffering" => Self::Buffering,
            _ => Self::Other,
        }
    }
}

/// Soloist's repeat mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Repeat {
    #[default]
    Off,
    Context,
    Track,
}

impl Repeat {
    fn parse(text: &str) -> Self {
        match text {
            "context" => Self::Context,
            "track" => Self::Track,
            _ => Self::Off,
        }
    }
}

/// The playback options Soloist reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    pub shuffle: bool,
    pub repeat: Repeat,
}

/// One `playback_state` snapshot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Snapshot {
    pub status: Status,
    pub item_uri: Option<String>,
    pub duration_ms: Option<u32>,
    pub position: Option<Anchor>,
    pub volume: Option<u8>,
    pub is_active: Option<bool>,
    pub options: Option<Options>,
}

/// One server-to-client frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    CommandResult {
        command: String,
    },
    Error {
        message: String,
    },
    AuthState {
        logged_in: bool,
        is_active: bool,
        device_name: String,
    },
    PlaybackState(Snapshot),
    TrackChanged {
        uri: String,
        duration_ms: Option<u32>,
    },
    PlaybackChanged {
        status: Status,
    },
    VolumeChanged {
        volume: u8,
    },
    DeviceChanged {
        is_active: bool,
        device_name: Option<String>,
    },
    OptionsChanged {
        options: Options,
    },
    PositionSync(Anchor),
    ContextChanged {
        uri: String,
    },
    QueueChanged,
    Unknown,
}

/// Decodes one text frame, mapping anything unreadable to `Unknown`.
pub fn decode(text: &str) -> Frame {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Frame::Unknown;
    };
    let Some(kind) = value.get("type").and_then(Value::as_str) else {
        return Frame::Unknown;
    };
    match kind {
        "command_result" => Frame::CommandResult {
            command: string(&value, "command").unwrap_or_default(),
        },
        "error" => Frame::Error {
            message: string(&value, "message").unwrap_or_default(),
        },
        "auth_state" => Frame::AuthState {
            logged_in: boolean(&value, "logged_in").unwrap_or(false),
            is_active: boolean(&value, "is_active").unwrap_or(false),
            device_name: string(&value, "device_name").unwrap_or_default(),
        },
        "playback_state" => Frame::PlaybackState(snapshot(&value)),
        "track_changed" => {
            let item = value.get("item");
            Frame::TrackChanged {
                uri: item
                    .and_then(|item| string(item, "uri"))
                    .unwrap_or_default(),
                duration_ms: item.and_then(duration_of),
            }
        }
        "playback_changed" => Frame::PlaybackChanged {
            status: value
                .get("status")
                .and_then(Value::as_str)
                .map(Status::parse)
                .unwrap_or_default(),
        },
        "volume_changed" => Frame::VolumeChanged {
            volume: value
                .get("volume")
                .and_then(Value::as_u64)
                .map(|volume| volume.min(100) as u8)
                .unwrap_or(0),
        },
        "device_changed" => Frame::DeviceChanged {
            is_active: boolean(&value, "is_active").unwrap_or(false),
            device_name: string(&value, "device_name"),
        },
        "options_changed" => Frame::OptionsChanged {
            options: value.get("options").and_then(options).unwrap_or_default(),
        },
        "position_sync" => value
            .get("position")
            .and_then(anchor)
            .map(Frame::PositionSync)
            .unwrap_or(Frame::Unknown),
        "context_changed" => Frame::ContextChanged {
            uri: value
                .get("context")
                .and_then(|context| string(context, "uri"))
                .unwrap_or_default(),
        },
        "queue_changed" => Frame::QueueChanged,
        _ => Frame::Unknown,
    }
}

fn snapshot(value: &Value) -> Snapshot {
    let item = value.get("item");
    Snapshot {
        status: value
            .get("status")
            .and_then(Value::as_str)
            .map(Status::parse)
            .unwrap_or_default(),
        item_uri: item
            .and_then(|item| string(item, "uri"))
            .filter(|uri| !uri.is_empty()),
        duration_ms: item.and_then(duration_of),
        position: value.get("position").and_then(anchor),
        volume: value
            .get("volume")
            .and_then(Value::as_u64)
            .map(|volume| volume.min(100) as u8),
        is_active: value.get("is_active").and_then(Value::as_bool),
        options: value.get("options").and_then(options),
    }
}

fn options(value: &Value) -> Option<Options> {
    Some(Options {
        shuffle: value
            .get("shuffle")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        repeat: value
            .get("repeat")
            .and_then(Value::as_str)
            .map(Repeat::parse)
            .unwrap_or_default(),
    })
}

fn anchor(value: &Value) -> Option<Anchor> {
    Some(Anchor {
        position_ms: value.get("position_ms").and_then(u32_of)?,
        timestamp_ms: value
            .get("timestamp_ms")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        speed: value
            .get("speed")
            .and_then(Value::as_f64)
            .unwrap_or_default(),
    })
}

fn duration_of(item: &Value) -> Option<u32> {
    item.get("decorations")?
        .get("playback")?
        .get("duration_ms")
        .and_then(u32_of)
}

fn u32_of(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .map(|number| number.min(u64::from(u32::MAX)) as u32)
}

fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn boolean(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(Value::as_bool)
}

#[cfg(test)]
mod tests;
