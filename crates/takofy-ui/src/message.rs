//! The vocabulary the window and its worker exchange.
//!
//! These types carry state, never credentials: `takofy-ui` has no path to a
//! token or an auth error, so the compiler keeps secrets out of a view.

use takofy_model::{
    AlbumDetail, ArtistDetail, ArtistPortrait, CoverImage, CoverUrl, Device, FollowedArtist,
    FollowedShow, Landing, Move, PlaylistAccess, PlaylistEntry, RemotePlayback, Removal, Revision,
    SavedAlbum, ShowDetail, TrackEntry, TrackLyrics,
};

/// A request from the window to the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Begin the interactive sign-in flow.
    Authorize,
    /// Load the tracks of one playlist.
    LoadTracks { playlist: String },
    /// Move rows of a playlist to a new place, as one change set on `revision`.
    MoveTrack {
        playlist: String,
        revision: Revision,
        moves: Vec<Move>,
    },
    /// Remove rows of a playlist, as one change set on `revision`.
    RemoveTracks {
        playlist: String,
        revision: Revision,
        /// Highest index first, so each removal reads against the list the one
        /// before it left.
        removals: Vec<Removal>,
    },
    /// Move one row of the sidebar's playlists to a new place, based on
    /// `revision`.
    MovePlaylist {
        revision: Revision,
        from: usize,
        to: usize,
    },
    /// Unlink one row of the sidebar's playlists, based on `revision`.
    ///
    /// The rootlist unlinks only, so the playlist survives the write.
    RemovePlaylist {
        playlist: String,
        revision: Revision,
        index: usize,
    },
    /// Put one playlist at the front of the sidebar's list, based on `revision`.
    ///
    /// The rootlist only links; the playlist itself is untouched.
    LinkPlaylist {
        playlist: String,
        revision: Revision,
    },
    /// Ask whether one album or show is in the library.
    LoadSaved { uri: String },
    /// Write one album or show into the library, or out of it.
    ///
    /// An artist is not saved this way: it is followed, through
    /// [`Command::SetFollow`].
    SetSaved { uri: String, saved: bool },
    /// Append tracks to a playlist, in the order given, based on `revision`.
    AddTracks {
        playlist: String,
        revision: Revision,
        from_index: usize,
        tracks: Vec<String>,
    },
    /// Rename one playlist, based on the revision its own state read answers.
    RenamePlaylist { playlist: String, name: String },
    /// Create a playlist called `name` and put it at the front of the user's
    /// rootlist, based on the rootlist's `revision`, with `tracks` to land in
    /// it once it exists.
    CreatePlaylist {
        name: String,
        revision: Revision,
        tracks: Vec<String>,
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
    /// Load the tracks the user has liked, newest save first.
    LoadLikedTracks,
    /// Write these tracks into the liked-songs set, or out of it.
    SetLiked { tracks: Vec<String>, liked: bool },
    /// The tracks one search query answers.
    Search { query: String },
    /// The portraits of the artists a search answer draws.
    LoadPortraits { query: String, artists: Vec<String> },
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
    /// Set the player's level. 0 is silence, 100 is unity gain.
    Volume { percent: u16 },
    /// Start watching the account's devices.
    WatchDevices,
    /// Ask the account for its devices again.
    ///
    /// The announce's reply is the only way to learn about a device that
    /// appeared after launch.
    RefreshDevices,
    /// Send one transport action to the device that holds the sound.
    Steer { device: String, steer: Steer },
    /// Label the track another device is playing.
    LabelRemote { track: String },
    /// Label these track URIs, in the order the caller listed them.
    LabelRows { uris: Vec<String> },
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
    /// A playlist rename did not stick, and the field stays open.
    Rename,
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
    /// The account's devices could not be reached.
    Devices,
    /// The devices cannot be reached without a restart.
    DevicesOff,
    /// A transport action did not reach the device holding the sound.
    Steer,
    /// Anything else; reproduce with `--headless` for the message.
    Other,
}

/// One transport action sent to the device that holds the sound.
///
/// A copy of what `takofy-core` speaks, because the window does not link it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Steer {
    /// Stop the other device. A repeat is harmless: it names a state, not a step.
    Pause,
    /// Start the other device.
    Resume,
    /// Skip one track forward, which may be sent only once.
    SkipNext,
    /// Skip one track back, which may be sent only once.
    SkipPrev,
    /// Jump to a position in the track the other device is playing.
    SeekTo(u32),
}

