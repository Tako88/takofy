//! Playback over librespot's player.
//!
//! The player owns a thread that `Drop` joins. `Playback` is therefore the sole
//! owner of its `Arc<Player>`: the field is private, there is no accessor
//! returning the `Arc`, the type is not `Clone`, and the event forwarder holds
//! only the receiver, never the player.

use std::future::Future;
use std::pin::Pin;
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

use crate::soloist::SoloistFault;

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
    #[error("{0}")]
    Soloist(SoloistFault),
}

/// How long a device may take to open before it is reported as missing.
///
/// Opening a device is the one call in this stack that can block on a driver
/// that has stopped answering, and it happens on the task that serves every
/// other command. A bound turns a wedged driver into a sentence instead of a
/// window that draws and does nothing.
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Opens the output device once and closes it, without a session or a player.
///
/// Rodio's `open` unwraps inside the player thread, so a missing device would
/// panic there and take the thread with it, silently. Probing in a throwaway
/// thread turns that into an error this side can draw, and the non-`Send` stream
/// never crosses threads.
///
/// This is also what `--probe-audio` reports: it answers "can this machine open
/// an output device at all" without a login, which is the first question a
/// silent install asks.
pub fn probe_device() -> Result<(), PlaybackError> {
    let build: SinkBuilder = audio_backend::find(None).ok_or(PlaybackError::NoAudioBackend)?;
    let format = AudioFormat::default();
    let (done, opened) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // A panic here is the missing device; the join would carry it, and a
        // channel carries it without making the caller wait forever.
        drop(build(None, format));
        let _ = done.send(());
    });
    match opened.recv_timeout(PROBE_TIMEOUT) {
        Ok(()) => Ok(()),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(PlaybackError::NoAudioDevice),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(PlaybackError::NoAudioDevice),
    }
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

/// The engine-agnostic seam: whatever makes sound.
///
/// The event stream is not part of it. A sound is handed back beside the
/// `UnboundedReceiver<PlaybackEvent>` it feeds, as `Playback::start` already does.
pub trait Sound: Send + Sync {
    /// Loads one track, starting at `position_ms` and playing only when asked to.
    fn load(&self, uri: &str, position_ms: u32, play: bool) -> Result<(), PlaybackError>;

    /// Loads and plays one track, starting at 0 ms.
    fn play(&self, uri: &str) -> Result<(), PlaybackError>;

    /// Jumps within the loaded track.
    fn seek(&self, position_ms: u32);

    /// Pauses playback, keeping the loaded track.
    fn pause(&self);

    /// Resumes a paused track.
    fn resume(&self);

    /// Stops playback.
    fn stop(&self);

    /// Sets the level, 0 to `VOLUME_MAX`.
    fn set_volume(&self, percent: u16);

    /// Gives the engine back, joining whatever thread it owns.
    fn shutdown(self: Box<Self>)
    -> Pin<Box<dyn Future<Output = Result<(), PlaybackError>> + Send>>;
}

/// A sound and the stream of events it produces.
pub type ExternalSound = (Box<dyn Sound>, UnboundedReceiver<PlaybackEvent>);

/// Which engine feeds the player.
pub enum SoundEngine {
    BuiltIn,
    External(ExternalSound),
}

/// Picks the engine.
///
/// The closure is the built-in device path, and it is not called when another
/// engine was handed in.
pub fn choose(
    engine: SoundEngine,
    start_builtin: impl FnOnce() -> Result<ExternalSound, PlaybackError>,
) -> Result<ExternalSound, PlaybackError> {
    match engine {
        SoundEngine::BuiltIn => start_builtin(),
        SoundEngine::External(sound) => Ok(sound),
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

        probe_device()?;

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

impl Sound for Playback {
    fn load(&self, uri: &str, position_ms: u32, play: bool) -> Result<(), PlaybackError> {
        Playback::load(self, uri, position_ms, play)
    }

    fn play(&self, uri: &str) -> Result<(), PlaybackError> {
        Playback::play(self, uri)
    }

    fn seek(&self, position_ms: u32) {
        Playback::seek(self, position_ms);
    }

    fn pause(&self) {
        Playback::pause(self);
    }

    fn resume(&self) {
        Playback::resume(self);
    }

    fn stop(&self) {
        Playback::stop(self);
    }

    /// The sink reads the `Volume` handle the player was built with, so the
    /// level is already set by the time it gets here.
    fn set_volume(&self, _percent: u16) {}

    fn shutdown(
        self: Box<Self>,
    ) -> Pin<Box<dyn Future<Output = Result<(), PlaybackError>> + Send>> {
        Box::pin(async move { Playback::shutdown(*self).await })
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
