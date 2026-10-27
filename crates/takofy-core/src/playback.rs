//! Playback over librespot's player.
//!
//! The player owns a thread that `Drop` joins. `Playback` is therefore the sole
//! owner of its `Arc<Player>`: the field is private, there is no accessor
//! returning the `Arc`, the type is not `Clone`, and the event forwarder holds
//! only the receiver, never the player.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use librespot::core::{Session, SpotifyUri};
use librespot::metadata::audio::item::AudioItem;
use librespot::playback::audio_backend::{self, SinkBuilder};
use librespot::playback::config::{AudioFormat, PlayerConfig, VolumeCtrl};
use librespot::playback::mixer::VolumeGetter;
use librespot::playback::mixer::mappings::MappedCtrl;
use librespot::playback::player::{Player, PlayerEvent, PlayerEventChannel};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// The loudest level a slider can name.
pub const VOLUME_MAX: u16 = 100;

/// The log target our own playback lines are filed under.
pub const PLAYBACK_LOG_TARGET: &str = "takofy_core::playback";

/// What an unavailable track logs: no cause, because `forward` cannot know one,
/// and no track. A preload failure reaches here before the track is playing.
const SKIPPED: &str = "a track could not be loaded and is unavailable";

/// The gain, shared with the player thread.
///
/// librespot reads the attenuation per packet through the `VolumeGetter` it was
/// built with, and its player has no setter, so the level lives here: the binary
/// keeps a handle to the same cell the player reads, and one atomic store
/// changes what comes out of the speakers.
#[derive(Clone, Debug)]
pub struct Volume {
    /// The attenuation factor as `f64` bits, because there is no `AtomicF64`.
    attenuation: Arc<AtomicU64>,
}

impl Volume {
    /// A handle to the level, seeded at `percent`.
    pub fn at(percent: u16) -> Self {
        Self {
            attenuation: Arc::new(AtomicU64::new(attenuation(percent).to_bits())),
        }
    }

    /// Sets the level. 0 is silence, 100 is unity gain, and anything above is
    /// clamped rather than louder than the source.
    pub fn set(&self, percent: u16) {
        self.attenuation
            .store(attenuation(percent).to_bits(), Ordering::Relaxed);
    }
}

impl Default for Volume {
    fn default() -> Self {
        Self::at(VOLUME_MAX)
    }
}

impl VolumeGetter for Volume {
    fn attenuation_factor(&self) -> f64 {
        f64::from_bits(self.attenuation.load(Ordering::Relaxed))
    }
}

/// The gain one percent names.
///
/// librespot's own log curve, so the slider's travel sounds like every other
/// client's instead of inventing a law of its own.
fn attenuation(percent: u16) -> f64 {
    let level = u32::from(percent.min(VOLUME_MAX)) * u32::from(u16::MAX) / u32::from(VOLUME_MAX);
    VolumeCtrl::default().to_mapped(level as u16)
}

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
    /// A track is loaded and this is how long it is.
    Duration { duration_ms: u32 },
    /// The player jumped to `position_ms` within the loaded track.
    Seeked { position_ms: u32 },
}

/// A track's length, read off the audio item so the mapping below stays one
/// line and the test can name a duration without the item's other fields.
fn duration_of(audio_item: &AudioItem) -> u32 {
    audio_item.duration_ms
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
        PlayerEvent::TrackChanged { audio_item } => Some(PlaybackEvent::Duration {
            duration_ms: duration_of(audio_item),
        }),
        PlayerEvent::Seeked { position_ms, .. } => Some(PlaybackEvent::Seeked {
            position_ms: *position_ms,
        }),
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
            | PlaybackEvent::Paused { .. }
            | PlaybackEvent::Duration { .. }
            | PlaybackEvent::Seeked { .. } => None,
        };
        on_event(&event);
        if let Some(outcome) = terminal {
            return Ok(outcome);
        }
    }
    Err(PlaybackError::PlayerClosed)
}

/// The track or episode a URI names, ready for the player.
///
/// Only these two kinds carry an audio stream: a local file names the user's own
/// disk, and everything else is a container, so `is_playable` alone would let a
/// local file through to a load that cannot succeed.
pub fn playable_track(uri: &str) -> Result<SpotifyUri, PlaybackError> {
    let parsed =
        SpotifyUri::from_uri(uri).map_err(|_| PlaybackError::NotPlayable(uri.to_owned()))?;
    match parsed {
        SpotifyUri::Track { .. } | SpotifyUri::Episode { .. } => Ok(parsed),
        _ => Err(PlaybackError::NotPlayable(uri.to_owned())),
    }
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
        volume: Volume,
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
            Box::new(volume),
            sink_builder,
        );
        // Register the event sender before any `load`, on the same FIFO queue.
        let raw = player.get_player_event_channel();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(forward(raw, tx));
        Ok((Self { player }, rx))
    }

    /// Loads one track, starting at `position_ms` and playing only when asked
    /// to.
    pub fn load(&self, uri: &str, position_ms: u32, play: bool) -> Result<(), PlaybackError> {
        let parsed = playable_track(uri)?;
        self.player.load(parsed, play, position_ms);
        Ok(())
    }

    /// Loads and plays one track, starting at 0 ms.
    pub fn play(&self, uri: &str) -> Result<(), PlaybackError> {
        self.load(uri, 0, true)
    }

    /// Jumps within the loaded track. Only a loaded track can be jumped in;
    /// while loading it restarts the load at that position.
    pub fn seek(&self, position_ms: u32) {
        self.player.seek(position_ms);
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
        if matches!(event, PlayerEvent::Unavailable { .. }) {
            log::warn!(target: PLAYBACK_LOG_TARGET, "{SKIPPED}");
        }
        let Some(mapped) = map_event(&event) else {
            continue;
        };
        if tx.send(mapped).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
