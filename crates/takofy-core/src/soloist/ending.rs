//! The sharp edges at the end of a Soloist load.
//!
//! Soloist has no "ended" event, so the end of a track, an external stop and an
//! unplayable item are judged from the frames that do arrive. One latch per load
//! generation means exactly one terminal event, whatever fires first.

use std::time::Duration;

use tokio::time::Instant;

use super::frames::{Frame, Status};
use super::state::{Expect, SoloistState};
use crate::playback::PlaybackEvent;

/// How near the reported end counts as the end of a track.
pub const END_SLACK_MS: u32 = 2_000;
/// How far past the end a still-advancing position may run before it is the end.
pub const END_GRACE_MS: u32 = 3_000;
/// How long a command may go unanswered before it is a failure.
pub const COMMAND_BUDGET: Duration = Duration::from_secs(5);
/// How long a load may stay unattached before it is unplayable.
pub const LOAD_BUDGET: Duration = Duration::from_secs(10);

/// What one observation decided.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Verdict {
    pub events: Vec<PlaybackEvent>,
    /// Soloist advanced on its own, so it must be paused.
    pub pause: bool,
}

/// The per-load judge.
#[derive(Debug, Default)]
pub struct Ending {
    generation: u64,
    latched: bool,
    play_in_flight: Option<Instant>,
    load_started: Option<Instant>,
}

impl Ending {
    /// Starts a fresh load generation and drops any previous terminal state.
    pub fn begin_load(&mut self, generation: u64, now: Instant) {
        self.generation = generation;
        self.latched = false;
        self.play_in_flight = None;
        self.load_started = Some(now);
    }

    /// Notes that a `play` command was just sent.
    pub fn play_sent(&mut self, now: Instant) {
        self.play_in_flight = Some(now);
    }

    /// Ends this generation without emitting anything: a stop was asked for.
    pub fn suppress(&mut self) {
        self.latched = true;
    }

    /// Judges one frame.
    ///
    /// Answers a superseded load left in flight must be filtered out first: an
    /// `error` frame names no command, so whoever calls this cannot tell the
    /// two apart.
    pub fn observe(
        &mut self,
        frame: &Frame,
        now: Instant,
        state: &SoloistState,
        expect: &Expect,
    ) -> Verdict {
        if self.latched || expect.generation != self.generation {
            return Verdict::default();
        }
        match frame {
            Frame::CommandResult { command } if command == "play" => {
                self.play_in_flight = None;
            }
            Frame::Error { .. } => {
                if self.play_in_flight.is_some() {
                    return self.unavailable();
                }
            }
            Frame::TrackChanged { uri, .. } => {
                if expect.seen_attached && !uri.is_empty() && uri != &expect.uri {
                    return self.ended(true);
                }
            }
            Frame::PlaybackChanged { status } if *status == Status::Idle => {
                if expect.seen_attached {
                    return self.judge_idle(now, state);
                }
            }
            Frame::PlaybackState(snapshot)
                if snapshot.status == Status::Idle && expect.seen_attached =>
            {
                return self.judge_idle(now, state);
            }
            _ => {}
        }
        Verdict::default()
    }

    /// Advances the clocks a frame cannot report.
    pub fn tick(&mut self, now: Instant, state: &SoloistState, expect: &Expect) -> Verdict {
        if self.latched || expect.generation != self.generation {
            return Verdict::default();
        }
        if let Some(sent) = self.play_in_flight
            && now.saturating_duration_since(sent) >= COMMAND_BUDGET
        {
            return self.unavailable();
        }
        if !expect.seen_attached
            && let Some(started) = self.load_started
            && now.saturating_duration_since(started) >= LOAD_BUDGET
        {
            return self.unavailable();
        }
        if expect.seen_attached
            && let Some(duration) = state.duration_ms
            && let Some(anchor) = state.clock.current()
            && anchor.speed > 0.0
        {
            let live = state.clock.live_ms(now);
            if i64::from(live) > i64::from(duration) + i64::from(END_GRACE_MS) {
                return self.ended(false);
            }
        }
        Verdict::default()
    }

    fn judge_idle(&mut self, now: Instant, state: &SoloistState) -> Verdict {
        let position = state.clock.live_ms(now);
        match state.duration_ms {
            None => self.ended(false),
            Some(duration)
                if i64::from(position) + i64::from(END_SLACK_MS) >= i64::from(duration) =>
            {
                self.ended(false)
            }
            Some(_) => self.stopped(),
        }
    }

    fn ended(&mut self, pause: bool) -> Verdict {
        self.latched = true;
        Verdict {
            events: vec![PlaybackEvent::Ended],
            pause,
        }
    }

    fn stopped(&mut self) -> Verdict {
        self.latched = true;
        Verdict {
            events: vec![PlaybackEvent::Stopped],
            pause: false,
        }
    }

    fn unavailable(&mut self) -> Verdict {
        self.latched = true;
        Verdict {
            events: vec![PlaybackEvent::Unavailable],
            pause: false,
        }
    }
}

#[cfg(test)]
mod tests;
