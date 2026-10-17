//! Playback over librespot's player.
//!
//! The player owns a thread that `Drop` joins. `Playback` is therefore the sole
//! owner of its `Arc<Player>`: the field is private, there is no accessor
//! returning the `Arc`, the type is not `Clone`, and the event forwarder holds
//! only the receiver, never the player.

use std::sync::Arc;

use librespot::core::{Session, SpotifyUri};
use librespot::playback::audio_backend::{self, SinkBuilder};
use librespot::playback::config::{AudioFormat, PlayerConfig};
use librespot::playback::mixer::NoOpVolume;
use librespot::playback::player::{Player, PlayerEvent, PlayerEventChannel};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// A playback event with the librespot types stripped out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackEvent {
    /// The player is loading a track.
    Loading,
    /// Playback is running at `position_ms`.
    Playing { position_ms: u32 },
    /// Playback is paused at `position_ms`.
    Paused { position_ms: u32 },
    /// The track reached its end.
    Ended,
    /// The track could not be loaded; a free account hits this.
    Unavailable,
    /// The player was stopped on request.
    Stopped,
}

/// Maps a librespot event, dropping everything with no meaning for a CLI.
pub fn map_event(event: &PlayerEvent) -> Option<PlaybackEvent> {
    match event {
        PlayerEvent::Loading { .. } => Some(PlaybackEvent::Loading),
        PlayerEvent::Playing { position_ms, .. } => Some(PlaybackEvent::Playing {
            position_ms: *position_ms,
        }),
        PlayerEvent::Paused { position_ms, .. } => Some(PlaybackEvent::Paused {
            position_ms: *position_ms,
        }),
        PlayerEvent::EndOfTrack { .. } => Some(PlaybackEvent::Ended),
        PlayerEvent::Unavailable { .. } => Some(PlaybackEvent::Unavailable),
        PlayerEvent::Stopped { .. } => Some(PlaybackEvent::Stopped),
        _ => None,
    }
}

/// How playback finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ended,
    Unavailable,
    Stopped,
}

/// Why playback could not start, or why its event stream ended badly.
#[derive(Debug, thiserror::Error)]
pub enum PlaybackError {
    #[error("no audio backend is available")]
    NoAudioBackend,
    #[error("no audio output device is available")]
    NoAudioDevice,
    #[error("player still shared; the player thread was not joined")]
    PlayerShared,
    #[error("player stopped unexpectedly")]
    PlayerClosed,
    #[error("not a playable track URI: {0}")]
    NotPlayable(String),
}

/// Drains mapped events, reporting each one, and returns the terminal outcome.
///
/// Blocks the calling thread; call off the runtime (e.g. via `spawn_blocking`).
///
/// A channel that closes before `Ended`/`Unavailable`/`Stopped` means the
/// player thread died, which is an error rather than a clean end.
pub fn drive(
    mut rx: UnboundedReceiver<PlaybackEvent>,
    mut on_event: impl FnMut(&PlaybackEvent),
) -> Result<Outcome, PlaybackError> {
    while let Some(event) = rx.blocking_recv() {
        let terminal = match event {
            PlaybackEvent::Ended => Some(Outcome::Ended),
            PlaybackEvent::Unavailable => Some(Outcome::Unavailable),
            PlaybackEvent::Stopped => Some(Outcome::Stopped),
            PlaybackEvent::Loading
            | PlaybackEvent::Playing { .. }
            | PlaybackEvent::Paused { .. } => None,
        };
        on_event(&event);
        if let Some(outcome) = terminal {
            return Ok(outcome);
        }
    }
    Err(PlaybackError::PlayerClosed)
}

/// A running player. The sole owner of the player thread.
pub struct Playback {
    player: Arc<Player>,
}

impl Playback {
    /// Opens the audio device and starts the player.
    ///
    /// Needs an active Tokio runtime: the event forwarder is spawned on it.
    pub fn start(
        session: Session,
    ) -> Result<(Self, UnboundedReceiver<PlaybackEvent>), PlaybackError> {
        let build: SinkBuilder = audio_backend::find(None).ok_or(PlaybackError::NoAudioBackend)?;
        let format = AudioFormat::default();
        let sink_builder = move || build(None, format);

        // Rodio's `open` unwraps inside the player thread, so a missing device
        // would panic there. Probe in a throwaway thread first, whose panic
        // `join` catches and the non-`Send` stream never crosses threads.
        let probe = std::thread::spawn(move || drop(build(None, format))).join();
        if probe.is_err() {
            return Err(PlaybackError::NoAudioDevice);
        }

        let player = Player::new(
            PlayerConfig::default(),
            session,
            Box::new(NoOpVolume),
            sink_builder,
        );
        // Register the event sender before any `load`, on the same FIFO queue.
        let raw = player.get_player_event_channel();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(forward(raw, tx));
        Ok((Self { player }, rx))
    }

    /// Loads and plays one track, starting at 0 ms.
    pub fn play(&self, uri: &str) -> Result<(), PlaybackError> {
        let parsed =
            SpotifyUri::from_uri(uri).map_err(|_| PlaybackError::NotPlayable(uri.to_owned()))?;
        if !parsed.is_playable() {
            return Err(PlaybackError::NotPlayable(uri.to_owned()));
        }
        self.player.load(parsed, true, 0);
        Ok(())
    }

    /// Pauses playback, keeping the loaded track.
    pub fn pause(&self) {
        self.player.pause();
    }

    /// Resumes a paused track.
    pub fn resume(&self) {
        self.player.play();
    }

    /// Stops playback.
    pub fn stop(&self) {
        self.player.stop();
    }

    /// Drops the sole owner off the runtime so `Player::drop` can join.
    pub async fn shutdown(self) -> Result<(), PlaybackError> {
        let player = self.player;
        let result = tokio::task::spawn_blocking(move || match into_sole(player) {
            Ok(player) => {
                // Dropping the sole owner joins the player thread.
                drop(player);
                Ok(())
            }
            Err(_shared) => {
                log::error!("Player still shared; thread not joined");
                Err(PlaybackError::PlayerShared)
            }
        })
        .await;
        match result {
            Ok(result) => result,
            Err(_) => {
                log::error!("Shutdown task failed; thread not joined");
                Err(PlaybackError::PlayerShared)
            }
        }
    }
}

/// Hands back the value only when this is the last owner of the `Arc`.
fn into_sole<T>(arc: Arc<T>) -> Result<T, Arc<T>> {
    Arc::try_unwrap(arc)
}

/// Forwards mapped events until the player's receiver closes.
async fn forward(mut raw: PlayerEventChannel, tx: UnboundedSender<PlaybackEvent>) {
    while let Some(event) = raw.recv().await {
        let Some(mapped) = map_event(&event) else {
            continue;
        };
        if tx.send(mapped).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use librespot::core::SpotifyUri;
    use librespot::playback::player::PlayerEvent;

    use super::{Outcome, PlaybackError, PlaybackEvent, drive, into_sole, map_event};

    fn track() -> SpotifyUri {
        SpotifyUri::from_uri("spotify:track:4uLU6hMCjMI75M1A2tKUQC").expect("valid track uri")
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
}
