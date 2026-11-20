//! Spotify Soloist: the second sound path.
//!
//! The protocol lives here and nowhere else. A process, a WebSocket and a
//! platform belong to the binary; this half only turns frames into the shared
//! playback vocabulary.

use std::fmt;

pub mod discovery;
pub mod ending;
pub mod expiry;
pub mod frames;
pub mod position;
pub mod sequence;
pub mod sound;
pub mod state;

pub use sound::{Connection, SoloistSound};

/// The log target every Soloist line is filed under.
///
/// Lines carry a fault kind only: never a frame, a track URI or a key.
pub const SOLOIST_LOG_TARGET: &str = "takofy_core::soloist";

/// Why Soloist could not play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoloistFault {
    /// The program was not found.
    Missing,
    /// The program is not running.
    NotRunning,
    /// The program opened no socket to talk to.
    NoSocket,
    /// The daemon has not been paired with the account.
    Unpaired,
    /// The build has expired.
    Expired,
    /// The daemon refused a command.
    Refused,
    /// The connection to the daemon was lost.
    Lost,
}

/// Never names a key, an argv or a track: this is drawn in the window.
impl fmt::Display for SoloistFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Missing => "the soloist program was not found",
            Self::NotRunning => "the soloist process is not running",
            Self::NoSocket => "the soloist process opened no socket",
            Self::Unpaired => "soloist is not paired with the account",
            Self::Expired => "the soloist build has expired",
            Self::Refused => "soloist refused the command",
            Self::Lost => "the connection to soloist was lost",
        })
    }
}
