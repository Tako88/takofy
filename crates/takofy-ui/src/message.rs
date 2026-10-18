//! The vocabulary the window and its worker exchange.
//!
//! These types carry state, never credentials: `takofy-ui` has no path to a
//! token or an auth error, so the compiler keeps secrets out of a view.

use takofy_model::{PlaylistEntry, TrackEntry};

/// A request from the window to the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Begin the interactive sign-in flow.
    Authorize,
    /// Load the tracks of one playlist.
    LoadTracks { playlist: String },
    /// Start playing a track.
    Play { uri: String },
    /// Pause playback.
    Pause,
    /// Resume playback.
    Resume,
    /// Stop playback.
    Stop,
    /// Jump to `position_ms` within the loaded track.
    Seek { position_ms: u32 },
    /// Fetch the playlists again.
    ReloadLibrary,
}

/// Why an operation failed, with no field that could carry a secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The loopback callback port is already in use.
    PortBusy,
    /// The credential store could not be read or written.
    Keychain,
    /// The provider returned no refresh token.
    NoRefreshToken,
    /// No audio output device is available.
    NoAudioDevice,
    /// The library could not be loaded.
    Load,
    /// Anything else; reproduce with `--headless` for the message.
    Other,
}

/// A playback change the worker reports to the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportUpdate {
    /// A track is being loaded.
    Loading,
    /// Playback is at `position_ms`; `paused` says whether it is running.
    Position { position_ms: u32, paused: bool },
    /// The track reached its end.
    Ended,
    /// The track could not be played.
    Unavailable,
    /// Playback was stopped on request.
    Stopped,
    /// A track is loaded and this is how long it is.
    Duration { duration_ms: u32 },
    /// The player jumped to `position_ms` within the loaded track.
    Seeked { position_ms: u32 },
    /// The command could not be carried out; carries no detail.
    Failed(Failure),
}

/// A state change sent from the worker to the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    /// A usable token is available.
    SignedIn,
    /// No token is stored, or the stored one could not be refreshed.
    NeedsAuthorization,
    /// An attempt failed and may be retried.
    SignInFailed(Failure),
    /// The user's playlists.
    Playlists(Vec<PlaylistEntry>),
    /// The playlists could not be loaded.
    PlaylistFailed(Failure),
    /// A playlist's tracks.
    Tracks {
        playlist: String,
        tracks: Vec<TrackEntry>,
    },
    /// A playlist's tracks could not be loaded.
    TracksFailed { playlist: String, failure: Failure },
    /// A playback change.
    Transport(TransportUpdate),
}
