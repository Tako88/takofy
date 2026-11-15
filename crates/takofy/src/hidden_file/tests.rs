use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use takofy_model::home::Day;

use super::*;

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("takofy-hidden-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        Self(path)
    }

    fn file(&self) -> PathBuf {
        self.0.join("hidden.conf")
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

fn artist(id: usize) -> String {
    format!("spotify:artist:{id:022}")
}

fn hidden(artists: &[String]) -> Hidden {
    let mut set = Hidden::default();
    for artist in artists {
        assert!(set.hide(artist), "{artist} is an artist address");
    }
    set
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

/// Writes `text` into `dir`, failing the test when that is impossible.
fn put(dir: &Temp, text: &str) -> PathBuf {
    dir.make();
    let path = dir.file();
    fs::write(&path, text).expect("a writable temp file");
    path
}

fn written(dir: &Temp) -> String {
    fs::read_to_string(dir.file()).expect("the file is still there")
}

type Log = Arc<Mutex<Vec<Hidden>>>;

fn recorder() -> (Log, impl Fn(&Hidden) -> io::Result<()> + Send + 'static) {
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let sink = {
        let log = Arc::clone(&log);
        move |hidden: &Hidden| {
            log.lock().expect("the sink log").push(hidden.clone());
            Ok(())
        }
    };
    (log, sink)
}

fn logged(log: &Log) -> Vec<Hidden> {
    log.lock().expect("the sink log").clone()
}

/// Opens `path` with a writer that saves through `sink`.
fn opened(
    path: &Path,
    sink: impl Fn(&Hidden) -> io::Result<()> + Send + 'static,
) -> (Loaded, HiddenSync) {
    open_at(path, |_| HiddenWriter::with_sink(sink, FLUSH_WAIT))
}

#[test]
fn an_absent_file_is_absent_and_not_an_error() {
    let dir = Temp::new("an_absent_file_is_absent_and_not_an_error");
    assert_eq!(load(&dir.file()), Loaded::Absent);
    assert_eq!(
        warning(&Loaded::Absent),
        None,
        "a file that is not there yet is not worth a line"
    );
}

#[test]
fn a_written_file_loads_back() {
    let dir = Temp::new("a_written_file_loads_back");
    dir.make();
    let set = hidden(&[artist(1), artist(2)]);
    write_atomic(&dir.file(), &set).expect("a writable temp file");
    assert_eq!(load(&dir.file()), Loaded::Found(set));
}

#[test]
fn a_valid_file_is_not_warned_about() {
    let dir = Temp::new("a_valid_file_is_not_warned_about");
    let path = put(&dir, "format=1\nhide=spotify:artist:a\n");
    let loaded = load(&path);
    assert!(matches!(loaded, Loaded::Found(_)), "{loaded:?}");
    assert_eq!(warning(&loaded), None);
}

#[test]
fn a_corrupt_file_is_refused_without_quoting_it() {
    let dir = Temp::new("a_corrupt_file_is_refused_without_quoting_it");
    let text = "format=1\nhide=not-an-address\n";
    let path = put(&dir, text);

    match load(&path) {
        Loaded::Refused(reason) => {
            assert!(
                reason.contains("line 2"),
                "a refusal names its line: {reason}"
            );
            assert!(
                !reason.contains("not-an-address"),
                "a refusal never repeats the value: {reason}"
            );
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(written(&dir), text, "a refused file is not rewritten");
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
    fs::write(dir.file(), [0xff, 0xfe, b'h', b'i']).expect("a writable temp file");
    assert_eq!(load(&dir.file()), Loaded::Unreadable);
}

#[test]
fn a_file_at_the_limit_loads() {
    let dir = Temp::new("a_file_at_the_limit_loads");
    let text = padded("hide=spotify:artist:a\n", LIMIT as usize);
    let path = put(&dir, &text);
    assert_eq!(
        load(&path),
        Loaded::Found(hidden(&["spotify:artist:a".to_owned()]))
    );
}

#[test]
fn a_file_over_the_limit_is_refused() {
    let dir = Temp::new("a_file_over_the_limit_is_refused");
    let text = padded("hide=spotify:artist:a\n", LIMIT as usize + 1);
    let path = put(&dir, &text);
    assert_eq!(load(&path), Loaded::Refused("file too large".to_owned()));
}

#[test]
fn no_warning_carries_a_hidden_address() {
    let dir = Temp::new("no_warning_carries_a_hidden_address");
    let kept = artist(7);
    let path = put(&dir, &format!("format=1\nhide={kept}\nformat=2\n"));

    let loaded = load(&path);
    assert!(matches!(loaded, Loaded::Refused(_)), "{loaded:?}");
    let line = warning(&loaded).expect("a refusal is warned about");
    assert!(
        !line.contains(&kept),
        "a warning never prints a hidden address: {line}"
    );
    assert!(
        !line.contains("spotify:"),
        "a warning never prints any address: {line}"
    );

    let unreadable = warning(&Loaded::Unreadable).expect("an unreadable file is warned about");
    assert!(
        !unreadable.contains(&kept),
        "an unreadable file is no reason to print a value: {unreadable}"
    );
}

#[test]
fn a_write_creates_the_parent_directory_and_replaces_the_file() {
    let dir = Temp::new("a_write_creates_the_parent_directory_and_replaces_the_file");
    let path = dir.0.join("nested").join("hidden.conf");
    let first = hidden(&[artist(1)]);
    let second = hidden(&[artist(2), artist(3)]);
    write_atomic(&path, &first).expect("a creatable directory");
    assert_eq!(
        fs::read_to_string(&path).expect("the written file"),
        first.render()
    );
    write_atomic(&path, &second).expect("a replaceable file");
    assert_eq!(
        fs::read_to_string(&path).expect("the replaced file"),
        second.render()
    );
}

#[test]
fn an_untouched_session_writes_nothing() {
    let dir = Temp::new("an_untouched_session_writes_nothing");
    let text = "format=1\nday=19000\nplaylist=1\nartist=2\nalbum=3\nhide=spotify:artist:a\n";
    let path = put(&dir, text);
    let (log, sink) = recorder();
    let (loaded, mut sync) = opened(&path, sink);
    let Loaded::Found(set) = loaded else {
        panic!("the file parses: {loaded:?}");
    };

    sync.observe(&set);
    sync.observe(&set);
    sync.close(&set);
    drop(sync);

    assert!(
        logged(&log).is_empty(),
        "a session that hid nobody and moved no row writes nothing"
    );
    assert_eq!(written(&dir), text, "the file is left exactly as it was");
}

#[test]
fn the_first_hide_writes_once() {
    let dir = Temp::new("the_first_hide_writes_once");
    let path = put(&dir, "format=1\n");
    let (log, sink) = recorder();
    let (loaded, mut sync) = opened(&path, sink);
    assert_eq!(loaded, Loaded::Found(Hidden::default()));

    let set = hidden(&[artist(5)]);
    sync.observe(&set);
    sync.observe(&set);
    sync.close(&set);
    drop(sync);

    assert_eq!(
        logged(&log),
        vec![set],
        "the first hide is written once, not once per frame"
    );
}

#[test]
fn a_burst_of_hides_coalesces() {
    let (entered, started) = mpsc::channel();
    let (release, unblocked) = mpsc::channel::<()>();
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let first = std::cell::Cell::new(true);
    let sink = {
        let log = Arc::clone(&log);
        move |hidden: &Hidden| {
            log.lock().expect("the sink log").push(hidden.clone());
            if first.replace(false) {
                let _ = entered.send(());
                let _ = unblocked.recv();
            }
            Ok(())
        }
    };
    let mut sync = HiddenSync::for_test(0, HiddenWriter::with_sink(sink, FLUSH_WAIT));

    let mut set = Hidden::default();
    assert!(set.hide(&artist(1)));
    sync.observe(&set);
    started
        .recv_timeout(Duration::from_secs(2))
        .expect("the sink takes its first call");

    let last: Hidden = {
        let mut final_set = set.clone();
        for id in 2..=5 {
            assert!(final_set.hide(&artist(id)));
            sync.observe(&final_set);
        }
        final_set
    };

    release.send(()).expect("unblock the sink");
    drop(sync);

    let lines = logged(&log);
    assert!(
        lines.len() < 5,
        "a burst coalesces into fewer writes: {} writes broke that",
        lines.len()
    );
    assert_eq!(
        lines.last(),
        Some(&last),
        "the last set of the burst is the one written: {lines:?}"
    );
}

#[test]
fn close_flushes_a_change_no_frame_sent() {
    let (log, sink) = recorder();
    let mut sync = HiddenSync::for_test(0, HiddenWriter::with_sink(sink, FLUSH_WAIT));
    let set = hidden(&[artist(6)]);

    sync.close(&set);
    sync.close(&set);
    drop(sync);

    assert_eq!(logged(&log), vec![set], "closing writes the change once");
}

#[test]
fn a_refused_file_starts_empty_and_the_first_change_replaces_it_whole() {
    let dir = Temp::new("a_refused_file_starts_empty_and_the_first_change_replaces_it_whole");
    let text = "format=1\nhide=not-an-address\n";
    let path = put(&dir, text);
    let (loaded, mut sync) = open_at(&path, HiddenWriter::start);
    assert!(
        matches!(loaded, Loaded::Refused(_)),
        "the bad line refuses the file: {loaded:?}"
    );

    // The app holds an empty set for a refused file, so closing with it must
    // leave the bytes the user wrote alone.
    sync.close(&Hidden::default());
    assert_eq!(written(&dir), text, "a refused file is left alone");

    let set = hidden(&[artist(9)]);
    sync.close(&set);
    drop(sync);
    assert_eq!(
        written(&dir),
        set.render(),
        "the first change replaces the unreadable file whole"
    );
}

#[test]
fn a_position_write_leaves_a_refused_file_alone_until_an_artist_is_hidden() {
    let dir = Temp::new("a_position_write_leaves_a_refused_file_alone_until_an_artist_is_hidden");
    let text = "format=1\nhide=not-an-address\n";
    let path = put(&dir, text);
    let writes = Arc::new(Mutex::new(Vec::new()));
    let sink = {
        let writes = Arc::clone(&writes);
        let sink_path = path.clone();
        move |hidden: &Hidden| {
            writes.lock().expect("the write log").push(hidden.clone());
            write_atomic(&sink_path, hidden)
        }
    };
    let (loaded, mut sync) = opened(&path, sink);
    assert!(matches!(loaded, Loaded::Refused(_)), "{loaded:?}");

    let mut set = Hidden::default();
    set.set_positions(Day::from_days(19_000), [3, 2, 1]);
    sync.observe(&set);
    sync.close(&set);
    assert_eq!(
        written(&dir),
        text,
        "a frame that only moved the rows must not replace a file it could not read"
    );
    assert!(
        writes.lock().expect("the write log").is_empty(),
        "and nothing reached the writer"
    );

    assert!(set.hide(&artist(9)));
    sync.observe(&set);
    sync.observe(&set);
    drop(sync);

    let replaced = written(&dir);
    assert_eq!(
        writes.lock().expect("the write log").len(),
        1,
        "the hide replaces the file once"
    );
    assert_eq!(replaced, set.render(), "with the hidden set present");
    assert_eq!(replaced.matches("hide=").count(), 1);
}

#[test]
fn a_position_write_leaves_an_unreadable_file_alone() {
    let dir = Temp::new("a_position_write_leaves_an_unreadable_file_alone");
    fs::create_dir_all(dir.file()).expect("a directory at the path");
    let (log, sink) = recorder();
    let (loaded, mut sync) = opened(&dir.file(), sink);
    assert_eq!(loaded, Loaded::Unreadable);

    let mut set = Hidden::default();
    set.set_positions(Day::from_days(19_000), [1, 1, 1]);
    sync.observe(&set);
    sync.close(&set);
    assert!(
        logged(&log).is_empty(),
        "an unreadable file is not replaced by where a row stands"
    );

    assert!(set.hide(&artist(2)));
    sync.close(&set);
    drop(sync);
    assert_eq!(logged(&log), vec![set], "a hide is still written, once");
}

#[test]
fn a_position_write_on_a_readable_file_records_where_the_rows_start() {
    let dir = Temp::new("a_position_write_on_a_readable_file_records_where_the_rows_start");
    let path = put(&dir, "format=1\n");
    let (loaded, mut sync) = open_at(&path, HiddenWriter::start);
    assert_eq!(loaded, Loaded::Found(Hidden::default()));

    let mut set = Hidden::default();
    set.set_positions(Day::from_days(19_000), [2, 1, 0]);
    sync.observe(&set);
    drop(sync);

    assert_eq!(
        written(&dir),
        set.render(),
        "a day's positions survive a restart on their own"
    );
}

#[test]
fn a_sync_without_a_writer_does_nothing_and_does_not_panic() {
    let mut sync = HiddenSync::off();
    let set = hidden(&[artist(4)]);
    sync.observe(&set);
    sync.close(&set);
    sync.close(&set);
}

#[test]
fn observe_after_a_baseline_revision_sends_nothing_for_that_revision() {
    let (log, sink) = recorder();
    let mut set = Hidden::default();
    assert!(set.hide(&artist(1)));
    let revision = set.revision();
    let mut sync = HiddenSync::for_test(revision, HiddenWriter::with_sink(sink, FLUSH_WAIT));

    sync.observe(&set);
    drop(sync);
    assert!(
        logged(&log).is_empty(),
        "the revision the sync was handed is already on disk"
    );
}

#[test]
fn a_stalled_sink_does_not_stall_drop() {
    let (entered, started) = mpsc::channel();
    let (release, unblocked) = mpsc::channel::<()>();
    let sink = move |_: &Hidden| {
        let _ = entered.send(());
        let _ = unblocked.recv();
        Ok(())
    };
    let mut sync =
        HiddenSync::for_test(0, HiddenWriter::with_sink(sink, Duration::from_millis(50)));
    let set = hidden(&[artist(2)]);
    sync.observe(&set);
    started
        .recv_timeout(Duration::from_secs(2))
        .expect("the sink starts");

    let began = Instant::now();
    drop(sync);
    assert!(
        began.elapsed() < Duration::from_secs(2),
        "drop returns bounded: {:?}",
        began.elapsed()
    );
    release.send(()).expect("unblock the sink");
}
