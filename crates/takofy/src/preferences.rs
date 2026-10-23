//! Finding, reading and writing the preferences file.
//!
//! This is the only module that touches the filesystem. The path comes from the
//! platform's configuration directory; with no directory the app runs on the
//! defaults and writes nothing. A file this version cannot read is refused
//! whole, so a bad value never half-applies.
//!
//! Writes happen on a `takofy-prefs` thread fed by the settled value, so the
//! drawing thread never waits on the filesystem. A frame calls
//! [`PrefsSync::observe`] and then uses its duration as the repaint delay, in
//! that order: the wake reports the window for the value `observe` last saw.
//! Closing waits a bounded time for the thread and then detaches it.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use log::warn;
use takofy_ui::prefs::{Prefs, Settle};

/// The directory under the platform's configuration root.
const DIR: &str = "takofy";
/// The file's name inside that directory.
const FILE: &str = "preferences.conf";
/// The most bytes a preferences file may have.
const LIMIT: u64 = 64 * 1024;
/// How long closing waits for the writer thread before detaching it.
const FLUSH_WAIT: Duration = Duration::from_secs(2);

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
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Loaded::Absent,
        Err(_) => return Loaded::Unreadable,
    };
    if !meta.is_file() {
        return Loaded::Unreadable;
    }
    let file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return Loaded::Unreadable,
    };
    let mut bytes = Vec::new();
    if file.take(LIMIT + 1).read_to_end(&mut bytes).is_err() {
        return Loaded::Unreadable;
    }
    if bytes.len() as u64 > LIMIT {
        return Loaded::Refused("file too large".to_owned());
    }
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => return Loaded::Unreadable,
    };
    match Prefs::parse(&text) {
        Ok(prefs) => Loaded::Found(prefs),
        Err(refusal) => Loaded::Refused(refusal.to_string()),
    }
}

/// Writes `prefs` to `path` through a temporary file beside it, so a reader
/// never sees half a file. The temporary name is per process, so two writers in
/// one process would collide; production has exactly one writer thread.
pub fn write_atomic(path: &Path, prefs: &Prefs) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".tmp.{}", std::process::id()));
    let temp = PathBuf::from(name);
    let written = write_then_rename(&temp, path, prefs);
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

fn write_then_rename(temp: &Path, path: &Path, prefs: &Prefs) -> io::Result<()> {
    let mut file = File::create(temp)?;
    file.write_all(prefs.render().as_bytes())?;
    drop(file);
    fs::rename(temp, path)
}

/// Signals that the writer thread has ended, whether the sink finished, panicked
/// or the channel closed.
struct Done(mpsc::Sender<()>);

impl Drop for Done {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

/// The thread that writes settled values, coalescing a burst into the last one.
pub struct PrefsWriter {
    values: Option<mpsc::Sender<Prefs>>,
    done: mpsc::Receiver<()>,
    handle: Option<JoinHandle<()>>,
    wait: Duration,
}

impl PrefsWriter {
    /// Starts the writer whose sink saves to `path`, or `None` when the thread
    /// cannot start.
    pub fn start(path: &Path) -> Option<Self> {
        let path = path.to_path_buf();
        Self::spawn(move |prefs| write_atomic(&path, prefs), FLUSH_WAIT)
    }

    /// Starts a writer over `sink`, waiting `wait` for the thread at close.
    #[cfg(test)]
    pub fn with_sink(
        sink: impl Fn(&Prefs) -> io::Result<()> + Send + 'static,
        wait: Duration,
    ) -> Option<Self> {
        Self::spawn(sink, wait)
    }

    fn spawn(
        sink: impl Fn(&Prefs) -> io::Result<()> + Send + 'static,
        wait: Duration,
    ) -> Option<Self> {
        let (values, received) = mpsc::channel();
        let (done, finished) = mpsc::channel();
        let body = move || {
            let _done = Done(done);
            while let Ok(mut latest) = received.recv() {
                while let Ok(next) = received.try_recv() {
                    latest = next;
                }
                if let Err(error) = sink(&latest) {
                    warn!("writing preferences failed: {:?}", error.kind());
                }
            }
        };
        match thread::Builder::new()
            .name("takofy-prefs".to_owned())
            .spawn(body)
        {
            Ok(handle) => Some(Self {
                values: Some(values),
                done: finished,
                handle: Some(handle),
                wait,
            }),
            Err(error) => {
                warn!("preferences writer could not start: {:?}", error.kind());
                None
            }
        }
    }

