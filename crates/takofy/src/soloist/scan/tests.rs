use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::{Disk, Scan, Tree, downloads_dir, scan};
use crate::soloist::locate::name;

/// A filesystem a test names by hand.
#[derive(Debug, Default)]
struct FakeTree {
    files: BTreeSet<PathBuf>,
    folders: BTreeSet<PathBuf>,
    bytes: BTreeMap<PathBuf, Vec<u8>>,
}

/// What an executable starts with on this platform.
const ELF: [u8; 4] = [0x7f, b'E', b'L', b'F'];
/// What the archive does: a tar names its first member before anything else.
const TAR: &[u8] = b"CHANGELOG.md";

impl FakeTree {
    fn file(mut self, path: &str) -> Self {
        self.files.insert(PathBuf::from(path));
        self
    }

    fn folder(mut self, path: &str) -> Self {
        self.folders.insert(PathBuf::from(path));
        self
    }

    /// A file whose first bytes are `bytes`, so it is not a program.
    fn blob(mut self, path: &str, bytes: &[u8]) -> Self {
        self.files.insert(PathBuf::from(path));
        self.bytes.insert(PathBuf::from(path), bytes.to_vec());
        self
    }
}

impl Tree for FakeTree {
    fn head(&self, path: &Path) -> Option<Vec<u8>> {
        if let Some(bytes) = self.bytes.get(path) {
            return Some(bytes.clone());
        }
        // Every file a test names is a program unless it says otherwise: the
        // ones that are not are the point of the tests that say so.
        self.files.contains(path).then_some(ELF.to_vec())
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.contains(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.folders.contains(path)
    }

    fn children(&self, folder: &Path) -> Vec<PathBuf> {
        let mut children: Vec<PathBuf> = self
            .folders
            .iter()
            .chain(self.files.iter())
            .filter(|path| path.parent() == Some(folder))
            .cloned()
            .collect();
        children.sort();
        children
    }
}

/// A disk holding one program, where the platform expects it.
fn installed(path: &str) -> FakeTree {
    FakeTree::default().file(path)
}

fn missing() -> Scan {
    Scan::Missing {
        searched_downloads: false,
    }
}

#[test]
fn a_typed_file_named_like_the_program_is_the_answer() {
    let tree = installed("/opt/soloist/soloist");

    assert_eq!(
        scan(Some(Path::new("/opt/soloist/soloist")), None, None, &tree),
        Scan::Found(PathBuf::from("/opt/soloist/soloist"))
    );
}

#[test]
fn a_typed_file_with_another_name_is_never_run() {
    let tree = FakeTree::default().file("/tmp/holiday-photos.zip");

    assert_eq!(
        scan(
            Some(Path::new("/tmp/holiday-photos.zip")),
            None,
            None,
            &tree
        ),
        Scan::NotSoloist
    );
}

#[test]
fn a_typed_folder_holds_the_program() {
    let tree = FakeTree::default()
        .folder("/opt/soloist")
        .file("/opt/soloist/soloist");

    assert_eq!(
        scan(Some(Path::new("/opt/soloist")), None, None, &tree),
        Scan::Found(PathBuf::from("/opt/soloist/soloist"))
    );
}

#[test]
fn a_typed_folder_is_looked_in_one_level_only() {
    let tree = FakeTree::default()
        .folder("/opt")
        .folder("/opt/soloist")
        .folder("/opt/soloist/build")
        .file("/opt/soloist/build/soloist");

    assert_eq!(scan(Some(Path::new("/opt")), None, None, &tree), missing());
}

#[test]
fn a_typed_path_that_is_not_there_never_falls_through_to_the_search() {
    let tree = FakeTree::default().file("/usr/bin/soloist");

    assert_eq!(
        scan(
            Some(Path::new("/opt/typo/soloist")),
            Some(OsStr::new("/usr/bin")),
            Some(Path::new("/home/x/Downloads")),
            &tree
        ),
        missing(),
        "a path the listener named is answered by that path or by nothing"
    );
}

#[test]
fn a_folder_named_like_the_program_is_not_a_program() {
    let tree = FakeTree::default().folder("/opt/soloist");

    assert_eq!(
        scan(Some(Path::new("/opt/soloist")), None, None, &tree),
        missing()
    );
}

#[test]
fn an_empty_answer_asks_the_path_first() {
    let tree = FakeTree::default()
        .file("/usr/bin/soloist")
        .file("/home/x/Downloads/soloist");

    assert_eq!(
        scan(
            None,
            Some(OsStr::new("/usr/bin")),
            Some(Path::new("/home/x/Downloads")),
            &tree
        ),
        Scan::Found(PathBuf::from("/usr/bin/soloist"))
    );
}

#[test]
fn an_empty_answer_falls_back_to_downloads() {
    let tree = FakeTree::default().file("/home/x/Downloads/soloist");

    assert_eq!(
        scan(
            None,
            Some(OsStr::new("/nowhere")),
            Some(Path::new("/home/x/Downloads")),
            &tree
        ),
        Scan::Found(PathBuf::from("/home/x/Downloads/soloist"))
    );
}

#[test]
fn a_downloaded_archive_is_looked_in_one_level() {
    let tree = FakeTree::default()
        .folder("/home/x/Downloads/soloist-linux-x86_64")
        .file("/home/x/Downloads/soloist-linux-x86_64/soloist");

    assert_eq!(
        scan(
            None,
            Some(OsStr::new("")),
            Some(Path::new("/home/x/Downloads")),
            &tree
        ),
        Scan::Found(PathBuf::from(
            "/home/x/Downloads/soloist-linux-x86_64/soloist"
        ))
    );
}

#[test]
fn nothing_anywhere_says_whether_downloads_was_looked_in() {
    let tree = FakeTree::default().folder("/home/x/Downloads");
    let searched = Scan::Missing {
        searched_downloads: true,
    };

    assert_eq!(
        scan(
            None,
            Some(OsStr::new("")),
            Some(Path::new("/home/x/Downloads")),
            &tree
        ),
        searched
    );
    assert_eq!(
        scan(None, Some(OsStr::new("")), None, &tree),
        Scan::Missing {
            searched_downloads: false,
        },
        "without a Downloads folder there was nothing to look in"
    );
}

#[test]
fn the_program_is_named_for_the_platform() {
    let path = format!("/opt/soloist/{}", name());
    let tree = installed(&path);

    assert_eq!(
        scan(Some(Path::new(&path)), None, None, &tree),
        Scan::Found(PathBuf::from(&path)),
        "the platform's own suffix is the one accepted"
    );
}

#[test]
fn the_downloads_folder_sits_under_the_profile() {
    assert_eq!(
        downloads_dir(Some(OsStr::new("/home/x"))),
        Some(PathBuf::from("/home/x/Downloads"))
    );
    assert_eq!(
        downloads_dir(Some(OsStr::new(""))),
        None,
        "an empty profile names no folder"
    );
    assert_eq!(
        downloads_dir(None),
        None,
        "an unset profile names no folder"
    );
}

#[test]
fn the_real_disk_answers_about_the_files_it_has() {
    let disk = Disk;

    assert!(disk.is_file(Path::new("Cargo.toml")));
    assert!(disk.is_dir(Path::new("src")));
    assert!(!disk.is_file(Path::new("src")));
}

#[test]
fn the_archive_spotify_ships_names_the_program_after_the_archive() {
    // Spotify's Linux tar holds one file called `soloist_release_x86_64`: no
    // folder, no extension, and not the bare name the program is called.
    let tree = FakeTree::default()
        .folder("/home/x/Downloads")
        .file("/home/x/Downloads/soloist_release_x86_64");

    assert_eq!(
        scan(
            None,
            Some(OsStr::new("")),
            Some(Path::new("/home/x/Downloads")),
            &tree
        ),
        Scan::Found(PathBuf::from("/home/x/Downloads/soloist_release_x86_64")),
        "the extracted program beside the archive is the program"
    );
}

#[test]
fn a_gzipped_archive_untarred_once_is_not_the_program() {
    // Spotify's download is a gzip whose payload is a tar holding `soloist`.
    // Unwrapping only the gzip leaves a file called `soloist_release_x86_64`,
    // which starts with `CHANGELOG.md` and is not runnable however it is named.
    let tree = FakeTree::default()
        .folder("/home/x/Downloads")
        .blob("/home/x/Downloads/soloist_release_x86_64", TAR);

    assert_eq!(
        scan(
            None,
            Some(OsStr::new("")),
            Some(Path::new("/home/x/Downloads")),
            &tree
        ),
        Scan::Missing {
            searched_downloads: true
        },
        "a tar beside the download is not the program"
    );
}

#[test]
fn an_archive_is_never_taken_for_the_program() {
    let tree = FakeTree::default()
        .folder("/home/x/Downloads")
        .file("/home/x/Downloads/soloist_release_x86_64.tar")
        .file("/home/x/Downloads/soloist_release_x86_64.tar.gz")
        .file("/home/x/Downloads/soloist_release_windows_x86_64.zip");

    assert_eq!(
        scan(
            None,
            Some(OsStr::new("")),
            Some(Path::new("/home/x/Downloads")),
            &tree
        ),
        Scan::Missing {
            searched_downloads: true
        },
        "a packed archive starts with the name too, and is not a program"
    );
}

#[test]
fn a_typed_archive_is_refused_as_not_the_program() {
    let tree = FakeTree::default().file("/home/x/Downloads/soloist_release_x86_64.tar");

    assert_eq!(
        scan(
            Some(Path::new("/home/x/Downloads/soloist_release_x86_64.tar")),
            None,
            None,
            &tree
        ),
        Scan::NotSoloist,
        "it was named, so it answers rather than falling through to a search"
    );
}
