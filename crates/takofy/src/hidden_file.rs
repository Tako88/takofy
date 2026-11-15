//! The file that holds the hidden artists and the positions the three rows start
//! at, kept beside `preferences.conf` in the configuration directory.
//!
//! The text is the grammar in `takofy_ui::home::discovery::hidden`; this module
//! only finds the file, reads it whole and replaces it atomically through
//! [`crate::small_file`]. A file this version cannot read is refused whole, and
//! the one warning about it never prints a value. Writes happen on a
//! `takofy-hidden` thread fed by the set's revision, so the drawing thread never
//! waits on the filesystem. A refused file starts the session with an empty set,
//! and the first change then replaces the file whole, as `preferences.rs` does.

use std::io;
use std::path::Path;
use std::time::Duration;

use log::warn;
use takofy_ui::home::discovery::hidden::{HIDE_MAX, Hidden};

use crate::latest_writer::LatestWriter;
use crate::preferences;
use crate::small_file::{self, Read};

/// The file's name inside the configuration directory.
const FILE: &str = "hidden.conf";
/// The most bytes the hidden file may have.
const LIMIT: u64 = 512 * 1024;
/// One hidden artist written at the longest address the grammar carries, with
/// its key and its newline.
const ENTRY_MAX: usize = "hide=spotify:artist:".len() + 22 + 1;
/// Every artist the set can hide still fits in the file it is read from.
const _: () = assert!(HIDE_MAX * ENTRY_MAX < LIMIT as usize);
/// How long closing waits for the writer thread before detaching it.
const FLUSH_WAIT: Duration = Duration::from_secs(2);
/// The writer thread's name, which is also how a warning says who it is.
const THREAD: &str = "takofy-hidden";
/// What a platform with no configuration directory is told.
const NO_DIRECTORY: &str = "no configuration directory: hidden artists will not be saved";

/// What the file held when the app started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Loaded {
    /// There is no file yet.
    Absent,
    /// The file parsed.
    Found(Hidden),
    /// The file was refused, with the reason to warn about.
    Refused(String),
    /// The file could not be read.
    Unreadable,
}

/// The one line a warning says about what the file held, or `None` when there is
/// nothing to say.
///
/// The reason a parse gives names its line and its key, never the text it could
/// not read, so no address reaches the log.
fn warning(loaded: &Loaded) -> Option<String> {
    match loaded {
        Loaded::Refused(reason) => Some(format!("hidden file refused: {reason}")),
        Loaded::Unreadable => Some("hidden file unreadable".to_owned()),
        Loaded::Absent | Loaded::Found(_) => None,
    }
}

/// Reads `path`, refusing it whole when any line cannot be read.
pub fn load(path: &Path) -> Loaded {
    match small_file::read_text(path, LIMIT) {
        Read::Absent => Loaded::Absent,
        Read::Unreadable => Loaded::Unreadable,
        Read::TooLarge => Loaded::Refused("file too large".to_owned()),
        Read::Text(text) => match Hidden::parse(&text) {
            Ok(hidden) => Loaded::Found(hidden),
            Err(refusal) => Loaded::Refused(refusal.to_string()),
        },
    }
}

/// Renders `hidden` and replaces `path` with it atomically.
pub fn write_atomic(path: &Path, hidden: &Hidden) -> io::Result<()> {
    small_file::write(path, &hidden.render())
}

/// The thread that saves the hidden set, coalescing a burst into the last one.
pub struct HiddenWriter {
    writer: LatestWriter<Hidden>,
}

impl HiddenWriter {
    /// Starts the writer whose sink saves to `path`, or `None` when the thread
    /// cannot start.
    pub fn start(path: &Path) -> Option<Self> {
        let path = path.to_path_buf();
        LatestWriter::start(
            THREAD,
            move |hidden| write_atomic(&path, hidden),
            FLUSH_WAIT,
        )
        .map(|writer| Self { writer })
    }

    /// Starts a writer over `sink`, waiting `wait` for the thread at close.
    #[cfg(test)]
    pub fn with_sink(
        sink: impl Fn(&Hidden) -> io::Result<()> + Send + 'static,
        wait: Duration,
    ) -> Option<Self> {
        LatestWriter::start(THREAD, sink, wait).map(|writer| Self { writer })
    }

    fn send(&self, hidden: Hidden) {
        self.writer.send(hidden);
    }
}

/// Sends the hidden set to the writer thread when it changed.
///
/// A position-only change never replaces a file this session could not read: up
/// to [`HIDE_MAX`] artists would go with it. There the writer is armed by a set
/// change alone, which is a hide or an unhide and nothing else.
pub struct HiddenSync {
    revision: u64,
    set_revision: u64,
    /// Whether the file we loaded holds every artist the set knows.
    intact: bool,
    writer: Option<HiddenWriter>,
}

impl HiddenSync {
    /// A sync that writes nothing, for when no directory names a file.
    pub fn off() -> Self {
        Self {
            revision: 0,
            set_revision: 0,
            intact: true,
            writer: None,
        }
    }

    /// A sync whose file already holds `revision`, saving through `writer`.
    #[cfg(test)]
    pub fn for_test(revision: u64, writer: Option<HiddenWriter>) -> Self {
        Self {
            revision,
            set_revision: revision,
            intact: true,
            writer,
        }
    }

    /// Sends the set when its revision differs from the last one sent.
    pub fn observe(&mut self, hidden: &Hidden) {
        self.send(hidden);
    }

    /// Sends a change the frames have not sent yet: the window is going away and
    /// there is no next frame.
    pub fn close(&mut self, hidden: &Hidden) {
        self.send(hidden);
    }

    fn send(&mut self, hidden: &Hidden) {
        // A set change arms the writer whatever the file held; a readable file
        // also records where its rows start.
        let set_changed = hidden.set_revision() != self.set_revision;
        if !set_changed && (hidden.revision() == self.revision || !self.intact) {
            return;
        }
        self.revision = hidden.revision();
        self.set_revision = hidden.set_revision();
        if let Some(writer) = &self.writer {
            writer.send(hidden.clone());
        }
    }
}

/// Reads the file once and starts the writer that saves later changes.
///
/// The result is what the file held and the sync that will save later changes. A
/// missing directory, a refusal and an unreadable file each log one warning.
pub fn open() -> (Loaded, HiddenSync) {
    let Some(dir) = preferences::config_dir(cfg!(windows), |key| std::env::var_os(key)) else {
        warn!("{NO_DIRECTORY}");
        return (Loaded::Absent, HiddenSync::off());
    };
    open_at(&dir.join(FILE), HiddenWriter::start)
}

/// Reads `path` and starts a writer over it through `start`.
///
/// The sync's baseline is the revision the loaded set already carries, so an
/// untouched session writes nothing. A refused or unreadable file is not intact,
/// so only a hide or an unhide replaces it; a readable one also records where
/// its rows start.
fn open_at(path: &Path, start: impl FnOnce(&Path) -> Option<HiddenWriter>) -> (Loaded, HiddenSync) {
    let loaded = load(path);
    if let Some(line) = warning(&loaded) {
        warn!("{line}");
    }
    let (revision, set_revision, intact) = match &loaded {
        Loaded::Found(hidden) => (hidden.revision(), hidden.set_revision(), true),
        Loaded::Absent => (0, 0, true),
        Loaded::Refused(_) | Loaded::Unreadable => (0, 0, false),
    };
    let writer = start(path);
    (
        loaded,
        HiddenSync {
            revision,
            set_revision,
            intact,
            writer,
        },
    )
}

#[cfg(test)]
mod tests;
