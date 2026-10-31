//! Finding the preferences file, and deciding when it is written.
//!
//! The path comes from the platform's configuration directory; with no directory
//! the app runs on the defaults and writes nothing. A file this version cannot
//! read is refused whole, so a bad value never half-applies. The bytes go
//! through [`crate::small_file`], which bounds the read and replaces the file
//! atomically.
//!
//! Writes happen on a `takofy-prefs` thread fed by the settled value, so the
//! drawing thread never waits on the filesystem. A frame calls
//! [`PrefsSync::observe`] and then uses its duration as the repaint delay, in
//! that order: the wake reports the window for the value `observe` last saw.
//! Closing waits a bounded time for the thread and then detaches it.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use log::warn;
use takofy_ui::prefs::{Prefs, Settle};

use crate::latest_writer::LatestWriter;
use crate::small_file::{self, Read};

/// The directory under the platform's configuration root.
const DIR: &str = "takofy";
/// The file's name inside that directory.
const FILE: &str = "preferences.conf";
/// The most bytes a preferences file may have.
const LIMIT: u64 = 64 * 1024;
/// How long closing waits for the writer thread before detaching it.
const FLUSH_WAIT: Duration = Duration::from_secs(2);
/// The writer thread's name, which is also how a warning says who it is.
const THREAD: &str = "takofy-prefs";

/// What the file held when the app started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Loaded {
    /// There is no file yet.
    Absent,
    /// The file parsed.
    Found(Prefs),
    /// The file was refused, with the reason to warn about.
    Refused(String),
    /// The file could not be read.
    Unreadable,
}

/// The preferences file for this platform, or `None` when the platform names no
/// configuration directory.
///
/// `windows` is a parameter rather than a compile-time branch so both paths are
/// testable from one host; `var` is the environment.
pub fn config_file(windows: bool, var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if windows {
        let appdata = var("APPDATA").filter(|value| !value.is_empty())?;
        return Some(PathBuf::from(appdata).join(DIR).join(FILE));
    }
    if let Some(xdg) = var("XDG_CONFIG_HOME") {
        let path = PathBuf::from(xdg);
        if !path.as_os_str().is_empty() && path.is_absolute() {
            return Some(path.join(DIR).join(FILE));
        }
    }
    let home = var("HOME").filter(|value| !value.is_empty())?;
    Some(PathBuf::from(home).join(".config").join(DIR).join(FILE))
}

/// Reads `path`, refusing it whole when any line cannot be read.
pub fn load(path: &Path) -> Loaded {
    match small_file::read_text(path, LIMIT) {
        Read::Absent => Loaded::Absent,
        Read::Unreadable => Loaded::Unreadable,
        Read::TooLarge => Loaded::Refused("file too large".to_owned()),
        Read::Text(text) => match Prefs::parse(&text) {
            Ok(prefs) => Loaded::Found(prefs),
            Err(refusal) => Loaded::Refused(refusal.to_string()),
        },
    }
}

/// Renders `prefs` and replaces `path` with it atomically.
pub fn write_atomic(path: &Path, prefs: &Prefs) -> io::Result<()> {
    small_file::write(path, &prefs.render())
}

/// The thread that saves settled preferences, coalescing a burst into the last
/// one.
pub struct PrefsWriter {
    writer: LatestWriter<Prefs>,
}

impl PrefsWriter {
    /// Starts the writer whose sink saves to `path`, or `None` when the thread
    /// cannot start.
    pub fn start(path: &Path) -> Option<Self> {
        let path = path.to_path_buf();
        LatestWriter::start(THREAD, move |prefs| write_atomic(&path, prefs), FLUSH_WAIT)
            .map(|writer| Self { writer })
    }

    /// Starts a writer over `sink`, waiting `wait` for the thread at close.
    #[cfg(test)]
    pub fn with_sink(
        sink: impl Fn(&Prefs) -> io::Result<()> + Send + 'static,
        wait: Duration,
    ) -> Option<Self> {
        LatestWriter::start(THREAD, sink, wait).map(|writer| Self { writer })
    }

    fn send(&self, prefs: Prefs) {
        self.writer.send(prefs);
    }
}

/// Sends settled values to the writer thread.
///
/// A frame calls [`PrefsSync::observe`] with the values it is showing and then
/// uses the returned duration as its repaint delay. The order matters: the wake
/// reports the window for the value `observe` last saw.
pub struct PrefsSync {
    settle: Settle,
    writer: Option<PrefsWriter>,
}

impl PrefsSync {
    /// A sync that writes nothing, for when no directory names a file.
    pub fn off() -> Self {
        Self {
            settle: Settle::new(Prefs::default()),
            writer: None,
        }
    }

    /// A sync whose file already holds `written`, saving through `writer`.
    #[cfg(test)]
    pub fn for_test(written: Prefs, writer: Option<PrefsWriter>) -> Self {
        Self {
            settle: Settle::new(written),
            writer,
        }
    }

    /// Takes what the app holds now and returns how long until it has been quiet
    /// long enough to write, or `None` when nothing is pending.
    pub fn observe(&mut self, now: Instant, current: &Prefs) -> Option<Duration> {
        let settled = self.settle.observe(now, current);
        let wake = self.settle.wake(now);
        if let Some(prefs) = settled {
            self.send(prefs);
        }
        wake
    }

    /// Writes whatever differs now, whatever its quiet time: the window is going
    /// away and there is no next frame.
    pub fn close(&mut self, current: &Prefs) {
        if let Some(prefs) = self.settle.flush(current) {
            self.send(prefs);
        }
    }

    fn send(&mut self, prefs: Prefs) {
        if let Some(writer) = &self.writer {
            writer.send(prefs);
        }
    }
}

/// Reads the file once and starts the writer that saves later changes.
///
/// The result is what the file held and the sync that will save later changes.
/// A missing directory, a refusal and an unreadable file each log one warning.
pub fn open() -> (Loaded, PrefsSync) {
    let Some(path) = config_file(cfg!(windows), |key| std::env::var_os(key)) else {
        warn!("no configuration directory: preferences will not be saved");
        return (Loaded::Absent, PrefsSync::off());
    };
    open_at(&path, PrefsWriter::start)
}

/// Reads `path` and starts a writer over it through `start`.
///
/// The sync's baseline is the loaded value, or the defaults when there is none,
/// so an untouched session writes nothing and a refused or unreadable file is
/// left alone until a value settles; the first settled change then replaces it
/// whole.
fn open_at(path: &Path, start: impl FnOnce(&Path) -> Option<PrefsWriter>) -> (Loaded, PrefsSync) {
    let loaded = load(path);
    let baseline = match &loaded {
        Loaded::Found(prefs) => prefs.clone(),
        Loaded::Refused(reason) => {
            warn!("preferences file refused: {reason}");
            Prefs::default()
        }
        Loaded::Unreadable => {
            warn!("preferences file unreadable");
            Prefs::default()
        }
        Loaded::Absent => Prefs::default(),
    };
    let writer = start(path);
    (
        loaded,
        PrefsSync {
            settle: Settle::new(baseline),
            writer,
        },
    )
}

#[cfg(test)]
mod tests;
