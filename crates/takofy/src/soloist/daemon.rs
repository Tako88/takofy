//! The daemon's state machine.
//!
//! Pure: every instant is handed in, so each rule is a test and nothing here
//! waits, spawns or reads. The loop that performs the actions lives beside it.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use takofy_core::connect::is_own_sound_device;
use takofy_core::soloist::expiry;

/// How long a daemon we spawned has to answer on a socket of its own.
pub const SPAWN_WAIT: Duration = Duration::from_secs(10);

/// How long a general failure waits for evidence of another instance.
///
/// Sized to one connect attempt: the socket is the only evidence there is.
pub const ADOPT_WAIT: Duration = Duration::from_secs(2);

/// The window a burst of failed attempts is counted over.
pub const BACKOFF_WINDOW: Duration = Duration::from_secs(60);

/// How many attempts that window allows before the machine settles.
pub const MAX_ATTEMPTS: usize = 3;

/// Where the daemon is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// No program was found.
    Missing,
    /// The program is there and is not running.
    NotRunning,
    /// We started it and are waiting for it to answer.
    Starting,
    /// It runs but opened no socket we could talk to.
    NoSocket,
    /// It answered, and is not signed in to the account.
    Unpaired,
    /// The build is past its expiry.
    Expired,
    /// It answered as this window's own device.
    Ready,
    /// A daemon someone else started answered as this window's own device.
    Adopted,
}

impl State {
    /// Whether a sound can be asked of a daemon in this state.
    pub fn playable(self) -> bool {
        matches!(self, Self::Ready | Self::Adopted)
    }

    /// Whether the process should be left running in this state.
    ///
    /// An unpaired daemon is waiting for a phone to pick it out of Spotify's
    /// device list, so killing it is how pairing becomes impossible: the phone
    /// would look for a process that no longer exists. Everything a phone can
    /// fix keeps its process; everything else is ours to replace.
    pub fn keeps_child(self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Ready | Self::Adopted | Self::Unpaired
        )
    }

    /// The fault a sound asked now would answer with, or `None` while the
    /// machine is still working on it.
    pub fn fault(self) -> Option<takofy_core::SoloistFault> {
        use takofy_core::SoloistFault;
        match self {
            Self::Missing => Some(SoloistFault::Missing),
            Self::NotRunning => Some(SoloistFault::NotRunning),
            Self::NoSocket => Some(SoloistFault::NoSocket),
            Self::Unpaired => Some(SoloistFault::Unpaired),
            Self::Expired => Some(SoloistFault::Expired),
            Self::Starting | Self::Ready | Self::Adopted => None,
        }
    }
}

/// What the machine is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The program was found.
    Located,
    /// A process we started exists.
    Spawned,
    /// A process ended, or its socket did.
    Exited(i32),
    /// Its port file was read.
    PortRead,
    /// Its socket accepted a connection.
    Connected,
    /// It said who it is.
    AuthState {
        logged_in: bool,
        device_name: String,
    },
    /// Time passed.
    Tick,
}

/// What the loop should do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Spawn,
    Connect,
}

pub struct Daemon {
    /// The name this window's sound announces itself under.
    name: String,
    state: State,
    /// When the current attempt began.
    attempt_at: Option<Instant>,
    /// Whether this attempt's socket answered.
    connected: bool,
    /// Whether this attempt's process is one we started.
    spawned: bool,
    /// When the recent attempts ended.
    attempts: VecDeque<Instant>,
    /// When the wait for adoption evidence ends.
    evidence_until: Option<Instant>,
}

impl Daemon {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            state: State::Missing,
            attempt_at: None,
            connected: false,
            spawned: false,
            attempts: VecDeque::new(),
            evidence_until: None,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    /// Names the build unusable. Only `--version` decides this, and an expired
    /// build is never started.
    pub fn expire(&mut self) {
        self.state = State::Expired;
        self.evidence_until = None;
    }

    /// Whether a general failure is waiting for a socket to answer.
    pub fn awaiting_evidence(&self) -> bool {
        self.evidence_until.is_some()
    }

    /// Opens one more attempt for a user action.
    ///
    /// The window bounds how often the machine restarts itself; a person
    /// asking for sound is not the machine retrying.
    pub fn retry(&mut self) {
        if matches!(self.state, State::NotRunning | State::NoSocket) {
            self.attempts.clear();
            self.state = State::NotRunning;
        }
    }

    pub fn event(&mut self, event: Event, now: Instant) {
        match event {
            Event::Located => {
                if self.state == State::Missing {
                    self.state = State::NotRunning;
                }
            }
            Event::Spawned => {
                self.attempt_at = Some(now);
                self.connected = false;
                self.spawned = true;
                self.evidence_until = None;
                self.state = State::Starting;
            }
            Event::Exited(code) => self.exited(code, now),
            Event::PortRead => {}
            Event::Connected => {
                if self.state == State::Starting {
                    self.connected = true;
                }
            }
            Event::AuthState {
                logged_in,
                device_name,
            } => self.auth(logged_in, &device_name),
            Event::Tick => self.tick(now),
        }
    }

    /// What the loop should do, as of `now`.
    pub fn action(&self, now: Instant) -> Action {
        match self.state {
            State::NotRunning if self.attempts_left(now) => Action::Spawn,
            State::Starting => Action::Connect,
            _ => Action::None,
        }
    }

    fn exited(&mut self, code: i32, now: Instant) {
        self.connected = false;
        self.spawned = false;
        if expiry::exit_code_means_expired(code) {
            self.state = State::Expired;
            self.evidence_until = None;
            return;
        }
        self.attempts.push_back(now);
        if code == 1 {
            // A general failure is not a diagnosis: the same code covers a
            // bad argument, an unwritable directory and another process
            // holding the data directory. Only the socket can say which.
            self.state = State::Starting;
            self.evidence_until = Some(now + ADOPT_WAIT);
            return;
        }
        self.evidence_until = None;
        self.state = State::NotRunning;
    }

    fn auth(&mut self, logged_in: bool, device_name: &str) {
        if self.state == State::Expired {
            return;
        }
        if !logged_in {
            self.state = State::Unpaired;
            self.evidence_until = None;
            return;
        }
        if self.state != State::Starting {
            return;
        }
        if is_own_sound_device(device_name, &self.name) {
            self.state = if self.spawned {
                State::Ready
            } else {
                State::Adopted
            };
            self.evidence_until = None;
            return;
        }
        // A port file can outlive its process, and the API has no
        // authentication: a device that is not ours is a stranger, and the
        // connection to it must not be kept.
        self.state = State::NotRunning;
        self.evidence_until = None;
    }

    fn tick(&mut self, now: Instant) {
        if let Some(until) = self.evidence_until {
            if now >= until {
                self.evidence_until = None;
                self.state = State::NotRunning;
            }
            return;
        }
        if self.state == State::Starting {
            let waited = self
                .attempt_at
                .is_some_and(|at| now.saturating_duration_since(at) >= SPAWN_WAIT);
            if waited {
                self.state = State::NoSocket;
            }
        }
    }

    fn attempts_left(&self, now: Instant) -> bool {
        self.attempts
            .iter()
            .filter(|at| now.saturating_duration_since(**at) < BACKOFF_WINDOW)
            .count()
            < MAX_ATTEMPTS
    }
}

#[cfg(test)]
mod tests;
