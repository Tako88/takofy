//! Finding the Soloist program.
//!
//! The configured path is the whole answer when it is set: a path that is not
//! there reads as missing rather than quietly searching elsewhere, so a typo in
//! the preference names itself.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// What the search found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Located {
    Found(PathBuf),
    Missing,
}

/// The name the program is installed under.
const NAME: &str = "soloist";

impl Located {
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Found(path) => Some(path),
            Self::Missing => None,
        }
    }
}

/// Whether `path` is a program this process could run.
pub fn exists(path: &Path) -> bool {
    path.is_file()
}

/// Finds the program: the preferred path first, then each `PATH` entry.
///
/// The file check is a parameter so a test names its own filesystem.
pub fn find(
    preferred: Option<&Path>,
    path_env: Option<&OsStr>,
    is_file: impl Fn(&Path) -> bool,
) -> Located {
    if let Some(preferred) = preferred {
        return if is_file(preferred) {
            Located::Found(preferred.to_path_buf())
        } else {
            Located::Missing
        };
    }
    let Some(path_env) = path_env else {
        return Located::Missing;
    };
    for entry in std::env::split_paths(path_env) {
        let candidate = entry.join(name());
        if is_file(&candidate) {
            return Located::Found(candidate);
        }
    }
    Located::Missing
}

/// The file name, with the suffix Windows needs on a `PATH` entry.
pub(crate) fn name() -> &'static str {
    if cfg!(windows) { "soloist.exe" } else { NAME }
}

#[cfg(test)]
mod tests;
