use super::*;
use std::cell::Cell;
use std::ffi::OsString;
use std::sync::{Arc, Mutex};
use std::thread;
use takofy_model::{
    Account, CoverSet, CoverUrl, LikedTrack, PlaylistAccess, PlaylistCover, PlaylistEntry, Revision,
};

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("takofy-snapshot-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn file(&self) -> PathBuf {
        self.0.join(FILE)
    }

    fn make(self) -> Self {
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

fn account() -> Account {
    Account::from_wire("someone").expect("a name")
}

fn other_account() -> Account {
    Account::from_wire("someone else").expect("a name")
}

fn snapshot() -> Snapshot {
    Snapshot {
        account: account(),
        playlists: Some(vec![PlaylistEntry {
            uri: "spotify:playlist:1".to_owned(),
            name: "One".to_owned(),
            named: true,
            cover: None,
            composed: Vec::new(),
            description: None,
            tracks: None,
            owner: None,
            access: None,
            saved_at: None,
        }]),
        liked: Some(vec![LikedTrack {
            uri: "spotify:track:1".to_owned(),
            saved_at: None,
        }]),
        ..Snapshot::new(account())
    }
}

type Seen = Arc<Mutex<Vec<Snapshot>>>;
type Sink = Box<dyn Fn(&Snapshot) -> io::Result<()> + Send>;

/// A sink that keeps every copy it is handed, and the list it keeps them in.
fn recorder() -> (Seen, Sink) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = {
        let seen = Arc::clone(&seen);
        move |snapshot: &Snapshot| {
            seen.lock().expect("the recorder").push(snapshot.clone());
            Ok(())
        }
    };
    (seen, Box::new(sink))
}

/// Waits until the writer thread has handed over `len` copies.
fn wait_for(seen: &Arc<Mutex<Vec<Snapshot>>>, len: usize) {
    for _ in 0..400 {
        if seen.lock().expect("the recorder").len() >= len {
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("the writer did not write");
}

fn writes(seen: &Arc<Mutex<Vec<Snapshot>>>) -> Vec<Snapshot> {
    seen.lock().expect("the recorder").clone()
}

/// A path the host calls absolute: the same test must run on a Windows runner.
fn absolute_root() -> &'static str {
    if cfg!(windows) { "C:\\cache" } else { "/cache" }
}

fn under(root: &str) -> PathBuf {
    PathBuf::from(root).join(DIR).join(FILE)
}

#[test]
fn the_xdg_cache_home_is_used_when_absolute() {
    let root = absolute_root();
    let path = cache_file(
        false,
        vars(&[("XDG_CACHE_HOME", root), ("HOME", "/home/x")]),
    );
    assert_eq!(path, Some(under(root)));
}

#[test]
fn a_relative_or_empty_xdg_cache_home_is_ignored() {
    for value in ["cache", ""] {
        let path = cache_file(
            false,
            vars(&[("XDG_CACHE_HOME", value), ("HOME", "/home/x")]),
        );
        assert_eq!(
            path,
            Some(PathBuf::from("/home/x").join(".cache").join(DIR).join(FILE)),
            "XDG_CACHE_HOME={value:?}"
        );
    }
}

#[test]
fn home_names_the_cache_directory_when_nothing_else_does() {
    assert_eq!(
        cache_file(false, vars(&[("HOME", "/home/x")])),
        Some(PathBuf::from("/home/x").join(".cache").join(DIR).join(FILE))
    );
}

#[test]
fn no_directory_is_none() {
    assert_eq!(cache_file(false, vars(&[("HOME", "")])), None);
    assert_eq!(cache_file(false, vars(&[])), None);
}

#[test]
fn windows_uses_local_appdata() {
    assert_eq!(
        cache_file(true, vars(&[("LOCALAPPDATA", "C:\\Cache")])),
        Some(under("C:\\Cache"))
    );
    assert_eq!(cache_file(true, vars(&[("LOCALAPPDATA", "")])), None);
    assert_eq!(cache_file(true, vars(&[])), None);
}

#[test]
fn a_missing_file_is_absent() {
    let dir = Temp::new("a_missing_file_is_absent");
    assert_eq!(load(&dir.file()), Cached::Absent);
}

#[test]
fn a_written_copy_reads_back() {
    let dir = Temp::new("a_written_copy_reads_back").make();
    write(&dir.file(), &snapshot()).expect("a writable copy");
    assert_eq!(load(&dir.file()), Cached::Found(snapshot()));
}

#[test]
fn a_file_that_is_not_text_is_unreadable() {
    let dir = Temp::new("a_file_that_is_not_text_is_unreadable").make();
    fs::write(dir.file(), [0xff, 0xfe]).expect("a writable file");
    assert_eq!(load(&dir.file()), Cached::Unreadable);
}

#[test]
fn a_directory_at_the_path_is_unreadable() {
    let dir = Temp::new("a_directory_at_the_path_is_unreadable").make();
    fs::create_dir_all(dir.file()).expect("a directory");
    assert_eq!(load(&dir.file()), Cached::Unreadable);
}

#[test]
fn a_refused_file_says_why() {
    let dir = Temp::new("a_refused_file_says_why").make();
    fs::write(dir.file(), "nonsense\n").expect("a writable file");
    match load(&dir.file()) {
        Cached::Refused(reason) => assert!(reason.contains("line 1"), "{reason}"),
        other => panic!("read {other:?}"),
    }
}

/// A copy whose rendered file is exactly `bytes` long, padded with its URI.
fn sized(bytes: usize) -> Snapshot {
    let base = Snapshot {
        liked: Some(vec![LikedTrack {
            uri: String::new(),
            saved_at: None,
        }]),
        ..Snapshot::new(account())
    };
    let overhead = snapshot_format::render(&base).len();
    Snapshot {
        liked: Some(vec![LikedTrack {
            uri: "u".repeat(bytes - overhead),
            saved_at: None,
        }]),
        ..Snapshot::new(account())
    }
}

#[test]
fn a_file_at_the_limit_is_read() {
    let dir = Temp::new("a_file_at_the_limit_is_read").make();
    let full = sized(LIMIT as usize);
    fs::write(dir.file(), snapshot_format::render(&full)).expect("a writable file");
    assert_eq!(load(&dir.file()), Cached::Found(full));
}

#[test]
fn a_file_over_the_limit_is_refused() {
    let dir = Temp::new("a_file_over_the_limit_is_refused").make();
    let over = sized(LIMIT as usize + 1);
    fs::write(dir.file(), snapshot_format::render(&over)).expect("a writable file");
    assert!(matches!(load(&dir.file()), Cached::Refused(_)));
}

#[test]
fn a_write_leaves_no_temporary_behind() {
    let dir = Temp::new("a_write_leaves_no_temporary_behind").make();
    write(&dir.file(), &snapshot()).expect("a writable copy");
    let leftovers: Vec<String> = fs::read_dir(dir.path())
        .expect("a readable directory")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".tmp."))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn an_unwritable_path_is_an_error() {
    let dir = Temp::new("an_unwritable_path_is_an_error").make();
    let blocker = dir.path().join("blocked");
    fs::write(&blocker, b"not a directory").expect("a writable file");
    let path = blocker.join(FILE);
    assert!(write(&path, &snapshot()).is_err());
}

