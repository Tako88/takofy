use std::sync::Arc;
use std::sync::Mutex;

use librespot::core::SpotifyUri;
use librespot::metadata::artist::ArtistsWithRole;
use librespot::metadata::audio::{AudioFiles, AudioItem, UniqueFields};
use librespot::playback::player::PlayerEvent;

use librespot::playback::mixer::VolumeGetter;

use super::{
    Outcome, PLAYBACK_LOG_TARGET, PlaybackError, PlaybackEvent, Volume, attenuation, drive,
    duration_of, forward, into_sole, map_event, playable_track,
};

/// The player reads the getter it was built with, so the cell behind it is
/// the whole interface: a clone must see what the other handle sets.
#[test]
fn the_players_handle_reads_what_the_window_sets() {
    let window = Volume::default();
    let player = window.clone();
    assert_eq!(player.attenuation_factor(), 1.0, "full volume by default");

    window.set(0);
    assert_eq!(player.attenuation_factor(), 0.0, "zero is silence");

    window.set(60);
    let quieter = player.attenuation_factor();
    assert!(
        (0.0..1.0).contains(&quieter),
        "60% is between silent and full, not {quieter}"
    );
}

/// librespot's curve is `Log(60 dB)`, so the gain is 10^(-3(1 - level)):
/// quiet enough at the bottom to be usable all the way down, instead of the
/// first fifth of the travel doing nothing audible.
#[test]
fn the_level_follows_the_log_curve() {
    for percent in [1u16, 25, 60, 99] {
        let level = f64::from(percent) / 100.0;
        let expected = 10f64.powf(-3.0 * (1.0 - level));
        let got = attenuation(percent);
        assert!(
            (got - expected).abs() < 1e-3,
            "{percent}% is {got}, not the log curve's {expected}"
        );
    }
}

#[test]
fn a_lower_level_is_quieter_and_above_full_is_clamped() {
    let volume = Volume::at(50);
    let half = volume.attenuation_factor();
    volume.set(20);
    assert!(volume.attenuation_factor() < half, "less is quieter");

    volume.set(100);
    assert_eq!(volume.attenuation_factor(), 1.0);
    volume.set(150);
    assert_eq!(
        volume.attenuation_factor(),
        1.0,
        "a level past full is never louder than the source"
    );
}

fn track() -> SpotifyUri {
    SpotifyUri::from_uri("spotify:track:4uLU6hMCjMI75M1A2tKUQC").expect("valid track uri")
}

#[test]
fn playable_track_table() {
    let track = "spotify:track:4uLU6hMCjMI75M1A2tKUQC";
    let episode = "spotify:episode:6rqhFgbbKwnb9MLmUQDhG6";
    assert!(matches!(
        playable_track(track),
        Ok(SpotifyUri::Track { .. })
    ));
    assert!(matches!(
        playable_track(episode),
        Ok(SpotifyUri::Episode { .. })
    ));

    for refused in [
        // A local file is the user's own disk, not a stream.
        "spotify:local:Artist:Album:Title:222",
        "spotify:artist:4uLU6hMCjMI75M1A2tKUQC",
        "spotify:album:4uLU6hMCjMI75M1A2tKUQC",
        "not a uri at all",
        "spotify:track:short",
    ] {
        assert!(
            matches!(playable_track(refused), Err(PlaybackError::NotPlayable(_))),
            "{refused} is not a playable track"
        );
    }
}

#[test]
fn loading_maps() {
    let event = PlayerEvent::Loading {
        play_request_id: 0,
        track_id: track(),
        position_ms: 0,
    };
    assert_eq!(map_event(&event), Some(PlaybackEvent::Loading));
}

#[test]
fn playing_maps_with_position() {
    let event = PlayerEvent::Playing {
        play_request_id: 0,
        track_id: track(),
        position_ms: 1234,
    };
    assert_eq!(
        map_event(&event),
        Some(PlaybackEvent::Playing { position_ms: 1234 })
    );
}

#[test]
fn paused_maps_with_position() {
    let event = PlayerEvent::Paused {
        play_request_id: 0,
        track_id: track(),
        position_ms: 4321,
    };
    assert_eq!(
        map_event(&event),
        Some(PlaybackEvent::Paused { position_ms: 4321 })
    );
}

