//! The vocabulary the window and its worker exchange.
//!
//! These types carry state, never credentials: `takofy-ui` has no path to a
//! token or an auth error, so the compiler keeps secrets out of a view.

use takofy_model::{AlbumDetail, ArtistDetail, CoverImage, CoverUrl, PlaylistEntry, TrackEntry};

/// A request from the window to the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Begin the interactive sign-in flow.
    Authorize,
    /// Load the tracks of one playlist.
    LoadTracks { playlist: String },
    /// Load one album with its tracks.
    LoadAlbum { album: String },
    /// Load one artist, its discography and its followed state.
    LoadArtist { artist: String },
    /// Label one release's track URIs, in the release's own order.
    LoadReleaseTracks {
        release: String,
        tracks: Vec<String>,
    },
    /// Write the artist into the user's followed set, or out of it.
    SetFollow { artist: String, followed: bool },
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
    /// Fetch one cover image from the worker.
    Fetch { url: CoverUrl },
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
    /// A write the user asked for did not stick.
    Save,
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
    /// A playlist's tracks, plus the context's own image when it carried one.
    Tracks {
        playlist: String,
        tracks: Vec<TrackEntry>,
        cover: Option<CoverUrl>,
    },
    /// A playlist's tracks could not be loaded.
    TracksFailed { playlist: String, failure: Failure },
    /// One album, its tracks in the album's own order.
    Album(AlbumDetail),
    /// An album could not be loaded.
    AlbumFailed { album: String, failure: Failure },
    /// One artist, its top tracks and its discography.
    Artist(ArtistDetail),
    /// An artist could not be loaded.
    ArtistFailed { artist: String, failure: Failure },
    /// One release's labelled tracks, in the release's own order.
    ReleaseTracks {
        release: String,
        tracks: Vec<TrackEntry>,
    },
    /// One release's tracks could not be loaded.
    ReleaseTracksFailed { release: String, failure: Failure },
    /// The followed state the server confirmed.
    Followed { artist: String, followed: bool },
    /// A follow or unfollow write did not stick.
    FollowFailed { artist: String, failure: Failure },
    /// A decoded cover, ready to upload.
    Cover { url: CoverUrl, image: CoverImage },
    /// A cover could not be fetched or decoded.
    CoverFailed { url: CoverUrl },
    /// A playback change.
    Transport(TransportUpdate),
}
