//! The daemon's command line, and starting it.
//!
//! The key is on the command line because that is the only way the daemon
//! takes it. It is therefore in the argv while the process runs, and it must
//! reach nothing else: no log line, no debug output, no error text.

use std::fmt;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use takofy_core::credentials::SoloistKey;

/// The address the daemon's own WebSocket binds to: the OS names the port.
pub const WS_ADDR: &str = "127.0.0.1:0";

/// How long `--version` may take before the program is given up on.
pub const VERSION_WAIT: Duration = Duration::from_secs(5);

/// How often a running `--version` is looked at.
const VERSION_POLL: Duration = Duration::from_millis(25);

/// A daemon command line, ready to be spawned.
///
/// The `Command` is private and the debug output is one word, because a
/// derived debug would print the key.
pub struct Launch {
    command: Command,
}

impl Launch {
    /// The daemon that serves playback, with its socket on a free port.
    pub fn new(exe: &Path, name: &str, key: &SoloistKey, data_dir: &Path) -> Self {
        let mut command = base(exe, name, key, data_dir);
        command.arg("--ws").arg(WS_ADDR);
        Self { command }
    }

    /// The one-shot run that pairs the daemon with the account and exits.
    pub fn pair(exe: &Path, name: &str, key: &SoloistKey, data_dir: &Path) -> Self {
        let mut command = base(exe, name, key, data_dir);
        command.arg("--pair");
        Self { command }
    }

    /// Starts the daemon. Nothing it prints is kept: the socket is the only
    /// thing this side listens to.
    pub fn spawn(mut self) -> Result<Child, SpawnError> {
        self.command.spawn().map_err(|error| SpawnError::of(&error))
    }

    /// Runs `--version` without the key and answers what it printed.
    pub fn version(exe: &Path) -> Result<String, VersionError> {
        Self::version_within(exe, VERSION_WAIT)
    }

    /// Runs `--version`, and gives up on a program that will not finish.
    ///
    /// The file is one the listener named, so it may be anything at all: a
    /// program that never exits is killed rather than holding the page that
    /// asked.
    pub fn version_within(exe: &Path, wait: Duration) -> Result<String, VersionError> {
        let mut child = Command::new(exe)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| VersionError::of(&error))?;
        let deadline = Instant::now()
            .checked_add(wait)
            .unwrap_or_else(Instant::now);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        return Err(VersionError::Other);
                    }
                    let mut printed = String::new();
                    if let Some(mut stdout) = child.stdout.take() {
                        stdout
                            .read_to_string(&mut printed)
                            .map_err(|_| VersionError::Other)?;
                    }
                    return Ok(printed);
                }
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(VersionError::Other);
                    }
                    std::thread::sleep(VERSION_POLL);
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(VersionError::of(&error));
                }
            }
        }
    }

    #[cfg(test)]
    pub fn argv(&self) -> Vec<String> {
        self.command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }
}

/// Never prints the key: the argv it would come from stays inside [`Launch`].
impl fmt::Debug for Launch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Launch(..)")
    }
}

/// The arguments both runs share.
fn base(exe: &Path, name: &str, key: &SoloistKey, data_dir: &Path) -> Command {
    let mut command = Command::new(exe);
    command
        .arg("-n")
        .arg(name)
        .arg("-k")
        .arg(key.expose())
        .arg("-D")
        .arg(data_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

/// Why the daemon did not start.
///
/// Kinds only: the underlying `io::Error` names a path, and an error text is
/// not the place for one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnError {
    NotFound,
    Denied,
    Other,
}

impl SpawnError {
    fn of(error: &std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::NotFound => Self::NotFound,
            std::io::ErrorKind::PermissionDenied => Self::Denied,
            _ => Self::Other,
        }
    }
}

impl fmt::Display for SpawnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotFound => "the soloist program was not found",
            Self::Denied => "the soloist program may not be run",
            Self::Other => "the soloist program could not be started",
        })
    }
}

impl std::error::Error for SpawnError {}

/// Why the daemon's build could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionError {
    NotFound,
    Denied,
    Other,
}

impl VersionError {
    fn of(error: &std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::NotFound => Self::NotFound,
            std::io::ErrorKind::PermissionDenied => Self::Denied,
            _ => Self::Other,
        }
    }
}

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotFound => "the soloist program was not found",
            Self::Denied => "the soloist program may not be run",
            Self::Other => "the soloist build could not be read",
        })
    }
}

impl std::error::Error for VersionError {}

#[cfg(test)]
mod tests;
