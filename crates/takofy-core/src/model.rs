//! Plain view types for playlists and tracks.

/// A playlist as listed in the rootlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistEntry {
    pub uri: String,
    pub name: String,
}

/// A track inside a playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackEntry {
    pub uri: String,
    pub title: String,
    pub artists: String,
}
