//! What this window has loaded, and whether a failed load was the link.
//!
//! A silent drop leaves librespot's keep-alive waiting before it notices the
//! socket is gone, so a load pressed inside that window fails on its key
//! request and reads as an unavailable track. The playback drain asks here
//! before such an `Unavailable` goes on to the window.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use takofy_core::playback::PlaybackEvent;

/// How long a load may go without starting before a failed one is read as a
/// lost link. librespot's key request gives up after this long.
pub const SLOW_LOAD: Duration = Duration::from_millis(1_500);

/// Where a rebuild reloads the track, and whether it plays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumePoint {
    pub uri: String,
    pub position_ms: u32,
    pub play: bool,
}

/// What one `Unavailable` means for the link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// A real failure: the window hears about it and the queue may advance.
    Forward,
    /// The link is down, so the load never had a chance.
    Link,
    /// A load that never started, well past the key timeout.
    Suspect,
}

/// The current load's position, and how much this session's loads are trusted.
#[derive(Clone, Default)]
pub struct Playhead {
    state: Arc<Mutex<State>>,
    down: Arc<AtomicBool>,
}

#[derive(Default)]
struct State {
    point: Option<Point>,
    /// When the current load was handed to the player.
    loaded_at: Option<Instant>,
    /// Whether the current load has reached `Playing`, `Paused` or `Duration`.
    started: bool,
    /// Whether any load since the last generation bump has.
    proven: bool,
    generation: u64,
}

struct Point {
    uri: String,
    position_ms: u32,
    /// Whether the track is moving, which is what the position drifts with.
    playing: bool,
    /// When `position_ms` was true.
    at: Instant,
}

impl State {
    fn anchor(&mut self, position_ms: u32, playing: bool, now: Instant) {
        if let Some(point) = self.point.as_mut() {
            point.position_ms = position_ms;
            point.playing = playing;
            point.at = now;
        }
    }

    fn forget(&mut self) {
        self.point = None;
        self.loaded_at = None;
        self.started = false;
    }
}

impl Playhead {
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records a load, which starts unstarted and unproven.
    pub fn loaded(&self, uri: &str, position_ms: u32, play: bool, now: Instant) {
        let mut state = self.state();
        state.point = Some(Point {
            uri: uri.to_owned(),
            position_ms,
            playing: play,
            at: now,
        });
        state.loaded_at = Some(now);
        state.started = false;
    }

    /// Records what the player did, re-anchoring the position it names.
    ///
    /// An event from a retired generation says nothing about this load.
    pub fn observe(&self, generation: u64, event: &PlaybackEvent, now: Instant) {
        let mut state = self.state();
        if generation != state.generation {
            return;
        }
        match *event {
            PlaybackEvent::Playing { position_ms } => {
                state.anchor(position_ms, true, now);
                state.started = true;
                state.proven = true;
            }
            PlaybackEvent::Paused { position_ms } => {
                state.anchor(position_ms, false, now);
                state.started = true;
                state.proven = true;
            }
            PlaybackEvent::Duration { .. } => {
                state.started = true;
                state.proven = true;
            }
            PlaybackEvent::Seeked { position_ms } => {
                if let Some(point) = state.point.as_mut() {
                    point.position_ms = position_ms;
                    point.at = now;
                }
            }
            PlaybackEvent::Ended | PlaybackEvent::Stopped => state.forget(),
            // A preload failure arrives while the loaded track still plays, so
            // only an unstarted load reads as the track itself failing.
            PlaybackEvent::Unavailable => {
                if !state.started {
                    state.forget();
                }
            }
            PlaybackEvent::Loading => {}
        }
    }

    /// Says what a failed load means, without forgetting anything.
    ///
    /// A failure from a retired generation is forwarded: it is not evidence
    /// about this one.
    pub fn judge_unavailable(&self, generation: u64, now: Instant) -> Verdict {
        let state = self.state();
        if generation != state.generation {
            return Verdict::Forward;
        }
        if self.is_down() {
            return Verdict::Link;
        }
        if !state.proven || state.started {
            return Verdict::Forward;
        }
        match state.loaded_at {
            Some(loaded_at) if now.saturating_duration_since(loaded_at) >= SLOW_LOAD => {
                Verdict::Suspect
            }
            _ => Verdict::Forward,
        }
    }

    /// Where a rebuild reloads the track, and whether it plays.
    pub fn resume_point(&self, now: Instant) -> Option<ResumePoint> {
        let state = self.state();
        let point = state.point.as_ref()?;
        // Only a started load moves: an unstarted one sits where it was asked
        // to load, however long it has been pending.
        let position_ms = if point.playing && state.started {
            point
                .position_ms
                .saturating_add(millis(now.saturating_duration_since(point.at)))
        } else {
            point.position_ms
        };
        Some(ResumePoint {
            uri: point.uri.clone(),
            position_ms,
            play: point.playing,
        })
    }

    pub fn set_down(&self, down: bool) {
        self.down.store(down, Ordering::SeqCst);
    }

    pub fn is_down(&self) -> bool {
        self.down.load(Ordering::SeqCst)
    }

    /// Opens a new session generation: it starts unproven and with the link up,
    /// and keeps the point a rebuild reloads.
    pub fn next_generation(&self) -> u64 {
        let generation = {
            let mut state = self.state();
            state.generation += 1;
            state.proven = false;
            state.generation
        };
        self.set_down(false);
        generation
    }

    pub fn generation(&self) -> u64 {
        self.state().generation
    }

    /// Forgets the load, so a rebuild has nothing to reload.
    pub fn clear(&self) {
        self.state().forget();
    }
}

/// A position gap in milliseconds, capped so it can be added to one.
fn millis(elapsed: Duration) -> u32 {
    u32::try_from(elapsed.as_millis()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests;