#[test]
fn ended_maps() {
    let event = PlayerEvent::EndOfTrack {
        play_request_id: 0,
        track_id: track(),
    };
    assert_eq!(map_event(&event), Some(PlaybackEvent::Ended));
}

#[test]
fn unavailable_maps() {
    let event = PlayerEvent::Unavailable {
        play_request_id: 0,
        track_id: track(),
    };
    assert_eq!(map_event(&event), Some(PlaybackEvent::Unavailable));
}

#[test]
fn stopped_maps() {
    let event = PlayerEvent::Stopped {
        play_request_id: 0,
        track_id: track(),
    };
    assert_eq!(map_event(&event), Some(PlaybackEvent::Stopped));
}

/// An item with only the fields a test needs; a real one comes from the
/// network, and a test must not need the network to name a duration.
fn audio_item(duration_ms: u32) -> AudioItem {
    AudioItem {
        track_id: track(),
        uri: "spotify:track:4uLU6hMCjMI75M1A2tKUQC".to_owned(),
        files: AudioFiles::default(),
        name: "A title".to_owned(),
        covers: Vec::new(),
        language: Vec::new(),
        duration_ms,
        is_explicit: false,
        availability: Ok(()),
        alternatives: None,
        unique_fields: UniqueFields::Track {
            artists: ArtistsWithRole::default(),
            album: String::new(),
            album_artists: Vec::new(),
            popularity: 0,
            number: 0,
            disc_number: 0,
        },
    }
}

#[test]
fn track_changed_maps_to_duration() {
    let event = PlayerEvent::TrackChanged {
        audio_item: Box::new(audio_item(222_000)),
    };
    assert_eq!(
        map_event(&event),
        Some(PlaybackEvent::Duration {
            duration_ms: 222_000
        }),
    );
}

#[test]
fn seeked_maps_with_position() {
    let event = PlayerEvent::Seeked {
        play_request_id: 0,
        track_id: track(),
        position_ms: 41_000,
    };
    assert_eq!(
        map_event(&event),
        Some(PlaybackEvent::Seeked {
            position_ms: 41_000
        }),
    );
}

#[test]
fn duration_and_seeked_are_not_terminal() {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tx.send(PlaybackEvent::Duration {
        duration_ms: 222_000,
    })
    .expect("send duration");
    tx.send(PlaybackEvent::Seeked {
        position_ms: 41_000,
    })
    .expect("send seeked");
    // `drive` runs until the channel closes; without dropping the sender this
    // waits for a close that never comes, and the test hangs rather than fails.
    drop(tx);
    let mut seen = Vec::new();
    let outcome = drive(rx, |event| seen.push(*event));
    assert_eq!(
        seen,
        vec![
            PlaybackEvent::Duration {
                duration_ms: 222_000
            },
            PlaybackEvent::Seeked {
                position_ms: 41_000
            },
        ],
        "both are reported",
    );
    assert!(
        outcome.is_err(),
        "the drain ends only when the player's channel closes",
    );
}

#[test]
fn duration_of_reads_the_audio_item() {
    assert_eq!(duration_of(&audio_item(180_000)), 180_000);
}

#[test]
fn unrelated_events_are_ignored() {
    assert_eq!(map_event(&PlayerEvent::VolumeChanged { volume: 50 }), None);
    assert_eq!(
        map_event(&PlayerEvent::SessionConnected {
            connection_id: "connection".to_owned(),
            user_name: "user".to_owned(),
        }),
        None
    );
    assert_eq!(
        map_event(&PlayerEvent::PlayRequestIdChanged { play_request_id: 7 }),
        None
    );
}

#[test]
fn closed_channel_without_terminal_event_is_an_error() {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    drop(tx);
    let error = drive(rx, |_| {}).expect_err("closed channel is an error");
    assert!(matches!(error, PlaybackError::PlayerClosed));
}

#[test]
fn ended_is_a_terminal_outcome() {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tx.send(PlaybackEvent::Ended).expect("send");
    assert_eq!(drive(rx, |_| {}).expect("outcome"), Outcome::Ended);
}