/// A change to the account's devices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceUpdate {
    /// The whole roster, the device holding the sound when one does, and
    /// whether that device is this window.
    Roster {
        devices: Vec<Device>,
        holder: Option<String>,
        held_here: bool,
    },
    /// What the holder is playing, or `None` when nothing is.
    Playing(Option<RemotePlayback>),
    /// The track the holder is playing, named.
    Labelled(TrackEntry),
    /// The devices could not be reached.
    Failed(Failure),
    /// A transport action did not reach its device.
    SteerFailed,
}

/// A change on the account's device side of the receiver.
///
/// A landing names the activation it belongs to, so a loss that crossed it in
/// flight can be told apart from one it followed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteUpdate {
    /// A hand-over landed: these rows, and this one is current.
    Landed(Landing),
    /// Another device took the sound, at this activation epoch.
    Lost { epoch: u64 },
    /// The rows a label named.
    Labelled(Vec<TrackEntry>),
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
    /// A playlist left the rootlist, and this is the revision the next write
    /// must name.
    PlaylistRemoved {
        playlist: String,
        revision: Revision,
    },
    /// The playlist could not be unlinked, so it stays in the sidebar.
    PlaylistRemoveFailed { playlist: String },
    /// A playlist re-joined the rootlist, and this is the revision the next write
    /// must name.
    PlaylistLinked {
        playlist: String,
        revision: Revision,
    },
    /// The playlist could not be put on the rootlist, so the library is as it was.
    PlaylistLinkFailed { playlist: String },
    /// Whether one album or show is in the library. `saved` is `None` when the
    /// probe did not answer.
    SavedState { uri: String, saved: Option<bool> },
    /// A saved album or show entered the library, or left it. The reply carried
    /// no body, so this is the write's own echo.
    Saved { uri: String, saved: bool },
    /// The saved album or show could not be written, so it is where it was.
    SavedFailed { uri: String, saved: bool },
    /// A track was appended to a playlist, and this is the revision the next
    /// write must name.
    TrackAdded {
        playlist: String,
        revision: Revision,
    },
    /// A track could not be appended to a playlist.
    AddFailed { playlist: String },
    /// A playlist exists but the pick it was created for did not land in it.
    PlaylistUnfilled { playlist: String },
    /// A playlist exists but the rename that gives it a name did not land, so it
    /// keeps the typed name for this session only.
    PlaylistUnnamed { playlist: String },
    /// A playlist was renamed, and this is the revision the next write names.
    PlaylistRenamed {
        playlist: String,
        name: String,
        revision: Revision,
    },
    /// A playlist rename did not stick, so the field stays open with its text.
    PlaylistRenameFailed { playlist: String },
    /// A playlist was created and linked, and this is the row to draw first.
    ///
    /// The revision is the link reply's, so the next rootlist write names the
    /// list the link left behind.
    PlaylistCreated {
        entry: PlaylistEntry,
        revision: Revision,
    },
    /// The playlist itself could not be created, so nothing exists.
    PlaylistCreateFailed,
    /// The playlist exists but could not be put on the rootlist.
    PlaylistUnlinked,
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
    /// Playlist rows were removed, and this is the revision the next write must
    /// name.
    TracksRemoved {
        playlist: String,
        revision: Revision,
    },
    /// A playlist removal did not stick.
    RemoveFailed { playlist: String, failure: Failure },
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
    /// The tracks the user has liked, newest save first.
    LikedTracks(Vec<String>),
    /// The liked tracks could not be loaded; a set already read is kept.
    LikedTracksFailed(Failure),
    /// The liked write landed for these tracks, with this value.
    Liked { tracks: Vec<String>, liked: bool },
    /// The liked write did not stick.
    LikeFailed {
        tracks: Vec<String>,
        failure: Failure,
    },
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
    /// The portraits of the artists a search answer draws, echoed with the
    /// query that asked.
    Portraits {
        query: String,
        portraits: Vec<ArtistPortrait>,
    },
    /// The portraits could not be read; echoed with the query that asked.
    ///
    /// Silent on screen: a portrait that does not arrive is drawn like a cover
    /// that has not been fetched.
    PortraitsFailed { query: String },
    /// A playback change.
    Transport(TransportUpdate),
    /// A change to the account's devices.
    Devices(DeviceUpdate),
    /// A change the receiver made to what this window is playing.
    Remote(RemoteUpdate),
}
