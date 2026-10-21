//! The vocabulary the window and its worker exchange.
//!
//! These types carry state, never credentials: `takofy-ui` has no path to a
//! token or an auth error, so the compiler keeps secrets out of a view.

use takofy_model::{
    AlbumDetail, ArtistDetail, CoverImage, CoverUrl, FollowedArtist, FollowedShow, PlaylistAccess,
    PlaylistEntry, Revision, SavedAlbum, ShowDetail, TrackEntry, TrackLyrics,
};

/// A request from the window to the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Begin the interactive sign-in flow.
    Authorize,
    /// Load the tracks of one playlist.
    LoadTracks { playlist: String },
    /// Move one row of a playlist to a new place, based on `revision`.
    MoveTrack {
        playlist: String,
        revision: Revision,
        from: usize,
        to: usize,
    },
    /// Move one row of the sidebar's playlists to a new place, based on
    /// `revision`.
    MovePlaylist {
        revision: Revision,
        from: usize,
        to: usize,
    },
    /// Append one track to a playlist, based on `revision`.
    AddTrack {
        playlist: String,
        revision: Revision,
        from_index: usize,
        track: String,
    },
    /// Load one album with its tracks.
    LoadAlbum { album: String },
    /// Load one artist, its discography and its followed state.
    LoadArtist { artist: String },
    /// Load one show with its episodes.
    LoadShow { show: String },
    /// Label one release's track URIs, in the release's own order.
    LoadReleaseTracks {
        release: String,
        tracks: Vec<String>,
    },
    /// Write the artist into the user's followed set, or out of it.
    SetFollow { artist: String, followed: bool },
    /// Load one track's lyrics.
    LoadLyrics { track: String },
    /// Load the shows the user follows, as the library root draws them.
    LoadFollowedShows,
    /// Load the artists the user follows, as the library root draws them.
    LoadFollowedArtists,
    /// The artists an artist is heard with, by the artist they belong to.
    LoadRelated { artist: String, uris: Vec<String> },
    /// Load the albums the user saved, as the library root draws them.
    LoadSavedAlbums,
    /// The tracks one search query answers.
    Search { query: String },
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
    /// The shows the user follows could not be loaded.
    Followed,
    /// The artists the user follows could not be loaded.
    FollowedArtists,
    /// The artists an artist is heard with.
    Related,
    /// The albums the user saved could not be loaded.
    SavedAlbums,
    /// A track's lyrics could not be loaded.
    Lyrics,
    /// A search's results could not be loaded.
    Search,
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
    /// What the sidebar may write with, sent right after `Playlists` and only
    /// when the read named a revision.
    ///
    /// Its `editable` means the sidebar's rows are the wire's own indices, so a
    /// row index may be written as a wire index.
    RootlistAccess(PlaylistAccess),
    /// The sidebar's playlists moved, and this is the revision the next write
    /// must name.
    PlaylistMoved { revision: Revision },
    /// A sidebar move did not stick.
    PlaylistMoveFailed,
    /// A track was appended to a playlist, and this is the revision the next
    /// write must name.
    TrackAdded {
        playlist: String,
        revision: Revision,
    },
    /// A track could not be appended to a playlist.
    AddFailed { playlist: String },
    /// A playlist's tracks, plus the context's own image when it carried one.
    Tracks {
        playlist: String,
        tracks: Vec<TrackEntry>,
        cover: Option<CoverUrl>,
        /// What the page may change, when the list's own state could be read.
        access: Option<PlaylistAccess>,
    },
    /// A playlist row moved, and this is the revision the next write must name.
    TrackMoved {
        playlist: String,
        revision: Revision,
    },
    /// A playlist move did not stick.
    MoveFailed { playlist: String, failure: Failure },
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
    /// One show, its episodes in the show's own order.
    Show(ShowDetail),
    /// A show could not be loaded.
    ShowFailed { show: String, failure: Failure },
    /// The shows the user follows, newest follow first.
    FollowedShows(Vec<FollowedShow>),
    /// The followed shows could not be loaded.
    FollowedShowsFailed(Failure),
    /// The artists the user follows, as the library root draws them.
    FollowedArtists(Vec<FollowedArtist>),
    /// The likeness cards for one artist, in the payload's own order.
    Related {
        artist: String,
        cards: Vec<FollowedArtist>,
    },
    /// The likeness cards could not be loaded.
    RelatedFailed { artist: String, failure: Failure },
    /// The followed artists could not be loaded.
    FollowedArtistsFailed(Failure),
    /// The albums the user saved, as the library root draws them.
    SavedAlbums(Vec<SavedAlbum>),
    /// The saved albums could not be loaded.
    SavedAlbumsFailed(Failure),
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
    /// One track's lyrics.
    Lyrics { track: String, lyrics: TrackLyrics },
    /// A track's lyrics could not be loaded.
    LyricsFailed { track: String, failure: Failure },
    /// The tracks one search query answered, echoed with the query that asked.
    Search {
        query: String,
        tracks: Vec<TrackEntry>,
    },
    /// A search's results could not be loaded; echoed with the query that asked.
    SearchFailed { query: String, failure: Failure },
    /// A playback change.
    Transport(TransportUpdate),
}
