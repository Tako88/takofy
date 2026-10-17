//! Plain data shared by the engine and the window.
//!
//! Both `takofy-core` and `takofy-ui` depend on this crate, and it depends on
//! nothing. That is the point: the window can hold what it draws without
//! depending on the crate that talks to Spotify, so "a view cannot reach the
//! network" stays enforced by the compiler.

/// A playlist as listed in the rootlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistEntry {
    pub uri: String,
    pub name: String,
    /// `false` when `name` is the URI fallback.
    pub named: bool,
}

/// A track inside a playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackEntry {
    pub uri: String,
    pub title: String,
    pub artists: String,
    /// `false` when `title` is the URI fallback.
    pub named: bool,
}
