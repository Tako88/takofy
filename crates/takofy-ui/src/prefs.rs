//! The values that survive a restart, the file that holds them, and the rule
//! for when that file is written.
//!
//! The file is a hand-parsed `key=value` list, so a later version can add a key
//! without corrupting an older one. Unknown keys are ignored; a value this
//! version cannot read refuses the whole file, because a half-applied file
//! would hide the half that was dropped.
//!
//! Writing waits for quiet: a drag moves the level every frame, and the
//! filesystem should see one value, not sixty.

use std::fmt;
use std::time::{Duration, Instant};

use crate::queue::Queue;
use crate::repeat::Repeat;
use crate::volume::{self, Volume};

/// How long a value must be quiet before it is written.
///
/// A drag moves the level every frame; a pause this long is the value at rest.
pub const QUIET: Duration = Duration::from_millis(400);

/// The device name a Soloist daemon announces.
///
/// It is what a listener looks for in the phone's device list, so it is the
/// app's own name. The window announces nothing while this engine is chosen, so
/// the two only collide if a daemon outlives a switch back to the built-in one.
pub const SOLOIST_NAME: &str = "Takofy";

/// Which of the two sound paths plays what the app loads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SoundChoice {
    /// The audio device, through librespot.
    #[default]
    BuiltIn,
    /// Spotify Soloist, through its own daemon.
    Soloist,
}

impl SoundChoice {
    /// The spelling a preferences file stores, which is not the display text.
    pub fn key(self) -> &'static str {
        match self {
            Self::BuiltIn => "builtin",
            Self::Soloist => "soloist",
        }
    }

    /// The engine `key` names, or `None` for any other spelling.
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "builtin" => Some(Self::BuiltIn),
            "soloist" => Some(Self::Soloist),
            _ => None,
        }
    }
}

/// A key the file knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    Volume,
    Mute,
    Shuffle,
    Repeat,
    Discovery,
    Engine,
    SoloistPath,
    SoloistName,
}

impl Key {
    /// Every key, in the order the file lists them.
    const ALL: [Key; 8] = [
        Key::Volume,
        Key::Mute,
        Key::Shuffle,
        Key::Repeat,
        Key::Discovery,
        Key::Engine,
        Key::SoloistPath,
        Key::SoloistName,
    ];

    /// The spelling in the file, which is also the spelling a reader matches.
    fn name(self) -> &'static str {
        match self {
            Key::Volume => "volume",
            Key::Mute => "mute",
            Key::Shuffle => "shuffle",
            Key::Repeat => "repeat",
            Key::Discovery => "discovery",
            Key::Engine => "engine",
            Key::SoloistPath => "soloist_path",
            Key::SoloistName => "soloist_name",
        }
    }

    /// The key `name` spells, or `None` for any key this version does not know.
    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.name() == name)
    }
}

/// Why a line refused the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reason {
    /// The line is not `key=value`.
    NoEquals,
    /// A key this version knows is padded with space.
    SpaceAroundKey,
    /// A known key holds a value outside the table.
    BadValue(Key),
    /// A known key appears on an earlier line.
    Duplicate(Key),
}

/// A refused line.
///
/// The value text is never kept, so a warning cannot print what the user wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refusal {
    line: usize,
    reason: Reason,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.reason {
            Reason::NoEquals => write!(f, "line {}: no `=`", self.line),
            Reason::SpaceAroundKey => write!(f, "line {}: space around a key", self.line),
            Reason::BadValue(key) => {
                write!(f, "line {}: bad value for `{}`", self.line, key.name())
            }
            Reason::Duplicate(key) => {
                write!(f, "line {}: `{}` appears twice", self.line, key.name())
            }
        }
    }
}

/// Volume, mute, shuffle, repeat, discovery and the sound path, as the file
/// holds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefs {
    volume: u16,
    muted: bool,
    shuffle: bool,
    repeat: Repeat,
    discovery: bool,
    engine: SoundChoice,
    soloist_path: Option<String>,
    soloist_name: String,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            volume: volume::MAX,
            muted: false,
            shuffle: false,
            repeat: Repeat::Off,
            discovery: true,
            engine: SoundChoice::BuiltIn,
            soloist_path: None,
            soloist_name: SOLOIST_NAME.to_owned(),
        }
    }
}

