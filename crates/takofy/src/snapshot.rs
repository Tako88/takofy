//! The cache file the window draws its library from before the network answers.
//!
//! The path comes from the platform's cache directory; with none the app saves
//! nothing and draws from the network as it always did. The bytes go through
//! [`crate::small_file`], which bounds the read and replaces the file
//! atomically, and the grammar is [`crate::snapshot_format`], which refuses a
//! copy it cannot read whole. A missing, unreadable or refused file is a miss,
//! not a partial draw.
//!
//! Writing waits for quiet: a refresh changes a list many times in a second and
//! the file should see one copy, not sixty. The clone that builds a copy happens
//! once per quiet window, on the UI thread; rendering and writing happen on a
//! `takofy-snapshot` thread, so the drawing thread never waits on the
//! filesystem. Closing only hands the last copy over; the bounded wait for that
//! thread happens when the sync is dropped.
//!
//! The copy belongs to one account, and that key is the account line alone:
//! playlist names and owner names are stored verbatim, by design.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use log::warn;
use takofy_ui::library::Snapshot;

use crate::latest_writer::LatestWriter;
use crate::small_file::{self, Read};
use crate::snapshot_format;

/// The directory under the platform's cache root.
const DIR: &str = "takofy";
/// The file's name inside that directory.
const FILE: &str = "library.snapshot";
/// How long the lists must be quiet before the copy is written.
pub const QUIET: Duration = Duration::from_secs(2);
/// The most bytes a copy may have, both read and written.
///
/// A library larger than this is not cached: the next launch would refuse it
/// anyway, and a file nobody reads is not worth keeping.
pub const LIMIT: u64 = 8 * 1024 * 1024;
/// How long closing waits for the writer thread before detaching it.
const FLUSH_WAIT: Duration = Duration::from_secs(2);
/// The writer thread's name, which is also how a warning says who it is.
const THREAD: &str = "takofy-snapshot";

/// What the file held when the app started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cached {
    /// There is no copy yet.
    Absent,
    /// The copy parsed.
    Found(Snapshot),
    /// The copy was refused, with the reason to warn about.
    Refused(String),
    /// The copy could not be read.
    Unreadable,
}

/// The cache file for this platform, or `None` when the platform names no cache
/// directory.
///
/// `windows` is a parameter rather than a compile-time branch so both paths are
/// testable from one host; `var` is the environment.
pub fn cache_file(windows: bool, var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if windows {
        let local = var("LOCALAPPDATA").filter(|value| !value.is_empty())?;
        return Some(PathBuf::from(local).join(DIR).join(FILE));
    }
    if let Some(xdg) = var("XDG_CACHE_HOME") {
        let path = PathBuf::from(xdg);
        if !path.as_os_str().is_empty() && path.is_absolute() {
            return Some(path.join(DIR).join(FILE));
        }
    }
    let home = var("HOME").filter(|value| !value.is_empty())?;
    Some(PathBuf::from(home).join(".cache").join(DIR).join(FILE))
}

/// Reads `path`, refusing it whole when any line cannot be read.
pub fn load(path: &Path) -> Cached {
    match small_file::read_text(path, LIMIT) {
        Read::Absent => Cached::Absent,
        Read::Unreadable => Cached::Unreadable,
        Read::TooLarge => Cached::Refused("file too large".to_owned()),
        Read::Text(text) => match snapshot_format::parse(&text) {
            Ok(snapshot) => Cached::Found(snapshot),
            Err(refusal) => Cached::Refused(refusal.to_string()),
        },
    }
}

/// Renders `snapshot` and replaces `path` with it atomically.
///
/// A copy over [`LIMIT`] is not written at all: the old file is removed, so the
/// next launch starts from the network instead of from a copy that would not be
/// read.
pub fn write(path: &Path, snapshot: &Snapshot) -> io::Result<()> {
    let text = snapshot_format::render(snapshot);
    if text.len() as u64 > LIMIT {
        let _ = fs::remove_file(path);
        return Ok(());
    }
    small_file::write(path, &text)
}

/// The thread that saves settled copies, coalescing a burst into the last one.
pub struct SnapshotWriter {
    writer: LatestWriter<Snapshot>,
    /// Set when the last write failed and cleared when one succeeds.
    failed: Arc<AtomicBool>,
}

impl SnapshotWriter {
    /// Starts the writer whose sink saves to `path`, or `None` when the thread
    /// cannot start.
    pub fn start(path: &Path) -> Option<Self> {
        let path = path.to_path_buf();
        Self::over(
            move |snapshot: &Snapshot| write(&path, snapshot),
            FLUSH_WAIT,
        )
    }

    /// Starts a writer over `sink`, waiting `wait` for the thread at close.
    #[cfg(test)]
    pub fn with_sink(
        sink: impl Fn(&Snapshot) -> io::Result<()> + Send + 'static,
        wait: Duration,
    ) -> Option<Self> {
        Self::over(sink, wait)
    }

