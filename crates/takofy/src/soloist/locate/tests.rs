use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::{Located, find, name};

/// A file check answering for one fixed set of paths.
fn only(paths: &[&str]) -> impl Fn(&Path) -> bool {
    let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    move |candidate: &Path| paths.iter().any(|path| path == candidate)
}

#[test]
fn the_configured_path_wins_over_the_path_entries() {
    let found = find(
        Some(Path::new("/opt/soloist")),
        Some(OsStr::new("/usr/bin")),
        only(&["/opt/soloist", "/usr/bin/soloist"]),
    );

    assert_eq!(found, Located::Found(PathBuf::from("/opt/soloist")));
}

#[test]
fn a_configured_path_that_is_not_there_is_missing() {
    let found = find(
        Some(Path::new("/opt/soloist")),
        Some(OsStr::new("/usr/bin")),
        only(&["/usr/bin/soloist"]),
    );

    assert_eq!(
        found,
        Located::Missing,
        "a path the user named is never answered by another program"
    );
    assert!(found.path().is_none());
}

#[test]
fn the_first_path_entry_holding_the_program_wins() {
    let path = std::env::join_paths(["/nowhere", "/usr/bin", "/usr/local/bin"]).expect("a path");
    let expected = Path::new("/usr/bin").join(name());

    let found = find(
        None,
        Some(&path),
        only(&["/usr/bin/soloist", "/usr/local/bin/soloist"]),
    );

    assert_eq!(found, Located::Found(expected));
}

#[test]
fn no_path_entries_and_no_program() {
    assert_eq!(
        find(None, Some(OsStr::new("")), only(&[])),
        Located::Missing
    );
    assert_eq!(find(None, None, only(&[])), Located::Missing);
}

#[test]
fn the_program_is_named_for_the_platform() {
    if cfg!(windows) {
        assert_eq!(name(), "soloist.exe", "Windows resolves the suffix itself");
    } else {
        assert_eq!(name(), "soloist");
    }
}
