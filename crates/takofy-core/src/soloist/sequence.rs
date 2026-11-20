//! Turning one load into Soloist commands, one answered command at a time.
//!
//! Soloist's own shuffle and repeat modes are switched off before every load,
//! then the track is played, optionally seeked and optionally paused. A new load
//! supersedes an unfinished sequence and drops its remaining commands.

use std::collections::VecDeque;
use std::time::Duration;

use tokio::time::Instant;

use super::ending::COMMAND_BUDGET;
use super::frames::{Command, Frame, Repeat, Status};

/// How long after the `play` result a seek may wait for attach.
pub const SEEK_WAIT: Duration = Duration::from_secs(3);
/// How long a seek may go unconfirmed before it is resent, and then dropped.
pub const VERIFY_BUDGET: Duration = Duration::from_secs(3);
/// How near the target an anchor must land to confirm a seek.
pub const SEEK_MATCH_SLACK_MS: u32 = 1_500;

/// One track to load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Load {
    pub uri: String,
    pub position_ms: u32,
    pub play: bool,
}

/// What the sequence wants done with one input.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Output {
    pub commands: Vec<Command>,
    /// Another device took the sound.
    pub stopped: bool,
    /// Pausing was refused twice; the state will report the truth.
    pub pause_refused: bool,
    /// The frame answered a command a superseded load left behind; no part of
    /// it belongs to the current load.
    pub superseded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Stage {
    #[default]
    Idle,
    Pre,
    Play,
    AwaitSeek,
    Seek,
    Verify,
    Pause,
    Done,
}

/// The command sequence for one connection.
#[derive(Debug, Default)]
pub struct Sequence {
    generation: u64,
    load: Option<Load>,
    stage: Stage,
    pre: VecDeque<Command>,
    in_flight: Option<Command>,
    active_confirmed: bool,
    normalised: bool,
    attached: bool,
    play_result_at: Option<Instant>,
    verify_since: Option<Instant>,
    seek_resent: bool,
    pause_attempts: u8,
    /// Answers still owed by commands a superseded sequence sent. They arrive
    /// in order, so they are swallowed before the new load's own answers.
    stale: u32,
    stale_since: Option<Instant>,
}

impl Sequence {
    /// Starts a load, superseding whatever was in flight.
    pub fn begin(&mut self, generation: u64, load: Load, now: Instant) -> Output {
        self.generation = generation;
        self.load = Some(load);
        self.stage = Stage::Pre;
        if self.in_flight.is_some() {
            self.owe_answer(now);
        }
        self.in_flight = None;
        self.attached = false;
        self.play_result_at = None;
        self.verify_since = None;
        self.seek_resent = false;
        self.pause_attempts = 0;
        self.pre.clear();
        if !self.active_confirmed {
            self.pre.push_back(Command::Activate);
        }
        if !self.normalised {
            self.pre.push_back(Command::SetShuffle { enabled: false });
            self.pre
                .push_back(Command::SetRepeatTrack { enabled: false });
            self.pre
                .push_back(Command::SetRepeatContext { enabled: false });
        }
        Output {
            commands: self.advance(now),
            ..Output::default()
        }
    }

    /// Drops the rest of the sequence, for a stop or a superseding load.
    /// Says the activation this sequence believed in never landed.
    ///
    /// A load that never attached is the evidence: `activate` acknowledged,
    /// the device still not holding the account's sound, the play inaudible.
    /// The next load then activates again instead of believing itself heard.
    pub fn withdraw_activation(&mut self) {
        self.active_confirmed = false;
    }

    pub fn abort(&mut self, now: Instant) {
        if self.in_flight.is_some() {
            self.owe_answer(now);
        }
        self.stage = Stage::Done;
        self.in_flight = None;
        self.pre.clear();
    }

