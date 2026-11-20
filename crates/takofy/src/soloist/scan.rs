//! Where the Soloist program is, and whether a path the listener named is it.
//!
//! A typed path is the whole answer: it resolves, or it reports that nothing is
//! there, and it never falls through to a search, so a typo names itself. An
//! empty answer asks for the search instead: the `PATH`, then Downloads.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::locate::{self, Located};

/// What a search found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scan {
    /// The program is here.
    Found(PathBuf),
    /// Nothing is here. Whether Downloads was looked in says what the page can
    /// suggest next.
    Missing { searched_downloads: bool },
    /// The named file is not Spotify Soloist, so it is never run.
    NotSoloist,
}

/// The disk, as a scan sees it.
///
/// The real one is [`Disk`]; a test names its own files without touching any.
pub trait Tree {
    fn is_file(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    fn children(&self, folder: &Path) -> Vec<PathBuf>;
}

/// The filesystem this process is running on.
pub struct Disk;

impl Tree for Disk {
    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn children(&self, folder: &Path) -> Vec<PathBuf> {
        let mut children: Vec<PathBuf> = std::fs::read_dir(folder)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .collect()
            })
            .unwrap_or_default();
        children.sort();
        children
    }
}

/// Finds the program.
///
/// `typed` is what the listener named: `Some` is the whole answer, `None` asks
/// for the search.
pub fn scan(
    typed: Option<&Path>,
    path_env: Option<&OsStr>,
    downloads: Option<&Path>,
    tree: &impl Tree,
) -> Scan {
    if let Some(typed) = typed {
        return typed_path(typed, tree);
    }
    if let Located::Found(found) = locate::find(None, path_env, |candidate| tree.is_file(candidate))
    {
        return Scan::Found(found);
    }
    let Some(downloads) = downloads else {
        return Scan::Missing {
            searched_downloads: false,
        };
    };
    match in_folder(downloads, tree) {
        Some(found) => Scan::Found(found),
        None => Scan::Missing {
            searched_downloads: true,
        },
    }
}

/// The folder a browser saves downloads in, given the profile folder.
///
/// The caller reads the platform's variable: `USERPROFILE` on Windows, `HOME`
/// elsewhere. An empty or unset value names no folder.
pub fn downloads_dir(profile: Option<&OsStr>) -> Option<PathBuf> {
    let profile = profile.filter(|value| !value.is_empty())?;
    Some(Path::new(profile).join("Downloads"))
}

fn typed_path(typed: &Path, tree: &impl Tree) -> Scan {
    if tree.is_file(typed) {
        return if named_like_program(typed) {
            Scan::Found(typed.to_path_buf())
        } else {
            Scan::NotSoloist
        };
    }
    if tree.is_dir(typed) {
        return match in_folder(typed, tree) {
            Some(found) => Scan::Found(found),
            None => Scan::Missing {
                searched_downloads: false,
            },
        };
    }
    Scan::Missing {
        searched_downloads: false,
    }
}

/// Whether the file is named like the program, which is the only file this
/// process ever runs.
///
/// Spotify's archive ships the program as `soloist_release_x86_64` on Linux and
/// `soloist.exe` on Windows, so the name is a hint rather than the truth: any
/// name that starts with the program's, carrying either no extension or the
/// Windows one. An archive is not the program — `soloist_release_x86_64.tar`
/// starts with the name too, and running it would produce nothing.
fn named_like_program(path: &Path) -> bool {
    let Some(stem) = path.file_stem().and_then(OsStr::to_str) else {
        return false;
    };
    if !stem.to_ascii_lowercase().starts_with(locate::NAME) {
        return false;
    }
    match path.extension().and_then(OsStr::to_str) {
        None => true,
        Some(extension) => extension.eq_ignore_ascii_case("exe"),
    }
}

/// The program in `folder`, in a folder one level inside it, or among its files.
///
/// The extracted archive is one file beside everything else in Downloads, so a
/// child file counts as much as a child folder does.
fn in_folder(folder: &Path, tree: &impl Tree) -> Option<PathBuf> {
    let direct = folder.join(locate::name());
    if tree.is_file(&direct) {
        return Some(direct);
    }
    for child in tree.children(folder) {
        if tree.is_file(&child) {
            if named_like_program(&child) {
                return Some(child);
            }
            continue;
        }
        if !tree.is_dir(&child) {
            continue;
        }
        for nested in tree.children(&child) {
            if tree.is_file(&nested) && named_like_program(&nested) {
                return Some(nested);
            }
        }
        let candidate = child.join(locate::name());
        if tree.is_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests;
