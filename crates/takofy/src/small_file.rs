//! Reading and writing a small file, with a size bound and an atomic replace.
//!
//! The files the app keeps in its config and cache directories are read and
//! written through here: a whole file is read or refused, never half-read, and
//! a write lands whole or not at all. On unix the written file is owner-only,
//! so a file that carries account-scoped data is not readable by other users of
//! the machine.

use std::fs::{self, File};
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};

/// What a bounded read found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    /// There is no file yet.
    Absent,
    /// The file could not be read, or its bytes are not text.
    Unreadable,
    /// The file is larger than the caller's bound.
    TooLarge,
    /// The file's whole text.
    Text(String),
}

/// Reads `path` whole, up to `limit` bytes.
pub fn read_text(path: &Path, limit: u64) -> Read {
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Read::Absent,
        Err(_) => return Read::Unreadable,
    };
    if !meta.is_file() {
        return Read::Unreadable;
    }
    let file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return Read::Unreadable,
    };
    let mut bytes = Vec::new();
    if file
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .is_err()
    {
        return Read::Unreadable;
    }
    if bytes.len() as u64 > limit {
        return Read::TooLarge;
    }
    match String::from_utf8(bytes) {
        Ok(text) => Read::Text(text),
        Err(_) => Read::Unreadable,
    }
}

/// Writes `text` to `path` through a temporary file beside it.
///
/// A reader sees the old file or the new one, never half of either. The
/// temporary name is per process, so two writers in one process would collide;
/// production has exactly one writer per file.
pub fn write(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".tmp.{}", std::process::id()));
    let temp = PathBuf::from(name);
    let written = write_then_rename(&temp, path, text);
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

fn write_then_rename(temp: &Path, path: &Path, text: &str) -> io::Result<()> {
    let mut file = create_private(temp)?;
    file.write_all(text.as_bytes())?;
    drop(file);
    fs::rename(temp, path)
}

#[cfg(unix)]
fn create_private(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_private(path: &Path) -> io::Result<File> {
    File::create(path)
}

#[cfg(test)]
mod tests;