    fn send(&self, prefs: Prefs) {
        if let Some(values) = &self.values {
            let _ = values.send(prefs);
        }
    }
}

impl Drop for PrefsWriter {
    fn drop(&mut self) {
        drop(self.values.take());
        match self.done.recv_timeout(self.wait) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                if let Some(handle) = self.handle.take()
                    && let Err(error) = handle.join()
                {
                    warn!("preferences writer ended unexpectedly: {error:?}");
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                warn!("preferences writer still running after {:?}", self.wait);
                drop(self.handle.take());
            }
        }
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
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    use takofy_ui::prefs::QUIET;
    use takofy_ui::queue::Queue;
    use takofy_ui::volume::Volume;

    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("takofy-prefs-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn file(&self) -> PathBuf {
            self.0.join("preferences.conf")
        }

        fn make(&self) -> &Self {
            fs::create_dir_all(&self.0).expect("a temp directory");
            self
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        move |key| {
            pairs
                .iter()
                .find(|(name, _)| name.as_str() == key)
                .map(|(_, value)| OsString::from(value))
        }
    }

    fn prefs(volume: u16) -> Prefs {
        Prefs::parse(&format!("volume={volume}\n")).expect("a valid level")
    }

    fn padded(content: &str, total: usize) -> String {
        assert!(content.len() + 2 <= total);
        let mut text = content.to_owned();
        text.push('#');
        text.push_str(&"x".repeat(total - content.len() - 2));
        text.push('\n');
        assert_eq!(text.len(), total);
        text
    }

    type Log = Arc<Mutex<Vec<Prefs>>>;

    fn recorder() -> (Log, impl Fn(&Prefs) -> io::Result<()> + Send + 'static) {
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let sink = {
            let log = Arc::clone(&log);
            move |prefs: &Prefs| {
                log.lock().expect("the sink log").push(prefs.clone());
                Ok(())
            }
        };
        (log, sink)
    }

    #[test]
    fn an_absolute_xdg_names_the_file() {
        let root = if cfg!(windows) { "C:\\xdg" } else { "/xdg" };
        assert_eq!(
            config_file(false, vars(&[("XDG_CONFIG_HOME", root), ("HOME", root)])),
            Some(PathBuf::from(root).join("takofy").join("preferences.conf"))
        );
    }

    #[test]
    fn an_empty_xdg_falls_back_to_home() {
        let home = PathBuf::from("/home/tako");
        assert_eq!(
            config_file(
                false,
                vars(&[("XDG_CONFIG_HOME", ""), ("HOME", "/home/tako")])
            ),
            Some(home.join(".config").join(DIR).join(FILE))
        );
    }

    #[test]
    fn a_relative_xdg_falls_back_to_home() {
        let home = PathBuf::from("/home/tako");
        assert_eq!(
            config_file(
                false,
                vars(&[("XDG_CONFIG_HOME", "xdg"), ("HOME", "/home/tako")])
            ),
            Some(home.join(".config").join(DIR).join(FILE))
        );
    }

    #[test]
    fn an_empty_home_with_no_xdg_is_no_directory() {
        assert_eq!(config_file(false, vars(&[("HOME", "")])), None);
    }

    #[test]
    fn no_variables_is_no_directory() {
        assert_eq!(config_file(false, vars(&[])), None);
    }

    #[test]
    fn appdata_names_the_file() {
        let root = if cfg!(windows) {
            "C:\\appdata"
        } else {
            "/appdata"
        };
        assert_eq!(
            config_file(true, vars(&[("APPDATA", root)])),
            Some(PathBuf::from(root).join("takofy").join("preferences.conf"))
        );
    }