#[test]
fn an_oversize_copy_removes_the_old_file() {
    let dir = Temp::new("an_oversize_copy_removes_the_old_file").make();
    write(&dir.file(), &snapshot()).expect("a writable copy");
    write(&dir.file(), &sized(LIMIT as usize + 1)).expect("a skipped write");
    assert!(!dir.file().exists());
}

#[cfg(unix)]
#[test]
fn a_written_file_is_private() {
    use std::os::unix::fs::PermissionsExt;

    let dir = Temp::new("a_written_file_is_private").make();
    write(&dir.file(), &snapshot()).expect("a writable copy");
    let mode = fs::metadata(dir.file())
        .expect("the copy")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn a_change_waits_for_two_seconds_of_quiet() {
    let (seen, sink) = recorder();
    let mut sync = SnapshotSync::for_test(None, SnapshotWriter::with_sink(sink, FLUSH_WAIT));
    let start = Instant::now();
    let build = || Some(snapshot());
    assert_eq!(sync.observe(start, 1, build), Some(QUIET));
    let later = start + Duration::from_millis(1500);
    assert_eq!(
        sync.observe(later, 1, build),
        Some(Duration::from_millis(500))
    );
    assert!(writes(&seen).is_empty());
    assert_eq!(sync.observe(start + QUIET, 1, build), None);
    wait_for(&seen, 1);
    assert_eq!(writes(&seen).len(), 1);
}

#[test]
fn a_second_change_restarts_the_quiet() {
    let (seen, sink) = recorder();
    let mut sync = SnapshotSync::for_test(None, SnapshotWriter::with_sink(sink, FLUSH_WAIT));
    let start = Instant::now();
    let build = || Some(snapshot());
    assert_eq!(sync.observe(start, 1, build), Some(QUIET));
    assert_eq!(sync.observe(start + QUIET / 2, 2, build), Some(QUIET));
    assert_eq!(
        sync.observe(start + QUIET, 2, build),
        Some(Duration::from_millis(1000))
    );
    assert!(writes(&seen).is_empty());
    assert_eq!(sync.observe(start + QUIET + QUIET / 2, 2, build), None);
    wait_for(&seen, 1);
    assert_eq!(writes(&seen).len(), 1);
}

#[test]
fn a_burst_writes_once() {
    let (seen, sink) = recorder();
    let mut sync = SnapshotSync::for_test(None, SnapshotWriter::with_sink(sink, FLUSH_WAIT));
    let start = Instant::now();
    let build = || Some(snapshot());
    assert_eq!(sync.observe(start, 1, build), Some(QUIET));
    for millis in [200, 900, 1500, 1900] {
        let now = start + Duration::from_millis(millis);
        assert!(sync.observe(now, 1, build).is_some());
    }
    assert!(writes(&seen).is_empty());
    assert_eq!(sync.observe(start + QUIET, 1, build), None);
    wait_for(&seen, 1);
    assert_eq!(writes(&seen), vec![snapshot()]);
}

#[test]
fn an_unchanged_copy_is_not_written() {
    let (seen, sink) = recorder();
    let mut sync = SnapshotSync::for_test(
        Some(snapshot()),
        SnapshotWriter::with_sink(sink, FLUSH_WAIT),
    );
    let start = Instant::now();
    assert_eq!(sync.observe(start, 1, || Some(snapshot())), Some(QUIET));
    assert_eq!(sync.observe(start + QUIET, 1, || Some(snapshot())), None);
    sync.close(|| Some(snapshot()));
    drop(sync);
    assert!(writes(&seen).is_empty());
}

/// Waits until the writer thread has recorded that its last write failed.
fn wait_for_a_failure(sync: &SnapshotSync) {
    for _ in 0..400 {
        let failed = sync
            .writer
            .as_ref()
            .is_some_and(|writer| writer.failed.load(Ordering::Relaxed));
        if failed {
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("the writer did not fail");
}

#[test]
fn a_failed_write_is_written_again_at_close() {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let attempted = AtomicBool::new(false);
    let sink = {
        let seen = Arc::clone(&seen);
        move |snapshot: &Snapshot| {
            seen.lock().expect("the recorder").push(snapshot.clone());
            if attempted.swap(true, Ordering::Relaxed) {
                Ok(())
            } else {
                Err(io::Error::other("disk full"))
            }
        }
    };
    let mut sync = SnapshotSync::for_test(None, SnapshotWriter::with_sink(sink, FLUSH_WAIT));
    let start = Instant::now();
    assert_eq!(sync.observe(start, 1, || Some(snapshot())), Some(QUIET));
    assert_eq!(sync.observe(start + QUIET, 1, || Some(snapshot())), None);
    wait_for_a_failure(&sync);
    sync.close(|| Some(snapshot()));
    drop(sync);
    assert_eq!(writes(&seen), vec![snapshot(), snapshot()]);
}

#[test]
fn a_deferred_build_waits_for_the_next_change() {
    let (seen, sink) = recorder();
    let mut sync = SnapshotSync::for_test(None, SnapshotWriter::with_sink(sink, FLUSH_WAIT));
    let calls = Cell::new(0);
    let build = || {
        calls.set(calls.get() + 1);
        (calls.get() > 1).then(snapshot)
    };
    let start = Instant::now();
    assert_eq!(sync.observe(start, 1, build), Some(QUIET));
    assert_eq!(sync.observe(start + QUIET, 1, build), None);
    assert_eq!(calls.get(), 1);
    assert_eq!(sync.observe(start + 2 * QUIET, 1, build), None);
    assert_eq!(calls.get(), 1);
    assert!(writes(&seen).is_empty());
    assert_eq!(sync.observe(start + 2 * QUIET, 2, build), Some(QUIET));
    assert_eq!(sync.observe(start + 3 * QUIET, 2, build), None);
    assert_eq!(calls.get(), 2);
    wait_for(&seen, 1);
    assert_eq!(writes(&seen), vec![snapshot()]);
}

#[test]
fn close_writes_what_differs_now() {
    let (seen, sink) = recorder();
    let mut sync = SnapshotSync::for_test(None, SnapshotWriter::with_sink(sink, FLUSH_WAIT));
    assert_eq!(
        sync.observe(Instant::now(), 1, || Some(snapshot())),
        Some(QUIET)
    );
    sync.close(|| Some(snapshot()));
    drop(sync);
    assert_eq!(writes(&seen), vec![snapshot()]);
}

#[test]
fn close_writes_nothing_when_nothing_differs() {
    let (seen, sink) = recorder();
    let mut sync = SnapshotSync::for_test(
        Some(snapshot()),
        SnapshotWriter::with_sink(sink, FLUSH_WAIT),
    );
    sync.close(|| Some(snapshot()));
    sync.close(|| None);
    drop(sync);
    assert!(writes(&seen).is_empty());
}

#[test]
fn off_saves_nothing() {
    let mut sync = SnapshotSync::off();
    assert_eq!(sync.observe(Instant::now(), 7, || Some(snapshot())), None);
    sync.close(|| Some(snapshot()));
}

/// A copy holding every field the file does not keep as it was handed over: a
/// cover wired largest-first with an unsized variant ahead of the sized ones,
/// and the access a live read attaches to a playlist row.
fn wire_order_copy() -> Snapshot {
    let url = |n: u8| {
        let mut id = [0u8; 20];
        id[0] = n;
        CoverUrl::from_file_id(&id).expect("a file id")
    };
    let cover = |n: u8| {
        CoverSet::from_sizes([(None, url(n)), (Some((640, 640)), url(n))]).expect("a cover set")
    };
    Snapshot {
        playlists: Some(vec![PlaylistEntry {
            uri: "spotify:playlist:1".to_owned(),
            name: "One".to_owned(),
            named: true,
            cover: Some(PlaylistCover::chosen(cover(1))),
            composed: vec![cover(2), cover(3)],
            description: None,
            tracks: Some(3),
            owner: None,
            access: Some(PlaylistAccess {
                revision: Revision::from_wire(b"rev-1").expect("a revision"),
                editable: true,
            }),
            saved_at: None,
        }]),
        liked: snapshot().liked,
        ..Snapshot::new(account())
    }
}

#[test]
fn a_copy_the_file_already_renders_is_not_written() {
    let dir = Temp::new("a_copy_the_file_already_renders_is_not_written").make();
    write(&dir.file(), &wire_order_copy()).expect("a writable copy");
    let before = fs::metadata(dir.file())
        .expect("the copy")
        .modified()
        .expect("an mtime");

    let (seen, sink) = recorder();
    let (cached, mut sync) = open_at(Some(dir.file()), move |_| {
        SnapshotWriter::with_sink(sink, FLUSH_WAIT)
    });
    assert!(matches!(cached, Cached::Found(_)));
    sync.close(|| Some(wire_order_copy()));
    drop(sync);
    assert!(
        writes(&seen).is_empty(),
        "a copy the file already renders was written"
    );
    assert_eq!(
        fs::metadata(dir.file())
            .expect("the copy")
            .modified()
            .expect("an mtime"),
        before,
        "the file was rewritten"
    );

    let (seen, sink) = recorder();
    let (_, mut sync) = open_at(Some(dir.file()), move |_| {
        SnapshotWriter::with_sink(sink, FLUSH_WAIT)
    });
    let mut changed = wire_order_copy();
    changed.playlists.as_mut().expect("playlists")[0].name = "Two".to_owned();
    sync.close(|| Some(changed.clone()));
    wait_for(&seen, 1);
    drop(sync);
    assert_eq!(writes(&seen), vec![changed]);
}

#[test]
fn a_refused_copy_is_replaced_whole() {
    let dir = Temp::new("a_refused_copy_is_replaced_whole").make();
    fs::write(dir.file(), "nonsense\n").expect("a writable file");
    let (cached, mut sync) = open_at(Some(dir.file()), SnapshotWriter::start);
    assert!(matches!(cached, Cached::Refused(_)));
    sync.close(|| Some(snapshot()));
    drop(sync);
    assert_eq!(load(&dir.file()), Cached::Found(snapshot()));
}

#[test]
fn no_cache_directory_saves_nothing() {
    let (cached, mut sync) = open_at(None, |_| None);
    assert_eq!(cached, Cached::Absent);
    assert_eq!(sync.observe(Instant::now(), 3, || Some(snapshot())), None);
    sync.close(|| Some(snapshot()));
}

#[test]
fn a_writer_that_cannot_start_saves_nothing() {
    let dir = Temp::new("a_writer_that_cannot_start_saves_nothing").make();
    write(&dir.file(), &snapshot()).expect("a writable copy");
    let (cached, mut sync) = open_at(Some(dir.file()), |_| None);
    assert!(matches!(cached, Cached::Found(_)));
    let different = Snapshot::new(other_account());
    sync.close(|| Some(different));
    drop(sync);
    assert_eq!(load(&dir.file()), Cached::Found(snapshot()));
}