    /// Handles a `command_result`.
    pub fn on_result(&mut self, command: &str, now: Instant) -> Output {
        let mut output = Output::default();
        if self.consume_superseded(now) {
            output.superseded = true;
            return output;
        }
        let Some(sent) = self.in_flight.clone() else {
            return output;
        };
        if sent.name() != command {
            return output;
        }
        self.in_flight = None;
        match self.stage {
            Stage::Pre => {
                if command == "activate" {
                    self.active_confirmed = true;
                }
                if command == "set_repeat_context" {
                    self.normalised = true;
                }
                output.commands = self.advance(now);
            }
            Stage::Play => {
                self.play_result_at = Some(now);
                self.stage = Stage::AwaitSeek;
                let no_seek = self.load.as_ref().is_none_or(|load| load.position_ms == 0);
                if no_seek || self.attached {
                    output.commands = self.advance(now);
                }
            }
            Stage::Seek => {
                self.stage = Stage::Verify;
                self.verify_since = Some(now);
            }
            Stage::Pause => self.stage = Stage::Done,
            Stage::Idle | Stage::AwaitSeek | Stage::Verify | Stage::Done => {}
        }
        output
    }

    /// Handles an `error` frame for the command in flight.
    pub fn on_error(&mut self, now: Instant) -> Output {
        let mut output = Output::default();
        if self.consume_superseded(now) {
            output.superseded = true;
            return output;
        }
        if self.in_flight.take().is_none() {
            return output;
        }
        match self.stage {
            // A refused play is unplayable; the judge reports it.
            Stage::Play => self.stage = Stage::Done,
            // A refused seek leaves the track where Soloist put it.
            Stage::Seek => output.commands = self.after_seek(now),
            Stage::Pause => {
                if self.pause_attempts < 2 {
                    output.commands = self.advance(now);
                } else {
                    self.stage = Stage::Done;
                    output.pause_refused = true;
                }
            }
            Stage::Pre => output.commands = self.advance(now),
            Stage::Idle | Stage::AwaitSeek | Stage::Verify | Stage::Done => {}
        }
        output
    }

    /// Handles an event that may move the sequence along.
    pub fn on_frame(&mut self, frame: &Frame, now: Instant) -> Output {
        let mut output = Output::default();
        match frame {
            Frame::TrackChanged { uri, .. } => {
                if self.expected_uri() == Some(uri.as_str()) {
                    self.attached = true;
                    if self.stage == Stage::AwaitSeek {
                        output.commands = self.advance(now);
                    }
                }
            }
            Frame::PlaybackState(snapshot) => {
                if snapshot.status != Status::Idle
                    && snapshot.item_uri.as_deref() == self.expected_uri()
                {
                    self.attached = true;
                    if self.stage == Stage::AwaitSeek {
                        output.commands = self.advance(now);
                    }
                }
            }
            Frame::PositionSync(anchor) => {
                if self.stage == Stage::Verify
                    && let Some(target) = self.load.as_ref().map(|load| load.position_ms)
                {
                    let distance = i64::from(anchor.position_ms) - i64::from(target);
                    if distance.unsigned_abs() <= u64::from(SEEK_MATCH_SLACK_MS) {
                        output.commands = self.after_seek(now);
                    }
                }
            }
            Frame::OptionsChanged { options } => {
                if options.shuffle || options.repeat != Repeat::Off {
                    self.normalised = false;
                }
            }
            Frame::DeviceChanged { is_active, .. } if !is_active => {
                let wanted = self.load.as_ref().is_some_and(|load| load.play);
                if wanted && !matches!(self.stage, Stage::Idle | Stage::Done) {
                    self.stage = Stage::Done;
                    if self.in_flight.take().is_some() {
                        self.owe_answer(now);
                    }
                    self.active_confirmed = false;
                    output.stopped = true;
                }
            }
            Frame::AuthState { .. }
            | Frame::CommandResult { .. }
            | Frame::ContextChanged { .. }
            | Frame::DeviceChanged { .. }
            | Frame::Error { .. }
            | Frame::PlaybackChanged { .. }
            | Frame::QueueChanged
            | Frame::VolumeChanged { .. }
            | Frame::Unknown => {}
        }
        output
    }

