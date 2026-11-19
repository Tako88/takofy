use super::*;
use serde_json::{Value, json};

fn encoded(command: &Command) -> Value {
    serde_json::from_str(&command.encode()).expect("our own encoder makes JSON")
}

#[test]
fn every_command_encodes_its_documented_shape() {
    assert_eq!(
        encoded(&Command::Play {
            uri: "spotify:track:abc".to_owned()
        }),
        json!({"type": "command", "command": "play", "uri": "spotify:track:abc"})
    );
    assert_eq!(
        encoded(&Command::Resume),
        json!({"type": "command", "command": "play"})
    );
    assert_eq!(
        encoded(&Command::Pause),
        json!({"type": "command", "command": "pause"})
    );
    assert_eq!(
        encoded(&Command::Seek { position_ms: 1234 }),
        json!({"type": "command", "command": "seek", "position_ms": 1234})
    );
    assert_eq!(
        encoded(&Command::SetVolume { volume: 42 }),
        json!({"type": "command", "command": "set_volume", "volume": 42})
    );
    assert_eq!(
        encoded(&Command::SetShuffle { enabled: false }),
        json!({"type": "command", "command": "set_shuffle", "enabled": false})
    );
    assert_eq!(
        encoded(&Command::SetRepeatContext { enabled: true }),
        json!({"type": "command", "command": "set_repeat_context", "enabled": true})
    );
    assert_eq!(
        encoded(&Command::SetRepeatTrack { enabled: false }),
        json!({"type": "command", "command": "set_repeat_track", "enabled": false})
    );
    assert_eq!(
        encoded(&Command::GetState),
        json!({"type": "command", "command": "get_state"})
    );
    assert_eq!(
        encoded(&Command::GetAuthState),
        json!({"type": "command", "command": "get_auth_state"})
    );
    assert_eq!(
        encoded(&Command::Activate),
        json!({"type": "command", "command": "activate"})
    );
}

#[test]
fn volume_never_encodes_above_the_maximum() {
    assert_eq!(
        encoded(&Command::SetVolume { volume: 250 }),
        json!({"type": "command", "command": "set_volume", "volume": 100})
    );
}

#[test]
fn command_results_and_errors_decode() {
    assert_eq!(
        decode(r#"{"type":"command_result","command":"pause"}"#),
        Frame::CommandResult {
            command: "pause".to_owned()
        }
    );
    assert_eq!(
        decode(r#"{"type":"error","message":"command requires authentication"}"#),
        Frame::Error {
            message: "command requires authentication".to_owned()
        }
    );
}

#[test]
fn auth_and_device_frames_decode() {
    assert_eq!(
        decode(r#"{"type":"auth_state","logged_in":true,"is_active":false,"device_name":"T"}"#),
        Frame::AuthState {
            logged_in: true,
            is_active: false,
            device_name: "T".to_owned()
        }
    );
    assert_eq!(
        decode(r#"{"type":"device_changed","is_active":true}"#),
        Frame::DeviceChanged {
            is_active: true,
            device_name: None
        }
    );
}

#[test]
fn a_full_playback_state_decodes() {
    let frame = decode(
        r#"{
            "type": "playback_state",
            "status": "playing",
            "item": {
                "uri": "spotify:track:abc",
                "entity_type": "track",
                "decorations": {"playback": {"duration_ms": 210000}}
            },
            "position": {"position_ms": 45000, "timestamp_ms": 1747654321000, "speed": 1.0},
            "volume": 65,
            "is_active": true,
            "options": {"shuffle": false, "repeat": "context", "playback_speed": 1.0}
        }"#,
    );
    let Frame::PlaybackState(snapshot) = frame else {
        panic!("expected a playback state, got {frame:?}");
    };
    assert_eq!(snapshot.status, Status::Playing);
    assert_eq!(snapshot.item_uri.as_deref(), Some("spotify:track:abc"));
    assert_eq!(snapshot.duration_ms, Some(210_000));
    assert_eq!(
        snapshot.position,
        Some(Anchor {
            position_ms: 45_000,
            timestamp_ms: 1_747_654_321_000,
            speed: 1.0,
        })
    );
    assert_eq!(snapshot.volume, Some(65));
    assert_eq!(snapshot.is_active, Some(true));
    assert_eq!(
        snapshot.options,
        Some(Options {
            shuffle: false,
            repeat: Repeat::Context
        })
    );
}

#[test]
fn granular_frames_decode() {
    assert_eq!(
        decode(
            r#"{"type":"track_changed","item":{"uri":"spotify:track:x","decorations":{"playback":{"duration_ms":1000}}}}"#
        ),
        Frame::TrackChanged {
            uri: "spotify:track:x".to_owned(),
            duration_ms: Some(1000)
        }
    );
    assert_eq!(
        decode(r#"{"type":"playback_changed","status":"buffering"}"#),
        Frame::PlaybackChanged {
            status: Status::Buffering
        }
    );
    assert_eq!(
        decode(r#"{"type":"volume_changed","volume":42}"#),
        Frame::VolumeChanged { volume: 42 }
    );
    assert_eq!(
        decode(
            r#"{"type":"position_sync","position":{"position_ms":5,"timestamp_ms":10,"speed":0.0}}"#
        ),
        Frame::PositionSync(Anchor {
            position_ms: 5,
            timestamp_ms: 10,
            speed: 0.0
        })
    );
    assert_eq!(
        decode(r#"{"type":"context_changed","context":{"uri":"spotify:album:a"}}"#),
        Frame::ContextChanged {
            uri: "spotify:album:a".to_owned()
        }
    );
    assert_eq!(decode(r#"{"type":"queue_changed"}"#), Frame::QueueChanged);
    assert_eq!(
        decode(r#"{"type":"options_changed","options":{"shuffle":true,"repeat":"track"}}"#),
        Frame::OptionsChanged {
            options: Options {
                shuffle: true,
                repeat: Repeat::Track
            }
        }
    );
}

#[test]
fn unknown_types_and_extra_fields_are_tolerated() {
    assert_eq!(decode(r#"{"type":"something_new","x":1}"#), Frame::Unknown);
    assert_eq!(decode(r#"{"type":"pause_ack"}"#), Frame::Unknown);
    assert_eq!(decode(r#"{"type":"pause"}"#), Frame::Unknown);
    assert_eq!(
        decode(r#"{"type":"volume_changed","volume":7,"extra":{"deep":true}}"#),
        Frame::VolumeChanged { volume: 7 }
    );
}

#[test]
fn malformed_json_is_never_a_panic() {
    for text in ["", "not json", "{", "[]", "null", r#"{"no_type":1}"#] {
        assert_eq!(decode(text), Frame::Unknown, "{text:?}");
    }
}

#[test]
fn a_status_outside_the_documented_set_is_other() {
    assert_eq!(
        decode(r#"{"type":"playback_changed","status":"mystery"}"#),
        Frame::PlaybackChanged {
            status: Status::Other
        }
    );
}
