use super::*;
use crate::playback::{PlaybackError, PlaybackEvent};
use serde_json::Value;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

struct Harness {
    sound: Box<dyn Sound>,
    events: UnboundedReceiver<PlaybackEvent>,
    to_daemon: UnboundedReceiver<String>,
    from_daemon: Option<UnboundedSender<String>>,
}

impl Harness {
    fn new(volume: u16) -> Self {
        let (outgoing, to_daemon) = unbounded_channel();
        let (from_daemon, incoming) = unbounded_channel();
        let (sound, events) = SoloistSound::start(Connection::new(outgoing, incoming), volume);
        Self {
            sound: Box::new(sound),
            events,
            to_daemon,
            from_daemon: Some(from_daemon),
        }
    }

    async fn command(&mut self) -> Value {
        let text = self.to_daemon.recv().await.expect("a command");
        serde_json::from_str(&text).expect("command JSON")
    }

    async fn expect_command(&mut self, name: &str) -> Value {
        let value = self.command().await;
        assert_eq!(value["command"], name, "expected {name}, got {value}");
        value
    }

    fn send(&self, text: &str) {
        self.from_daemon
            .as_ref()
            .expect("socket open")
            .send(text.to_owned())
            .expect("frame sent");
    }

    async fn event(&mut self) -> PlaybackEvent {
        self.events.recv().await.expect("an event")
    }

    async fn loading(&mut self) {
        assert_eq!(self.event().await, PlaybackEvent::Loading);
    }