    #[test]
    fn an_empty_appdata_is_no_directory() {
        assert_eq!(
            config_file(true, vars(&[("APPDATA", ""), ("HOME", "/home/tako")])),
            None
        );
    }

    #[test]
    fn an_unset_appdata_is_no_directory() {
        assert_eq!(config_file(true, vars(&[("HOME", "/home/tako")])), None);
    }

    #[test]
    fn a_missing_file_is_absent() {
        let dir = Temp::new("a_missing_file_is_absent");
        assert_eq!(load(&dir.file()), Loaded::Absent);
    }

    #[test]
    fn a_valid_file_is_found() {
        let dir = Temp::new("a_valid_file_is_found");
        dir.make();
        let wanted =
            Prefs::parse("volume=40\nmute=true\nshuffle=true\nrepeat=all\n").expect("valid prefs");
        fs::write(dir.file(), wanted.render()).expect("a writable temp file");
        assert_eq!(load(&dir.file()), Loaded::Found(wanted));
    }

    #[test]
    fn a_refused_file_is_refused_and_left_alone() {
        let dir = Temp::new("a_refused_file_is_refused_and_left_alone");
        dir.make();
        let text = "volume=80\nrepeat=sometimes\n";
        fs::write(dir.file(), text).expect("a writable temp file");
        match load(&dir.file()) {
            Loaded::Refused(reason) => {
                assert!(
                    reason.contains("line 2"),
                    "a refusal names its line: {reason}"
                );
                assert!(
                    !reason.contains("sometimes"),
                    "a refusal never repeats the value: {reason}"
                );
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert_eq!(
            fs::read_to_string(dir.file()).expect("the file is still there"),
            text,
            "a refused file is not rewritten"
        );
    }

    #[test]
    fn a_directory_at_the_path_is_unreadable() {
        let dir = Temp::new("a_directory_at_the_path_is_unreadable");
        fs::create_dir_all(dir.file()).expect("a directory at the path");
        assert_eq!(load(&dir.file()), Loaded::Unreadable);
    }

    #[test]
    fn a_file_that_is_not_utf8_is_unreadable() {
        let dir = Temp::new("a_file_that_is_not_utf8_is_unreadable");
        dir.make();
        fs::write(dir.file(), [0xff, 0xfe, b'v', b'o']).expect("a writable temp file");
        assert_eq!(load(&dir.file()), Loaded::Unreadable);
    }

    #[test]
    fn a_file_at_the_limit_loads() {
        let dir = Temp::new("a_file_at_the_limit_loads");
        dir.make();
        let text = padded("volume=80\n", LIMIT as usize);
        fs::write(dir.file(), &text).expect("a writable temp file");
        assert_eq!(load(&dir.file()), Loaded::Found(prefs(80)));
    }

    #[test]
    fn a_file_over_the_limit_is_refused() {
        let dir = Temp::new("a_file_over_the_limit_is_refused");
        dir.make();
        let text = padded("volume=80\n", LIMIT as usize + 1);
        fs::write(dir.file(), &text).expect("a writable temp file");
        assert_eq!(
            load(&dir.file()),
            Loaded::Refused("file too large".to_owned())
        );
    }

    #[test]
    fn a_write_creates_the_parent_directory_and_replaces_the_file() {
        let dir = Temp::new("a_write_creates_the_parent_directory_and_replaces_the_file");
        let path = dir.path().join("nested").join("preferences.conf");
        write_atomic(&path, &prefs(70)).expect("a creatable directory");
        assert_eq!(
            fs::read_to_string(&path).expect("the written file"),
            prefs(70).render()
        );
        write_atomic(&path, &prefs(20)).expect("a replaceable file");
        assert_eq!(
            fs::read_to_string(&path).expect("the replaced file"),
            prefs(20).render()
        );
    }

    #[test]
    fn a_failed_write_leaves_no_temp_file() {
        let dir = Temp::new("a_failed_write_leaves_no_temp_file");
        fs::create_dir_all(dir.file()).expect("a directory at the target");
        assert!(write_atomic(&dir.file(), &prefs(50)).is_err());
        let temp = dir
            .path()
            .join(format!("preferences.conf.tmp.{}", std::process::id()));
        assert!(!temp.exists(), "a failed write removes its temp file");
    }

    #[test]
    fn five_rapid_sends_end_with_the_last_value() {
        let (entered, started) = mpsc::channel();
        let (release, unblocked) = mpsc::channel::<()>();
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let first = std::cell::Cell::new(true);
        let sink = {
            let log = Arc::clone(&log);
            move |prefs: &Prefs| {
                log.lock().expect("the sink log").push(prefs.clone());
                if first.replace(false) {
                    let _ = entered.send(());
                    let _ = unblocked.recv();
                }
                Ok(())
            }
        };
        let writer = PrefsWriter::with_sink(sink, FLUSH_WAIT).expect("a writer thread");
        writer.send(prefs(1));
        started
            .recv_timeout(Duration::from_secs(2))
            .expect("the sink takes its first call");
        for volume in 2..=5 {
            writer.send(prefs(volume));
        }
        release.send(()).expect("unblock the sink");
        drop(writer);
        let log = log.lock().expect("the sink log");
        assert!(
            log.len() < 5,
            "a burst coalesces into fewer writes: {} writes broke that",
            log.len()
        );
        assert_eq!(
            log.last(),
            Some(&prefs(5)),
            "the last value of the burst is written: {log:?}"
        );
    }

    #[test]
    fn drop_flushes_a_buffered_value() {
        let (log, sink) = recorder();
        let writer = PrefsWriter::with_sink(sink, FLUSH_WAIT).expect("a writer thread");
        writer.send(prefs(33));
        drop(writer);
        assert_eq!(*log.lock().expect("the sink log"), vec![prefs(33)]);
    }

    #[test]
    fn a_stalled_sink_does_not_stall_drop() {
        let (entered, started) = mpsc::channel();
        let (release, unblocked) = mpsc::channel::<()>();
        let sink = move |_: &Prefs| {
            let _ = entered.send(());
            let _ = unblocked.recv();
            Ok(())
        };
        let writer =
            PrefsWriter::with_sink(sink, Duration::from_millis(50)).expect("a writer thread");
        writer.send(prefs(40));
        started
            .recv_timeout(Duration::from_secs(2))
            .expect("the sink starts");
        let began = Instant::now();
        drop(writer);
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "drop returns bounded: {:?}",
            began.elapsed()
        );
        release.send(()).expect("unblock the sink");
    }

    #[test]
    fn a_panicking_sink_does_not_panic_drop() {
        let writer = PrefsWriter::with_sink(|_: &Prefs| panic!("the sink panicked"), FLUSH_WAIT)
            .expect("a writer thread");
        writer.send(prefs(40));
        drop(writer);
    }

    #[test]
    fn observe_sends_once_after_quiet_and_returns_a_wake_while_pending() {
        let base = prefs(40);
        let (log, sink) = recorder();
        let mut sync = PrefsSync {
            settle: Settle::new(base),
            writer: PrefsWriter::with_sink(sink, FLUSH_WAIT),
        };
        let changed = prefs(60);
        let now = Instant::now();
        assert_eq!(sync.observe(now, &changed), Some(QUIET));
        assert!(
            log.lock().expect("the sink log").is_empty(),
            "quiet has not passed yet"
        );
        assert_eq!(sync.observe(now + QUIET, &changed), None);
        drop(sync);
        assert_eq!(*log.lock().expect("the sink log"), vec![changed]);
    }

    #[test]
    fn an_untouched_sync_returns_no_wake() {
        let (log, sink) = recorder();
        let mut sync = PrefsSync {
            settle: Settle::new(prefs(40)),
            writer: PrefsWriter::with_sink(sink, FLUSH_WAIT),
        };
        let now = Instant::now();
        assert_eq!(sync.observe(now, &prefs(40)), None);
        assert_eq!(sync.observe(now + QUIET, &prefs(40)), None);
        drop(sync);
        assert!(
            log.lock().expect("the sink log").is_empty(),
            "an untouched session writes nothing"
        );
    }

    #[test]
    fn close_flushes_an_unsettled_value_once() {
        let (log, sink) = recorder();
        let mut sync = PrefsSync {
            settle: Settle::new(prefs(40)),
            writer: PrefsWriter::with_sink(sink, FLUSH_WAIT),
        };
        let changed = prefs(60);
        sync.close(&changed);
        sync.close(&changed);
        drop(sync);
        assert_eq!(*log.lock().expect("the sink log"), vec![changed]);
    }

    #[test]
    fn a_sync_without_a_writer_does_nothing_and_does_not_panic() {
        let mut sync = PrefsSync::off();
        let now = Instant::now();
        let changed = prefs(25);
        assert_eq!(sync.observe(now, &changed), Some(QUIET));
        assert_eq!(sync.observe(now + QUIET, &changed), None);
        sync.close(&changed);
        sync.close(&changed);
    }

    /// A running app's state, which `Prefs::of` reads back as `prefs`.
    fn state(prefs: &Prefs) -> (Volume, Queue) {
        let mut queue = Queue::seeded(1);
        prefs.seed_queue(&mut queue);
        (prefs.volume(), queue)
    }

    /// Writes `text` into `dir` and opens it with a writer that saves through
    /// `sink`.
    fn opened(
        dir: &Temp,
        text: &str,
        sink: impl Fn(&Prefs) -> io::Result<()> + Send + 'static,
    ) -> (Loaded, PrefsSync) {
        dir.make();
        fs::write(dir.file(), text).expect("a writable temp file");
        open_at(&dir.file(), |_| PrefsWriter::with_sink(sink, FLUSH_WAIT))
    }

    #[test]
    fn a_found_file_that_matches_its_own_values_writes_nothing_at_close() {
        let dir = Temp::new("a_found_file_that_matches_its_own_values_writes_nothing_at_close");
        let (log, sink) = recorder();
        let text = "volume=40\nshuffle=true\nrepeat=all\n";
        let (loaded, mut sync) = opened(&dir, text, sink);
        let written = Prefs::parse(text).expect("valid prefs");
        assert_eq!(loaded, Loaded::Found(written.clone()));

        let (volume, queue) = state(&written);
        sync.close(&Prefs::of(&volume, &queue));
        drop(sync);

        assert!(
            log.lock().expect("the sink log").is_empty(),
            "the file already holds these values, so closing writes nothing"
        );
    }

    #[test]
    fn a_changed_value_after_a_found_file_writes_once() {
        let dir = Temp::new("a_changed_value_after_a_found_file_writes_once");
        let (log, sink) = recorder();
        let text = "volume=40\nshuffle=true\nrepeat=all\n";
        let (loaded, mut sync) = opened(&dir, text, sink);
        assert_eq!(
            loaded,
            Loaded::Found(Prefs::parse(text).expect("valid prefs"))
        );

        let changed = Prefs::parse("volume=60\nshuffle=true\nrepeat=all\n").expect("valid prefs");
        let (volume, queue) = state(&changed);
        sync.close(&Prefs::of(&volume, &queue));
        sync.close(&Prefs::of(&volume, &queue));
        drop(sync);

        assert_eq!(
            *log.lock().expect("the sink log"),
            vec![changed],
            "a value that differs from the file is written once"
        );
    }

    #[test]
    fn a_refused_file_is_not_rewritten_until_a_value_settles() {
        let dir = Temp::new("a_refused_file_is_not_rewritten_until_a_value_settles");
        dir.make();
        let text = "volume=80\nrepeat=sometimes\n";
        fs::write(dir.file(), text).expect("a writable temp file");
        let (loaded, mut sync) = open_at(&dir.file(), PrefsWriter::start);
        assert!(
            matches!(loaded, Loaded::Refused(_)),
            "the bad value refuses the file: {loaded:?}"
        );

        // The app holds the defaults for a refused file, so closing with them
        // must leave the bytes the user wrote alone.
        let (volume, queue) = state(&Prefs::default());
        sync.close(&Prefs::of(&volume, &queue));
        drop(sync);

        assert_eq!(
            fs::read_to_string(dir.file()).expect("the file is still there"),
            text,
            "a refused file is not rewritten"
        );
    }
}