    /// Starts a writer over `sink`, noting whether the last write landed.
    fn over(
        sink: impl Fn(&Snapshot) -> io::Result<()> + Send + 'static,
        wait: Duration,
    ) -> Option<Self> {
        let failed = Arc::new(AtomicBool::new(false));
        let warned = AtomicBool::new(false);
        let sink = {
            let failed = Arc::clone(&failed);
            move |snapshot: &Snapshot| {
                let result = sink(snapshot);
                failed.store(result.is_err(), Ordering::Relaxed);
                // A cache directory that refuses one write refuses the next one
                // for the same reason, so the run hears about it once.
                if let Err(error) = &result
                    && !warned.swap(true, Ordering::Relaxed)
                {
                    warn!("snapshot write failed: {:?}", error.kind());
                }
                Ok(())
            }
        };
        LatestWriter::start(THREAD, sink, wait).map(|writer| Self { writer, failed })
    }

    fn send(&self, snapshot: Snapshot) {
        self.writer.send(snapshot);
    }
}

/// Decides when the copy is written, and writes it.
///
/// A frame calls [`SnapshotSync::observe`] with the library's generation and
/// then uses the returned duration as its repaint delay, in that order: the
/// wake reports the window for the generation `observe` last saw. Closing
/// writes whatever differs now, because there is no next frame.
pub struct SnapshotSync {
    /// The file's text for the copy it holds, or `None` when it holds none this
    /// build trusts.
    written: Option<String>,
    /// The library generation `observe` last saw.
    generation: u64,
    /// When the lists last changed, once they have been quiet enough to write.
    since: Option<Instant>,
    writer: Option<SnapshotWriter>,
}

impl SnapshotSync {
    /// A sync that saves nothing, for when no directory names a file.
    pub fn off() -> Self {
        Self {
            written: None,
            generation: 0,
            since: None,
            writer: None,
        }
    }

    /// A sync over a writer, whose file already holds `written`.
    /// Test-only: the window builds its sync through [`open`].
    #[cfg(test)]
    pub fn for_test(written: Option<Snapshot>, writer: Option<SnapshotWriter>) -> Self {
        Self {
            written: written.as_ref().map(snapshot_format::render),
            generation: 0,
            since: None,
            writer,
        }
    }

    /// Notes how many times the lists have changed and returns how long until
    /// they have been quiet long enough to write, or `None` when nothing is
    /// pending.
    ///
    /// `build` hands over the copy to save, or `None` when there is nothing
    /// worth saving yet. A deferred build waits for the next generation change:
    /// the account update and every list reply bump it, and the bump that ends
    /// a write re-arms the timer too.
    pub fn observe(
        &mut self,
        now: Instant,
        generation: u64,
        build: impl FnOnce() -> Option<Snapshot>,
    ) -> Option<Duration> {
        self.writer.as_ref()?;
        self.forget_failed_write();
        if generation != self.generation {
            self.generation = generation;
            self.since = Some(now);
            return Some(QUIET);
        }
        let since = self.since?;
        let wait = QUIET.saturating_sub(now.saturating_duration_since(since));
        if !wait.is_zero() {
            return Some(wait);
        }
        self.since = None;
        if let Some(snapshot) = build() {
            self.save(snapshot);
        }
        None
    }

    /// Writes whatever differs now, whatever its quiet time: the window is going
    /// away and there is no next frame.
    pub fn close(&mut self, build: impl FnOnce() -> Option<Snapshot>) {
        self.since = None;
        if self.writer.is_none() {
            return;
        }
        self.forget_failed_write();
        if let Some(snapshot) = build() {
            self.save(snapshot);
        }
    }

    /// Forgets the copy the file holds when the last write failed, so the next
    /// compare hands it over again.
    fn forget_failed_write(&mut self) {
        let Some(writer) = &self.writer else {
            return;
        };
        if writer.failed.swap(false, Ordering::Relaxed) {
            self.written = None;
        }
    }

    /// Hands `snapshot` to the writer unless the file's text already holds it.
    fn save(&mut self, snapshot: Snapshot) {
        let text = snapshot_format::render(&snapshot);
        if self.written.as_ref() == Some(&text) {
            return;
        }
        self.written = Some(text);
        if let Some(writer) = &self.writer {
            writer.send(snapshot);
        }
    }
}

/// Reads the file once and starts the writer that saves later copies.
///
/// A missing file is silent: no copy is what a first launch looks like. A
/// refusal and an unreadable file each log one warning, and the next copy
/// replaces them whole.
pub fn open() -> (Cached, SnapshotSync) {
    open_at(
        cache_file(cfg!(windows), |key| std::env::var_os(key)),
        SnapshotWriter::start,
    )
}

/// Reads `path` and starts a writer over it through `start`.
fn open_at(
    path: Option<PathBuf>,
    start: impl FnOnce(&Path) -> Option<SnapshotWriter>,
) -> (Cached, SnapshotSync) {
    let Some(path) = path else {
        warn!("no cache directory: the library will not be saved");
        return (Cached::Absent, SnapshotSync::off());
    };
    let cached = load(&path);
    let written = match &cached {
        Cached::Found(snapshot) => Some(snapshot_format::render(snapshot)),
        Cached::Refused(reason) => {
            warn!("library snapshot refused: {reason}");
            None
        }
        Cached::Unreadable => {
            warn!("library snapshot unreadable");
            None
        }
        Cached::Absent => None,
    };
    let sync = SnapshotSync {
        written,
        generation: 0,
        since: None,
        writer: start(&path),
    };
    (cached, sync)
}

#[cfg(test)]
mod tests;