#[test]
fn unavailable_is_a_terminal_outcome() {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tx.send(PlaybackEvent::Unavailable).expect("send");
    assert_eq!(drive(rx, |_| {}).expect("outcome"), Outcome::Unavailable);
}

#[test]
fn stopped_is_a_terminal_outcome() {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tx.send(PlaybackEvent::Stopped).expect("send");
    assert_eq!(drive(rx, |_| {}).expect("outcome"), Outcome::Stopped);
}

#[test]
fn non_terminal_events_are_reported_before_the_terminal_one() {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tx.send(PlaybackEvent::Loading).expect("send loading");
    tx.send(PlaybackEvent::Playing { position_ms: 5 })
        .expect("send playing");
    tx.send(PlaybackEvent::Ended).expect("send ended");
    let mut seen = Vec::new();
    let outcome = drive(rx, |event| seen.push(*event)).expect("outcome");
    assert_eq!(outcome, Outcome::Ended);
    assert_eq!(
        seen,
        vec![
            PlaybackEvent::Loading,
            PlaybackEvent::Playing { position_ms: 5 },
            PlaybackEvent::Ended,
        ]
    );
}

#[test]
fn into_sole_returns_the_value_when_sole_owner() {
    let arc = Arc::new(7_u32);
    assert_eq!(into_sole(arc).ok(), Some(7));
}

#[test]
fn into_sole_returns_the_arc_when_shared() {
    let arc = Arc::new(7_u32);
    let clone = Arc::clone(&arc);
    let returned = into_sole(arc).expect_err("shared arc cannot be unwrapped");
    assert!(Arc::ptr_eq(&returned, &clone));
}

/// The message an unavailable track logs; the test spells it out so a reword
/// is seen.
const SKIPPED: &str = "a track could not be loaded and is unavailable";

/// The lines this module logged, so a test can count them.
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Recorder;

impl log::Log for Recorder {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record<'_>) {
        if record.target() != PLAYBACK_LOG_TARGET || !self.enabled(record.metadata()) {
            return;
        }
        LOGGED.lock().expect("lock").push(record.args().to_string());
    }

    fn flush(&self) {}
}

/// Begins recording this module's lines, once for the whole test binary.
fn record_logs() {
    static RECORDER: Recorder = Recorder;
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        if log::set_logger(&RECORDER).is_ok() {
            log::set_max_level(log::LevelFilter::Warn);
        }
    });
}

fn unavailable(play_request_id: u64, id: &str) -> PlayerEvent {
    PlayerEvent::Unavailable {
        play_request_id,
        track_id: SpotifyUri::from_uri(&format!("spotify:track:{id}")).expect("valid track"),
    }
}

/// One line per unavailable event, and the line names no track.
#[tokio::test]
async fn each_unavailable_event_logs_one_line_that_names_no_track() {
    record_logs();
    LOGGED.lock().expect("lock").clear();

    let ids = ["4uLU6hMCjMI75M1A2tKUQC", "6rqhFgbbKwnb9MLmUQDhG6"];
    let (raw_tx, raw) = tokio::sync::mpsc::unbounded_channel();
    for (index, id) in ids.iter().enumerate() {
        raw_tx
            .send(unavailable(index as u64, id))
            .expect("raw send");
    }
    drop(raw_tx);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    forward(raw, tx).await;

    let lines = LOGGED.lock().expect("lock").clone();
    assert_eq!(lines.len(), 2, "one line per event, got {lines:?}");
    for line in &lines {
        assert_eq!(line, SKIPPED);
        assert!(!line.contains("spotify"), "must not name a uri: {line}");
        for id in ids {
            assert!(!line.contains(id), "must not name the track: {line}");
        }
    }

    assert!(matches!(rx.try_recv(), Ok(PlaybackEvent::Unavailable)));
    assert!(matches!(rx.try_recv(), Ok(PlaybackEvent::Unavailable)));
    assert!(rx.try_recv().is_err(), "only the two events were forwarded");
}

/// The line reaches the log only if the binary names this exact target.
#[test]
fn the_log_target_is_the_module_that_logs() {
    assert_eq!(
        module_path!().strip_suffix("::tests"),
        Some(PLAYBACK_LOG_TARGET),
        "the line's target must be the module that logs it"
    );
}
