//! Folding Soloist frames into the shared playback vocabulary.
//!
//! The fold only reports transitions. A repeated status emits nothing, and a
//! duration is reported once per load generation, then again only when it
//! changes. The fold never invents a duration Soloist did not send.

use tokio::time::Instant;

use super::SoloistFault;
use super::frames::{Frame, Options, Snapshot, Status};
use super::position::Clock;
use crate::playback::PlaybackEvent;

/// How far a reported anchor may be from the prediction before it is a seek.
pub const SEEK_JUMP_MS: u32 = 1_000;

/// The queryable state of one Soloist connection.
#[derive(Debug, Clone)]
pub struct SoloistState {
    pub logged_in: bool,
    pub auth_known: bool,
    pub status: Status,
    pub item_uri: Option<String>,
    pub duration_ms: Option<u32>,
    pub volume: Option<u8>,
    pub is_active: bool,
    pub device_name: String,
    pub options: Options,
    pub clock: Clock,
}

impl Default for SoloistState {
    fn default() -> Self {
        Self {
            logged_in: false,
            auth_known: false,
            status: Status::Idle,
            item_uri: None,
            duration_ms: None,
            volume: None,
            is_active: false,
            device_name: String::new(),
            options: Options::default(),
            clock: Clock::default(),
        }
    }
}

impl SoloistState {
    /// The fault the account state names, once Soloist has said anything.
    pub fn pairing(&self) -> Option<SoloistFault> {
        if self.auth_known && !self.logged_in {
            Some(SoloistFault::Unpaired)
        } else {
            None
        }
    }
}

/// What the current load is waiting for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Expect {
    pub generation: u64,
    pub uri: String,
    pub seen_attached: bool,
}

/// Turns frames into events and keeps the queryable state.
#[derive(Debug, Default)]
pub struct Fold {
    pub state: SoloistState,
    generation: u64,
    loading: bool,
    duration: Option<u32>,
    /// The last status reported as an event. A new load may leave the daemon on
    /// the status the old one ended at, and must still be heard.
    reported: Option<Status>,
}

impl Fold {
    /// Applies one frame and reports the events it justifies.
    pub fn apply(
        &mut self,
        frame: &Frame,
        now: Instant,
        expect: &mut Expect,
    ) -> Vec<PlaybackEvent> {
        if expect.generation != self.generation {
            self.generation = expect.generation;
            self.loading = false;
            self.duration = None;
            self.reported = None;
        }

        let mut events = Vec::new();
        match frame {
            Frame::AuthState {
                logged_in,
                is_active,
                device_name,
            } => {
                self.state.auth_known = true;
                self.state.logged_in = *logged_in;
                self.state.is_active = *is_active;
                self.state.device_name.clone_from(device_name);
            }
            Frame::PlaybackState(snapshot) => {
                self.absorb(snapshot, now);
                self.status_event(snapshot.status, now, expect, &mut events);
            }
            Frame::TrackChanged { uri, duration_ms } => {
                if !uri.is_empty() {
                    self.state.item_uri = Some(uri.clone());
                }
                if let Some(duration) = duration_ms {
                    self.state.duration_ms = Some(*duration);
                    self.emit_duration(*duration, &mut events);
                }
                if *uri == expect.uri && !uri.is_empty() {
                    expect.seen_attached = true;
                    self.loading = false;
                }
            }
            Frame::PlaybackChanged { status } => {
                self.status_event(*status, now, expect, &mut events);
            }
            Frame::VolumeChanged { volume } => self.state.volume = Some(*volume),
            Frame::DeviceChanged {
                is_active,
                device_name,
            } => {
                self.state.is_active = *is_active;
                if let Some(name) = device_name {
                    self.state.device_name.clone_from(name);
                }
            }
            Frame::OptionsChanged { options } => self.state.options = *options,
            Frame::PositionSync(anchor) => {
                let predicted = self.state.clock.live_ms(now);
                let jump = i64::from(predicted) - i64::from(anchor.position_ms);
                if self.state.clock.is_anchored() && jump.unsigned_abs() > u64::from(SEEK_JUMP_MS) {
                    events.push(PlaybackEvent::Seeked {
                        position_ms: anchor.position_ms,
                    });
                }
                self.state.clock.anchor(*anchor, now);
            }
            Frame::CommandResult { command } => {
                if command == "play" && !expect.seen_attached && !self.loading {
                    self.loading = true;
                    events.push(PlaybackEvent::Loading);
                }
            }
            Frame::Error { .. }
            | Frame::ContextChanged { .. }
            | Frame::QueueChanged
            | Frame::Unknown => {}
        }
        events
    }

    fn absorb(&mut self, snapshot: &Snapshot, now: Instant) {
        if let Some(uri) = &snapshot.item_uri {
            self.state.item_uri = Some(uri.clone());
        }
        if let Some(duration) = snapshot.duration_ms {
            self.state.duration_ms = Some(duration);
        }
        if let Some(volume) = snapshot.volume {
            self.state.volume = Some(volume);
        }
        if let Some(is_active) = snapshot.is_active {
            self.state.is_active = is_active;
        }
        if let Some(options) = snapshot.options {
            self.state.options = options;
        }
        if let Some(anchor) = snapshot.position {
            self.state.clock.anchor(anchor, now);
        }
    }

    fn status_event(
        &mut self,
        status: Status,
        now: Instant,
        expect: &mut Expect,
        events: &mut Vec<PlaybackEvent>,
    ) {
        let matches = self.state.item_uri.as_deref() == Some(expect.uri.as_str());
        if matches && status != Status::Idle && !expect.uri.is_empty() {
            expect.seen_attached = true;
        }
        let previous = self.reported;
        self.state.status = status;
        self.reported = Some(status);
        if let Some(duration) = self.state.duration_ms {
            self.emit_duration(duration, events);
        }
        if previous == Some(status) {
            return;
        }
        match status {
            Status::Buffering => {
                if !self.loading {
                    self.loading = true;
                    events.push(PlaybackEvent::Loading);
                }
            }
            Status::Playing => {
                self.loading = false;
                events.push(PlaybackEvent::Playing {
                    position_ms: self.state.clock.live_ms(now),
                });
            }
            Status::Paused => {
                self.loading = false;
                events.push(PlaybackEvent::Paused {
                    position_ms: self.state.clock.live_ms(now),
                });
            }
            Status::Idle | Status::Other => self.loading = false,
        }
    }

    fn emit_duration(&mut self, duration: u32, events: &mut Vec<PlaybackEvent>) {
        if self.duration == Some(duration) {
            return;
        }
        self.duration = Some(duration);
        events.push(PlaybackEvent::Duration {
            duration_ms: duration,
        });
    }
}

#[cfg(test)]
mod tests;
