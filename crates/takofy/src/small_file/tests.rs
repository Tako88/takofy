use super::{Read, read_text, write};
use std::fs;
use std::path::{Path, PathBuf};

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("takofy-file-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn file(&self) -> PathBuf {
        self.0.join("small.file")
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

#[test]
fn a_missing_file_is_absent() {
    let dir = Temp::new("a_missing_file_is_absent");
    assert_eq!(read_text(&dir.file(), 16), Read::Absent);
}

#[test]
fn a_file_at_the_limit_is_read_whole() {
    let dir = Temp::new("a_file_at_the_limit_is_read_whole");
    dir.make();
    fs::write(dir.file(), b"0123456789abcdef").expect("a writable temp file");
    assert_eq!(
        read_text(&dir.file(), 16),
        Read::Text("0123456789abcdef".to_owned())
    );
}

#[test]
fn a_file_over_the_limit_is_refused() {
    let dir = Temp::new("a_file_over_the_limit_is_refused");
    dir.make();
    fs::write(dir.file(), b"0123456789abcdefg").expect("a writable temp file");
    assert_eq!(read_text(&dir.file(), 16), Read::TooLarge);
}

#[test]
fn a_directory_at_the_path_is_unreadable() {
    let dir = Temp::new("a_directory_at_the_path_is_unreadable");
    fs::create_dir_all(dir.file()).expect("a directory at the path");
    assert_eq!(read_text(&dir.file(), 16), Read::Unreadable);
}

#[test]
fn bytes_that_are_not_text_are_unreadable() {
    let dir = Temp::new("bytes_that_are_not_text_are_unreadable");
    dir.make();
    fs::write(dir.file(), [0xff, 0xfe, b'a']).expect("a writable temp file");
    assert_eq!(read_text(&dir.file(), 16), Read::Unreadable);
}

#[test]
fn a_write_creates_the_parent_directory_and_replaces_the_file() {
    let dir = Temp::new("a_write_creates_the_parent_directory_and_replaces_the_file");
    let path = dir.path().join("nested").join("small.file");
    write(&path, "first\n").expect("a creatable directory");
    assert_eq!(
        fs::read_to_string(&path).expect("the written file"),
        "first\n"
    );
    write(&path, "second\n").expect("a replaceable file");
    assert_eq!(
        fs::read_to_string(&path).expect("the replaced file"),
        "second\n"
    );
}

#[test]
fn a_failed_write_leaves_no_temp_file() {
    let dir = Temp::new("a_failed_write_leaves_no_temp_file");
    fs::create_dir_all(dir.file()).expect("a directory at the target");
    assert!(write(&dir.file(), "text").is_err());
    let temp = dir
        .path()
        .join(format!("small.file.tmp.{}", std::process::id()));
    assert!(!temp.exists(), "a failed write removes its temp file");
}

#[cfg(unix)]
#[test]
fn a_written_file_is_private_to_its_user() {
    use std::os::unix::fs::PermissionsExt;

    let dir = Temp::new("a_written_file_is_private_to_its_user");
    let path = dir.path().join("small.file");
    write(&path, "secret\n").expect("a writable directory");
    let mode = fs::metadata(&path)
        .expect("the written file")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "the file is readable by its user alone");
}

#[cfg(unix)]
#[test]
fn a_write_narrows_an_existing_wide_file_to_its_user() {
    use std::os::unix::fs::PermissionsExt;

    let dir = Temp::new("a_write_narrows_an_existing_wide_file_to_its_user");
    dir.make();
    let path = dir.file();
    fs::write(&path, "old\n").expect("a writable temp file");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("a wider mode");
    write(&path, "secret\n").expect("a writable directory");
    let mode = fs::metadata(&path)
        .expect("the written file")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "an existing file is narrowed to its user");
}
