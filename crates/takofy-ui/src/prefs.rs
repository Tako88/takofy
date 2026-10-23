//! The four values that survive a restart, the file that holds them, and the
//! rule for when that file is written.
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

/// A key the file knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    Volume,
    Mute,
    Shuffle,
    Repeat,
}

impl Key {
    /// Every key, in the order the file lists them.
    const ALL: [Key; 4] = [Key::Volume, Key::Mute, Key::Shuffle, Key::Repeat];

    /// The spelling in the file, which is also the spelling a reader matches.
    fn name(self) -> &'static str {
        match self {
            Key::Volume => "volume",
            Key::Mute => "mute",
            Key::Shuffle => "shuffle",
            Key::Repeat => "repeat",
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

/// Volume, mute, shuffle and repeat, as the file holds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefs {
    volume: u16,
    muted: bool,
    shuffle: bool,
    repeat: Repeat,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            volume: volume::MAX,
            muted: false,
            shuffle: false,
            repeat: Repeat::Off,
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
        }
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

    /// The file's four lines, in a fixed order.
    pub fn render(&self) -> String {
        format!(
            "volume={}\nmute={}\nshuffle={}\nrepeat={}\n",
            self.volume,
            self.muted,
            self.shuffle,
            self.repeat.key()
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
        }
    }
}

/// One value the file can hold.
enum Setting {
    Volume(u16),
    Mute(bool),
    Shuffle(bool),
    Repeat(Repeat),
}