    async fn drive_to_play(&mut self) {
        self.expect_command("activate").await;
        self.send(r#"{"type":"command_result","command":"activate"}"#);
        self.expect_command("set_shuffle").await;
        self.send(r#"{"type":"command_result","command":"set_shuffle"}"#);
        self.expect_command("set_repeat_track").await;
        self.send(r#"{"type":"command_result","command":"set_repeat_track"}"#);
        self.expect_command("set_repeat_context").await;
        self.send(r#"{"type":"command_result","command":"set_repeat_context"}"#);
        self.expect_command("play").await;
    }
}

const TRACK: &str = "spotify:track:2JRo0gjbX4GrCqBYdRohoo";

#[tokio::test(start_paused = true)]
async fn a_load_reports_loading_playing_duration_and_ended() {
    let mut harness = Harness::new(50);
    let started = harness.expect_command("set_volume").await;
    assert_eq!(started["volume"], 50);

    harness.sound.load(TRACK, 0, true).expect("valid track");
    harness.drive_to_play().await;
    harness.send(r#"{"type":"command_result","command":"play"}"#);
    harness.loading().await;

    harness.send(
        r#"{"type":"playback_state","status":"playing","item":{"uri":"spotify:track:2JRo0gjbX4GrCqBYdRohoo","entity_type":"track","decorations":{"playback":{"duration_ms":200000}}},"position":{"position_ms":199000,"timestamp_ms":1,"speed":1.0}}"#,
    );
    assert_eq!(
        harness.event().await,
        PlaybackEvent::Duration {
            duration_ms: 200_000
        }
    );
    assert_eq!(
        harness.event().await,
        PlaybackEvent::Playing {
            position_ms: 199_000
        }
    );

    harness.send(r#"{"type":"playback_changed","status":"idle"}"#);
    assert_eq!(harness.event().await, PlaybackEvent::Ended);
}

#[tokio::test(start_paused = true)]
async fn a_positioned_non_playing_load_seeks_then_pauses() {
    let mut harness = Harness::new(70);
    let started = harness.expect_command("set_volume").await;
    assert_eq!(started["volume"], 70);

    harness
        .sound
        .load(TRACK, 60_000, false)
        .expect("valid track");
    harness.drive_to_play().await;
    harness.send(r#"{"type":"command_result","command":"play"}"#);
    harness.loading().await;

    harness.send(
        r#"{"type":"playback_state","status":"paused","item":{"uri":"spotify:track:2JRo0gjbX4GrCqBYdRohoo","entity_type":"track","decorations":{"playback":{"duration_ms":210000}}},"position":{"position_ms":60000,"timestamp_ms":1,"speed":0.0}}"#,
    );
    assert_eq!(
        harness.event().await,
        PlaybackEvent::Duration {
            duration_ms: 210_000
        }
    );
    assert_eq!(
        harness.event().await,
        PlaybackEvent::Paused {
            position_ms: 60_000
        }
    );

    let seek = harness.expect_command("seek").await;
    assert_eq!(seek["position_ms"], 60_000);
    harness.send(r#"{"type":"command_result","command":"seek"}"#);
    harness.send(
        r#"{"type":"position_sync","position":{"position_ms":60000,"timestamp_ms":2,"speed":0.0}}"#,
    );
    harness.expect_command("pause").await;
    harness.send(r#"{"type":"command_result","command":"pause"}"#);
}

#[tokio::test(start_paused = true)]
async fn silence_becomes_unavailable_after_the_load_budget() {
    let mut harness = Harness::new(50);
    harness.expect_command("set_volume").await;
    harness.sound.load(TRACK, 0, true).expect("valid track");
    harness.drive_to_play().await;
    harness.send(r#"{"type":"command_result","command":"play"}"#);
    harness.loading().await;

    tokio::time::advance(super::super::ending::LOAD_BUDGET).await;
    assert_eq!(harness.event().await, PlaybackEvent::Unavailable);
}

#[tokio::test(start_paused = true)]
async fn an_error_during_play_becomes_unavailable() {
    let mut harness = Harness::new(50);
    harness.expect_command("set_volume").await;
    harness.sound.load(TRACK, 0, true).expect("valid track");
    harness.drive_to_play().await;
    harness.send(r#"{"type":"error","message":"command requires authentication"}"#);
    assert_eq!(harness.event().await, PlaybackEvent::Unavailable);
}

#[tokio::test(start_paused = true)]
async fn closing_the_socket_closes_the_event_stream() {
    let mut harness = Harness::new(50);
    harness.expect_command("set_volume").await;
    harness.from_daemon.take().expect("socket open");
    assert!(harness.events.recv().await.is_none());
}

#[tokio::test(start_paused = true)]
async fn set_volume_is_clamped_to_the_maximum() {
    let mut harness = Harness::new(10);
    harness.expect_command("set_volume").await;
    harness.sound.set_volume(250);
    let command = harness.expect_command("set_volume").await;
    assert_eq!(command["volume"], 100);
}

#[tokio::test(start_paused = true)]
async fn stop_reports_stopped_and_pauses_the_daemon() {
    let mut harness = Harness::new(50);
    harness.expect_command("set_volume").await;
    harness.sound.stop();
    assert_eq!(harness.event().await, PlaybackEvent::Stopped);
    harness.expect_command("pause").await;
}

#[tokio::test(start_paused = true)]
async fn an_unplayable_uri_fails_synchronously() {
    let harness = Harness::new(50);
    assert!(matches!(
        harness.sound.load("spotify:album:not-a-track", 0, true),
        Err(PlaybackError::NotPlayable(_))
    ));
}

#[tokio::test(start_paused = true)]
async fn a_second_load_supersedes_the_first() {
    let mut harness = Harness::new(50);
    harness.expect_command("set_volume").await;
    harness.sound.load(TRACK, 0, true).expect("valid track");
    harness.drive_to_play().await;

    harness
        .sound
        .load("spotify:track:4uLU6hMCjMI75M1A2tKUQC", 0, true)
        .expect("valid track");
    let play = harness.expect_command("play").await;
    assert_eq!(play["uri"], "spotify:track:4uLU6hMCjMI75M1A2tKUQC");

    harness.sound.stop();
    assert_eq!(
        harness.events.recv().await.expect("an event"),
        PlaybackEvent::Stopped
    );
    harness.expect_command("pause").await;
}

#[tokio::test(start_paused = true)]
async fn a_superseded_plays_error_does_not_mark_the_new_load_unavailable() {
    let mut harness = Harness::new(50);
    harness.expect_command("set_volume").await;
    harness.sound.load(TRACK, 0, true).expect("valid track");
    harness.drive_to_play().await;

    harness
        .sound
        .load("spotify:track:4uLU6hMCjMI75M1A2tKUQC", 0, true)
        .expect("valid track");
    harness.expect_command("play").await;

    // The refused play of the superseded load arrives after the new one went out.
    harness.send(r#"{"type":"error","message":"track is not playable"}"#);
    harness.send(r#"{"type":"command_result","command":"play"}"#);
    harness.loading().await;

    harness.send(
        r#"{"type":"playback_state","status":"playing","item":{"uri":"spotify:track:4uLU6hMCjMI75M1A2tKUQC","entity_type":"track","decorations":{"playback":{"duration_ms":180000}}},"position":{"position_ms":0,"timestamp_ms":1,"speed":1.0}}"#,
    );
    assert_eq!(
        harness.event().await,
        PlaybackEvent::Duration {
            duration_ms: 180_000
        }
    );
    assert!(matches!(
        harness.event().await,
        PlaybackEvent::Playing { .. }
    ));
}

#[tokio::test(start_paused = true)]
async fn shutdown_ends_the_task() {
    let (outgoing, _to_daemon) = unbounded_channel();
    let (_from_daemon, incoming) = unbounded_channel();
    let (sound, _events) = SoloistSound::start(Connection::new(outgoing, incoming), 50);
    let sound: Box<dyn Sound> = Box::new(sound);
    sound.shutdown().await.expect("clean shutdown");
}

#[tokio::test(start_paused = true)]
async fn no_event_arrives_after_a_shutdown() {
    let (outgoing, _to_daemon) = unbounded_channel();
    let (_from_daemon, incoming) = unbounded_channel();
    let (sound, mut events) = SoloistSound::start(Connection::new(outgoing, incoming), 50);
    let sound: Box<dyn Sound> = Box::new(sound);
    sound.shutdown().await.expect("clean shutdown");
    assert!(events.recv().await.is_none());
}