impl Prefs {
    /// The values a running app holds, ready to be written.
    pub fn of(volume: &Volume, queue: &Queue) -> Self {
        Self {
            volume: volume.level(),
            muted: volume.muted() && volume.level() > 0,
            shuffle: queue.shuffle(),
            repeat: queue.repeat(),
            ..Self::default()
        }
    }

    /// The same values with the discovery setting the app holds, which the
    /// player knows nothing about.
    pub fn with_discovery(mut self, discovery: bool) -> Self {
        self.discovery = discovery;
        self
    }

    /// Whether the discovery rows may be read.
    pub fn discovery(&self) -> bool {
        self.discovery
    }

    /// The same values with the sound path the app holds, which the player has
    /// no part in.
    ///
    /// A path that is not one line, or is empty, is no path. A name that is not
    /// one line, or is empty, keeps the name already there.
    pub fn with_soloist(mut self, engine: SoundChoice, path: Option<&str>, name: &str) -> Self {
        self.engine = engine;
        self.soloist_path = path
            .and_then(single_line)
            .filter(|path| !path.is_empty())
            .map(str::to_owned);
        if let Some(name) = single_line(name).filter(|name| !name.is_empty()) {
            self.soloist_name = name.to_owned();
        }
        self
    }

    /// Which sound path plays what this file remembers.
    pub fn engine(&self) -> SoundChoice {
        self.engine
    }

    /// The Soloist program the user chose, or `None` when they chose none.
    pub fn soloist_path(&self) -> Option<&str> {
        self.soloist_path.as_deref()
    }

    /// The device name the Soloist daemon announces.
    pub fn soloist_name(&self) -> &str {
        &self.soloist_name
    }

    /// The slider this file remembers, owing the player the level it should
    /// sound at now.
    pub fn volume(&self) -> Volume {
        Volume::restored(self.volume, self.muted)
    }

    /// Applies the saved modes to a queue that is not playing yet.
    pub fn seed_queue(&self, queue: &mut Queue) {
        queue.restore_modes(self.repeat, self.shuffle);
    }

    /// The file's lines, one per key, in a fixed order.
    pub fn render(&self) -> String {
        format!(
            "volume={}\nmute={}\nshuffle={}\nrepeat={}\ndiscovery={}\nengine={}\nsoloist_path={}\nsoloist_name={}\n",
            self.volume,
            self.muted,
            self.shuffle,
            self.repeat.key(),
            self.discovery,
            self.engine.key(),
            self.soloist_path.as_deref().unwrap_or_default(),
            self.soloist_name
        )
    }

    /// Reads the file.
    ///
    /// A line this version cannot read refuses the whole text: nothing is
    /// half-applied. An unknown key or a comment is skipped, so a file from a
    /// later version still loads.
    pub fn parse(text: &str) -> Result<Self, Refusal> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut prefs = Self::default();
        let mut seen: Vec<Key> = Vec::new();

        for (index, raw) in text.split('\n').enumerate() {
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            let refuse = |reason| Refusal {
                line: index + 1,
                reason,
            };
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| refuse(Reason::NoEquals))?;
            let Some(known) = Key::from_name(key.trim()) else {
                continue;
            };
            if key != key.trim() {
                return Err(refuse(Reason::SpaceAroundKey));
            }
            let setting =
                read_value(known, value).ok_or_else(|| refuse(Reason::BadValue(known)))?;
            if seen.contains(&known) {
                return Err(refuse(Reason::Duplicate(known)));
            }
            seen.push(known);
            prefs.set(setting);
        }

        if prefs.volume == 0 {
            prefs.muted = false;
        }
        Ok(prefs)
    }

    /// Stores one setting, which `read_value` has already validated.
    fn set(&mut self, setting: Setting) {
        match setting {
            Setting::Volume(level) => self.volume = level,
            Setting::Mute(muted) => self.muted = muted,
            Setting::Shuffle(shuffle) => self.shuffle = shuffle,
            Setting::Repeat(repeat) => self.repeat = repeat,
            Setting::Discovery(discovery) => self.discovery = discovery,
            Setting::Engine(engine) => self.engine = engine,
            Setting::SoloistPath(path) => self.soloist_path = path,
            Setting::SoloistName(name) => self.soloist_name = name,
        }
    }
}