/// The value `text` holds for `key`, or `None` when it is outside the table.
fn read_value(key: Key, text: &str) -> Option<Setting> {
    match key {
        Key::Volume => level(text).map(Setting::Volume),
        Key::Mute => flag(text).map(Setting::Mute),
        Key::Shuffle => flag(text).map(Setting::Shuffle),
        Key::Repeat => Repeat::from_key(text).map(Setting::Repeat),
    }
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
mod tests {
    use super::*;

    fn prefs(volume: u16) -> Prefs {
        Prefs {
            volume,
            ..Prefs::default()
        }
    }

    fn refusal(line: usize, reason: Reason) -> Refusal {
        Refusal { line, reason }
    }

    #[test]
    fn prefs_round_trip() {
        for repeat in [Repeat::Off, Repeat::All, Repeat::One] {
            for mute in [false, true] {
                for shuffle in [false, true] {
                    for level in [0, 1, volume::MAX] {
                        let prefs = Prefs {
                            volume: level,
                            muted: mute && level > 0,
                            shuffle,
                            repeat,
                        };
                        assert_eq!(
                            Prefs::parse(&prefs.render()),
                            Ok(prefs.clone()),
                            "a rendered file must parse back to the value it came from"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn default_prefs_render_the_documented_file() {
        assert_eq!(
            Prefs::default().render(),
            "volume=100\nmute=false\nshuffle=false\nrepeat=off\n",
            "the file is four fixed lines in a fixed order with a trailing newline"
        );
    }

    #[test]
    fn an_empty_text_is_the_defaults() {
        assert_eq!(
            Prefs::parse(""),
            Ok(Prefs::default()),
            "a missing value keeps its default"
        );
        assert_eq!(
            Prefs::parse("\n\n   \n"),
            Ok(Prefs::default()),
            "blank lines carry no values"
        );
    }

    #[test]
    fn a_partial_file_keeps_the_other_defaults() {
        let parsed = Prefs::parse("volume=80\n");

        assert_eq!(
            parsed,
            Ok(prefs(80)),
            "a file naming one key leaves the other three at their defaults"
        );
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let parsed = Prefs::parse("theme=dark\nvolume=35\nbalance=weird\nlater=1 0\nlater=2\n");

        assert_eq!(
            parsed,
            Ok(prefs(35)),
            "a key this version does not know is not a corrupt file"
        );
    }

    #[test]
    fn a_wrong_case_key_is_ignored_not_malformed() {
        assert_eq!(
            Prefs::parse("Volume=80\nREPEAT=sometimes\n"),
            Ok(Prefs::default()),
            "spellings are case-sensitive, so another case is an unknown key"
        );
    }

    #[test]
    fn crlf_bom_blank_lines_and_a_missing_trailing_newline_are_accepted() {
        let parsed = Prefs::parse("\u{feff}volume=80\r\n\r\n   \nmute=true\r\nrepeat=all");

        assert_eq!(
            parsed,
            Ok(Prefs {
                volume: 80,
                muted: true,
                repeat: Repeat::All,
                ..Prefs::default()
            }),
            "a BOM, CRLF line ends and a missing final newline are encoding, not edits"
        );
    }

    #[test]
    fn a_leading_zero_volume_reads_as_its_number() {
        assert_eq!(
            Prefs::parse("volume=080\n"),
            Ok(prefs(80)),
            "digits parse as a number, so leading zeros do not change it"
        );
        assert_eq!(
            Prefs::parse("volume=000\n"),
            Ok(prefs(0)),
            "and a run of zeros is zero"
        );
    }

    #[test]
    fn hash_lines_are_skipped() {
        let parsed = Prefs::parse("# note\nvolume=80\n  # indented\n#volume=999\nmute=true\n");

        assert_eq!(
            parsed,
            Ok(Prefs {
                volume: 80,
                muted: true,
                ..Prefs::default()
            }),
            "a comment is skipped whole, so a commented value is not applied"
        );
    }

    #[test]
    fn a_hash_line_without_equals_is_not_malformed() {
        assert_eq!(
            Prefs::parse("# my settings\n"),
            Ok(Prefs::default()),
            "a comment needs no `=`"
        );
    }

    #[test]
    fn refusals_name_the_line() {
        for value in ["101", "-1", "+5", "1e2", "", "8 0", "65536", " 80"] {
            let text = format!("mute=false\nvolume={value}\n");
            let refusal = Prefs::parse(&text).expect_err("an unreadable volume refuses the file");

            assert_eq!(refusal.line, 2, "the line is counted from one: {value:?}");
            assert_eq!(
                refusal.reason,
                Reason::BadValue(Key::Volume),
                "a volume is digits from 0 to 100: {value:?}"
            );
        }
    }

    #[test]
    fn a_space_around_a_known_key_is_refused_whole() {
        for line in ["volume = 80", "volume =80", " volume=80"] {
            let text = format!("mute=true\n{line}\n");
            let refusal = Prefs::parse(&text).expect_err("a padded key is an editing choice");

            assert_eq!(refusal.line, 2, "the refused line is named: {line:?}");
            assert_eq!(
                refusal.reason,
                Reason::SpaceAroundKey,
                "a space around a known key is refused, not trimmed: {line:?}"
            );
        }
    }

    #[test]
    fn a_space_around_an_unknown_key_is_ignored() {
        assert_eq!(
            Prefs::parse(" theme = dark\n"),
            Ok(Prefs::default()),
            "only a known key can be padded; an unknown key is ignored whatever it holds"
        );
    }

    #[test]
    fn bad_spellings_are_refused() {
        for (line, key) in [
            ("repeat=sometimes", Key::Repeat),
            ("repeat=Off", Key::Repeat),
            ("mute=yes", Key::Mute),
            ("shuffle=1", Key::Shuffle),
        ] {
            let text = format!("{line}\n");
            let refusal = Prefs::parse(&text).expect_err("a value outside the table is refused");

            assert_eq!(refusal.line, 1, "the refused line is named: {line:?}");
            assert_eq!(
                refusal.reason,
                Reason::BadValue(key),
                "the value must be spelled exactly as the table: {line:?}"
            );
        }
    }

    #[test]
    fn a_line_with_no_equals_is_refused() {
        let refusal = Prefs::parse("volume=80\nnonsense\n")
            .expect_err("a line that is not `key=value` is malformed");

        assert_eq!(refusal.line, 2, "the line without an `=` is named");
        assert_eq!(refusal.reason, Reason::NoEquals);
    }

    #[test]
    fn duplicate_known_keys_are_refused() {
        let refusal = Prefs::parse("volume=80\nvolume=60\n")
            .expect_err("two values for one key have no honest winner");

        assert_eq!(refusal.line, 2, "the second line is the duplicate");
        assert_eq!(refusal.reason, Reason::Duplicate(Key::Volume));
    }

    #[test]
    fn one_bad_value_applies_nothing() {
        let refusal = Prefs::parse("volume=80\nrepeat=sometimes\n")
            .expect_err("a file with one unreadable value is refused whole");

        assert_eq!(refusal.line, 2, "the unreadable line is named");
        assert_eq!(
            refusal.reason,
            Reason::BadValue(Key::Repeat),
            "the value at fault is named, and the readable line before it is dropped"
        );
    }

    #[test]
    fn a_refusal_never_prints_the_value() {
        let refusal =
            Prefs::parse("volume=secret-9\n").expect_err("a value outside the table is refused");

        assert_eq!(
            refusal.to_string(),
            "line 1: bad value for `volume`",
            "the message names the line and the key, and nothing else"
        );
        assert!(
            !refusal.to_string().contains("secret"),
            "the text the user wrote is never quoted back"
        );
    }

    #[test]
    fn each_reason_has_its_own_message() {
        for (reason, message) in [
            (Reason::NoEquals, "line 3: no `=`"),
            (Reason::SpaceAroundKey, "line 3: space around a key"),
            (
                Reason::BadValue(Key::Volume),
                "line 3: bad value for `volume`",
            ),
            (
                Reason::BadValue(Key::Repeat),
                "line 3: bad value for `repeat`",
            ),
            (Reason::Duplicate(Key::Mute), "line 3: `mute` appears twice"),
            (
                Reason::Duplicate(Key::Shuffle),
                "line 3: `shuffle` appears twice",
            ),
        ] {
            assert_eq!(
                refusal(3, reason).to_string(),
                message,
                "each reason spells its own sentence: {reason:?}"
            );
        }
    }

    #[test]
    fn the_split_is_at_the_first_equals() {
        assert_eq!(
            Prefs::parse("volume==80\n"),
            Err(refusal(1, Reason::BadValue(Key::Volume))),
            "`=80` is the value, so the line has an `=` and is not a duplicate"
        );
    }

    #[test]
    fn an_embedded_equals_belongs_to_the_value() {
        assert_eq!(
            Prefs::parse("repeat=off=x\n"),
            Err(refusal(1, Reason::BadValue(Key::Repeat))),
            "only the first `=` splits, so `off=x` is the whole value"
        );
    }

    #[test]
    fn a_bad_value_is_reported_before_a_duplicate() {
        assert_eq!(
            Prefs::parse("volume=80\nvolume=999\n"),
            Err(refusal(2, Reason::BadValue(Key::Volume))),
            "the unreadable value is named, not the repeat of the key"
        );
    }

    #[test]
    fn a_duplicate_is_detected_for_any_known_key() {
        for (text, key) in [
            ("mute=true\nmute=true\n", Key::Mute),
            ("shuffle=false\nshuffle=true\n", Key::Shuffle),
            ("repeat=all\nrepeat=one\n", Key::Repeat),
        ] {
            assert_eq!(
                Prefs::parse(text),
                Err(refusal(2, Reason::Duplicate(key))),
                "a second line for one key is refused whatever the key: {text:?}"
            );
        }
    }

    #[test]
    fn a_muted_zero_reads_the_same_in_either_order() {
        let mute_first = Prefs::parse("mute=true\nvolume=0\n");
        let volume_first = Prefs::parse("volume=0\nmute=true\n");

        assert_eq!(
            mute_first,
            Ok(prefs(0)),
            "the zero silences the mute wherever it is listed"
        );
        assert_eq!(
            mute_first, volume_first,
            "the file is a set of values, so its order does not change the result"
        );
    }

    #[test]
    fn muted_zero_normalises_to_unmuted() {
        assert_eq!(
            Prefs::parse("volume=0\nmute=true\n"),
            Ok(prefs(0)),
            "a mute at zero has nothing to silence, so it is not a mute"
        );
    }

    #[test]
    fn prefs_of_records_a_muted_zero_as_unmuted() {
        let volume = Volume::muted_at(0);
        let queue = Queue::seeded(1);
        let saved = Prefs::of(&volume, &queue);

        assert_eq!(saved, prefs(0), "a mute at zero has nothing to silence");
        assert_eq!(
            Prefs::parse(&saved.render()),
            Ok(saved.clone()),
            "what `of` records is what `parse` reads back"
        );
    }

    #[test]
    fn prefs_of_reads_a_volume_and_a_queue() {
        let volume = Volume::muted_at(70);
        let mut queue = Queue::seeded(7);
        queue.restore_modes(Repeat::One, true);

        assert_eq!(
            Prefs::of(&volume, &queue),
            Prefs {
                volume: 70,
                muted: true,
                shuffle: true,
                repeat: Repeat::One,
            },
            "the file records the level behind the lid and the queue's modes"
        );
    }

    #[test]
    fn prefs_volume_and_seed_queue_reproduce_the_saved_state() {
        let saved = Prefs {
            volume: 40,
            muted: true,
            shuffle: true,
            repeat: Repeat::All,
        };

        let volume = saved.volume();
        assert_eq!(volume.level(), 40, "the saved level comes back");
        assert!(volume.muted(), "and the saved lid with it");

        let mut queue = Queue::seeded(3);
        saved.seed_queue(&mut queue);
        assert!(queue.shuffle(), "the saved shuffle comes back");
        assert_eq!(queue.repeat(), Repeat::All, "and the saved repeat mode");
    }

    #[test]
    fn set_fraction_zero_while_muted_is_a_prefs_change() {
        let mut volume = Volume::muted_at(70);
        let queue = Queue::seeded(1);
        let before = Prefs::of(&volume, &queue);

        assert!(!volume.set_fraction(0.0), "the sound was already silent");
        assert_eq!(
            volume.take(),
            None,
            "silence to silence owes the player no command"
        );

        let after = Prefs::of(&volume, &queue);
        assert_ne!(
            before, after,
            "the file still remembers the level and the lid it had"
        );
        assert_eq!(after, prefs(0), "the saved value is the zero, unmuted");
    }

    #[test]
    fn an_untouched_settle_arms_nothing() {
        let written = prefs(100);
        let mut settle = Settle::new(written.clone());
        let base = Instant::now();

        assert_eq!(
            settle.observe(base, &written),
            None,
            "a value equal to the written one is not a change"
        );
        assert_eq!(
            settle.wake(base),
            None,
            "an untouched session has no timer to wake for"
        );

        for step in 1..=10 {
            let now = base + QUIET * step;
            assert_eq!(settle.observe(now, &written), None);
            assert_eq!(
                settle.wake(now),
                None,
                "time passing alone must never arm a write"
            );
        }
    }

    #[test]
    fn a_drag_of_sixty_frames_writes_nothing_then_once() {
        let mut settle = Settle::new(prefs(100));
        let base = Instant::now();
        let mut last = prefs(100);

        for frame in 0..60u32 {
            let now = base + Duration::from_millis(16) * frame;
            last = prefs(100 - frame as u16);
            assert_eq!(
                settle.observe(now, &last),
                None,
                "no write happens mid-drag, frame {frame}"
            );
        }

        let quiet = base + Duration::from_millis(16) * 59 + QUIET;
        assert_eq!(
            settle.observe(quiet, &last),
            Some(last.clone()),
            "the value at rest is written once"
        );
    }

    #[test]
    fn returning_to_the_written_value_cancels() {
        let written = prefs(100);
        let mut settle = Settle::new(written.clone());
        let base = Instant::now();
        let dragged = prefs(40);

        assert_eq!(settle.observe(base, &dragged), None);
        let back = base + Duration::from_millis(100);
        assert_eq!(
            settle.observe(back, &written),
            None,
            "a value that returns to the written one cancels its pending write"
        );
        assert_eq!(settle.wake(back), None, "and leaves no timer behind");
        assert_eq!(
            settle.observe(back + QUIET, &written),
            None,
            "the cancelled value is never written later"
        );
    }

    #[test]
    fn a_repeated_observe_after_a_write_returns_none() {
        let mut settle = Settle::new(prefs(100));
        let base = Instant::now();
        let changed = prefs(40);

        assert_eq!(settle.observe(base, &changed), None);
        assert_eq!(
            settle.observe(base + QUIET, &changed),
            Some(changed.clone())
        );
        assert_eq!(
            settle.observe(base + QUIET, &changed),
            None,
            "a value already written is not written again"
        );
        assert_eq!(
            settle.observe(base + QUIET * 2, &changed),
            None,
            "and staying on it keeps it written"
        );
    }

    #[test]
    fn a_change_after_a_write_starts_a_new_cycle() {
        let mut settle = Settle::new(prefs(100));
        let base = Instant::now();
        let first = prefs(40);
        let second = prefs(20);

        assert_eq!(settle.observe(base, &first), None);
        assert_eq!(settle.observe(base + QUIET, &first), Some(first.clone()));

        let changed_at = base + QUIET + Duration::from_millis(10);
        assert_eq!(
            settle.observe(changed_at, &second),
            None,
            "a change after a write restarts the quiet clock"
        );
        assert_eq!(settle.wake(changed_at), Some(QUIET), "the window is ahead");
        assert_eq!(
            settle.observe(changed_at + QUIET - Duration::from_millis(1), &second),
            None,
            "one millisecond short of quiet is not quiet"
        );
        assert_eq!(
            settle.observe(changed_at + QUIET, &second),
            Some(second.clone()),
            "the new value writes after its own quiet window"
        );
    }

    #[test]
    fn a_pause_longer_than_quiet_mid_drag_writes_once_per_pause() {
        let mut settle = Settle::new(prefs(100));
        let base = Instant::now();
        let first = prefs(90);
        let second = prefs(50);

        assert_eq!(settle.observe(base, &first), None);
        assert_eq!(
            settle.observe(base + QUIET, &first),
            Some(first.clone()),
            "a mid-drag pause is a rest, and a rest is written"
        );

        let resumed = base + QUIET + Duration::from_millis(16);
        assert_eq!(settle.observe(resumed, &second), None);
        assert_eq!(
            settle.observe(resumed + QUIET, &second),
            Some(second.clone()),
            "the resumed drag writes once more when it rests again"
        );
    }

    #[test]
    fn wake_gives_the_remaining_time_then_none_after_the_write() {
        let mut settle = Settle::new(prefs(100));
        let base = Instant::now();
        let changed = prefs(40);

        assert_eq!(settle.observe(base, &changed), None);
        assert_eq!(
            settle.wake(base),
            Some(QUIET),
            "a fresh candidate has the whole window ahead"
        );
        assert_eq!(
            settle.wake(base + Duration::from_millis(150)),
            Some(QUIET - Duration::from_millis(150)),
            "a wake names what is left of the window"
        );
        assert_eq!(
            settle.wake(base + QUIET),
            Some(Duration::ZERO),
            "a window that has run out asks for no wait"
        );
        assert_eq!(
            settle.wake(base + QUIET * 2),
            Some(Duration::ZERO),
            "and a late wake still asks for nothing"
        );

        assert_eq!(
            settle.observe(base + QUIET, &changed),
            Some(changed.clone())
        );
        assert_eq!(
            settle.wake(base + QUIET),
            None,
            "a written value has no timer"
        );
    }

    #[test]
    fn flush_returns_the_unsettled_value_and_marks_it_written() {
        let mut settle = Settle::new(prefs(100));
        let base = Instant::now();
        let changed = prefs(40);

        assert_eq!(settle.observe(base, &changed), None);
        assert_eq!(
            settle.flush(&changed),
            Some(changed.clone()),
            "closing writes the value that had not settled yet"
        );
        assert_eq!(
            settle.observe(base + QUIET, &changed),
            None,
            "the flushed value is written, so it is not written again"
        );
    }

    #[test]
    fn flush_returns_none_when_nothing_differs() {
        let written = prefs(100);
        let mut settle = Settle::new(written.clone());

        assert_eq!(
            settle.flush(&written),
            None,
            "an untouched session has nothing to flush"
        );
    }

    #[test]
    fn flush_clears_the_pending_candidate() {
        let mut settle = Settle::new(prefs(100));
        let base = Instant::now();
        let changed = prefs(40);

        assert_eq!(settle.observe(base, &changed), None);
        assert_eq!(settle.flush(&changed), Some(changed.clone()));
        assert_eq!(
            settle.wake(base + Duration::from_millis(10)),
            None,
            "no candidate survives the flush"
        );
        let back = base + QUIET;
        assert_eq!(
            settle.observe(back, &prefs(100)),
            None,
            "the value flushed away is a change again, not a silent no-op"
        );
        assert_eq!(
            settle.wake(back),
            Some(QUIET),
            "and it arms a window of its own"
        );
    }

    #[test]
    fn flush_twice_is_idempotent() {
        let mut settle = Settle::new(prefs(100));
        let base = Instant::now();
        let changed = prefs(40);

        assert_eq!(settle.observe(base, &changed), None);
        assert_eq!(settle.flush(&changed), Some(changed.clone()));
        assert_eq!(
            settle.flush(&changed),
            None,
            "a second close finds nothing left to write"
        );
    }
}
