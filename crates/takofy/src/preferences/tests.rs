use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use takofy_ui::prefs::QUIET;
use takofy_ui::queue::Queue;
use takofy_ui::volume::Volume;

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("takofy-prefs-{}-{name}", std::process::id()));
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
    let writer = PrefsWriter::with_sink(sink, Duration::from_millis(50)).expect("a writer thread");
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