/// One value the file can hold.
enum Setting {
    Volume(u16),
    Mute(bool),
    Shuffle(bool),
    Repeat(Repeat),
    Discovery(bool),
    Engine(SoundChoice),
    SoloistPath(Option<String>),
    SoloistName(String),
}

/// The value `text` holds for `key`, or `None` when it is outside the table.
fn read_value(key: Key, text: &str) -> Option<Setting> {
    match key {
        Key::Volume => level(text).map(Setting::Volume),
        Key::Mute => flag(text).map(Setting::Mute),
        Key::Shuffle => flag(text).map(Setting::Shuffle),
        Key::Repeat => Repeat::from_key(text).map(Setting::Repeat),
        Key::Discovery => flag(text).map(Setting::Discovery),
        Key::Engine => SoundChoice::from_key(text).map(Setting::Engine),
        Key::SoloistPath => single_line(text)
            .map(|path| (!path.is_empty()).then(|| path.to_owned()))
            .map(Setting::SoloistPath),
        Key::SoloistName => single_line(text)
            .filter(|name| !name.is_empty())
            .map(|name| Setting::SoloistName(name.to_owned())),
    }
}

/// `text` when it fits on one line, which is the only shape a single value can
/// take in a file whose lines are the values.
fn single_line(text: &str) -> Option<&str> {
    (!text.contains('\n') && !text.contains('\r')).then_some(text)
}

/// A level: ASCII digits only, from 0 to full. Signed or padded text is not a
/// level, and leading zeros read as the number they spell.
fn level(text: &str) -> Option<u16> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse::<u16>()
        .ok()
        .filter(|level| *level <= volume::MAX)
}

fn flag(text: &str) -> Option<bool> {
    match text {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Writes a settled value once, and no sooner.
#[derive(Debug)]
pub struct Settle {
    written: Prefs,
    candidate: Option<(Prefs, Instant)>,
}

impl Settle {
    /// A settle whose file already holds `written`.
    pub fn new(written: Prefs) -> Self {
        Self {
            written,
            candidate: None,
        }
    }

    /// Notes what the app holds now, returning the value to write when it has
    /// been quiet for [`QUIET`].
    pub fn observe(&mut self, now: Instant, current: &Prefs) -> Option<Prefs> {
        if *current == self.written {
            self.candidate = None;
            return None;
        }
        match self.candidate.as_ref() {
            Some((pending, since)) if pending == current => {
                if now.saturating_duration_since(*since) < QUIET {
                    return None;
                }
            }
            _ => {
                self.candidate = Some((current.clone(), now));
                return None;
            }
        }
        self.written = current.clone();
        self.candidate = None;
        Some(current.clone())
    }

    /// How long until the pending value has been quiet long enough, or `None`
    /// when nothing is pending.
    ///
    /// It reports the window for the candidate `observe` last saw, so a frame
    /// must call `observe` before `wake`.
    pub fn wake(&self, now: Instant) -> Option<Duration> {
        let (_, since) = self.candidate.as_ref()?;
        Some(QUIET.saturating_sub(now.saturating_duration_since(*since)))
    }

    /// Writes what is pending now, whatever its quiet time, and returns it.
    ///
    /// Used at close, where there is no next frame to wait for.
    pub fn flush(&mut self, current: &Prefs) -> Option<Prefs> {
        self.candidate = None;
        if *current == self.written {
            return None;
        }
        self.written = current.clone();
        Some(current.clone())
    }
}

#[cfg(test)]
mod tests;