    /// Advances timers that no frame reports.
    pub fn tick(&mut self, now: Instant) -> Output {
        let mut output = Output::default();
        match self.stage {
            Stage::AwaitSeek => {
                let waited = self
                    .play_result_at
                    .is_some_and(|at| now.saturating_duration_since(at) >= SEEK_WAIT);
                if self.attached || waited {
                    output.commands = self.advance(now);
                }
            }
            Stage::Verify => {
                let expired = self
                    .verify_since
                    .is_some_and(|at| now.saturating_duration_since(at) >= VERIFY_BUDGET);
                if expired {
                    if self.seek_resent {
                        output.commands = self.after_seek(now);
                    } else if let Some(target) = self.load.as_ref().map(|load| load.position_ms) {
                        self.seek_resent = true;
                        self.stage = Stage::Seek;
                        self.in_flight = Some(Command::Seek {
                            position_ms: target,
                        });
                        output.commands = vec![Command::Seek {
                            position_ms: target,
                        }];
                    }
                }
            }
            Stage::Idle | Stage::Pre | Stage::Play | Stage::Seek | Stage::Pause | Stage::Done => {}
        }
        output
    }

    fn owe_answer(&mut self, now: Instant) {
        self.stale = self.stale.saturating_add(1);
        self.stale_since.get_or_insert(now);
    }

    /// Swallows one answer owed to a superseded load. A claim lapses with the
    /// answer budget, so a silent daemon cannot eat the new load's answers.
    fn consume_superseded(&mut self, now: Instant) -> bool {
        if self.stale == 0 {
            return false;
        }
        let lapsed = self
            .stale_since
            .is_some_and(|since| now.saturating_duration_since(since) >= COMMAND_BUDGET);
        if lapsed {
            self.stale = 0;
            self.stale_since = None;
            return false;
        }
        self.stale -= 1;
        if self.stale == 0 {
            self.stale_since = None;
        }
        true
    }

    fn expected_uri(&self) -> Option<&str> {
        self.load.as_ref().map(|load| load.uri.as_str())
    }

    fn advance(&mut self, now: Instant) -> Vec<Command> {
        match self.stage {
            Stage::Pre => {
                if let Some(command) = self.pre.pop_front() {
                    self.in_flight = Some(command.clone());
                    return vec![command];
                }
                let Some(load) = self.load.clone() else {
                    self.stage = Stage::Done;
                    return Vec::new();
                };
                self.stage = Stage::Play;
                self.in_flight = Some(Command::Play {
                    uri: load.uri.clone(),
                });
                vec![Command::Play { uri: load.uri }]
            }
            Stage::AwaitSeek => {
                let Some(load) = self.load.clone() else {
                    self.stage = Stage::Done;
                    return Vec::new();
                };
                if load.position_ms == 0 {
                    return self.after_seek(now);
                }
                self.stage = Stage::Seek;
                self.in_flight = Some(Command::Seek {
                    position_ms: load.position_ms,
                });
                vec![Command::Seek {
                    position_ms: load.position_ms,
                }]
            }
            Stage::Pause => {
                self.pause_attempts += 1;
                self.in_flight = Some(Command::Pause);
                vec![Command::Pause]
            }
            Stage::Idle | Stage::Play | Stage::Seek | Stage::Verify | Stage::Done => Vec::new(),
        }
    }

    fn after_seek(&mut self, now: Instant) -> Vec<Command> {
        let play = self.load.as_ref().is_none_or(|load| load.play);
        if play {
            self.stage = Stage::Done;
            Vec::new()
        } else {
            self.stage = Stage::Pause;
            self.advance(now)
        }
    }
}

#[cfg(test)]
mod tests;
