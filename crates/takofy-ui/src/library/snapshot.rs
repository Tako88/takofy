//! The five lists the library draws, as the binary saves them.
//!
//! Plain data over model rows: the window hands it out and takes it back, and
//! the binary is the only side that knows the file. A list is `None` when the
//! copy does not hold it, which is not the same as an empty list.

use takofy_model::{Account, FollowedArtist, FollowedShow, LikedTrack, PlaylistEntry, SavedAlbum};

/// The saved copy of the library, for one account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The account whose lists these are.
    pub account: Account,
    /// The sidebar's playlists.
    pub playlists: Option<Vec<PlaylistEntry>>,
    /// The followed shows.
    pub shows: Option<Vec<FollowedShow>>,
    /// The followed artists.
    pub artists: Option<Vec<FollowedArtist>>,
    /// The saved albums.
    pub albums: Option<Vec<SavedAlbum>>,
    /// The liked songs.
    pub liked: Option<Vec<LikedTrack>>,
}

impl Snapshot {
    /// An empty copy for `account`: no list held.
    pub fn new(account: Account) -> Self {
        Self {
            account,
            playlists: None,
            shows: None,
            artists: None,
            albums: None,
            liked: None,
        }
    }

    /// Whether any list holds a row.
    ///
    /// An empty copy says nothing about the library, so there is nothing worth
    /// writing; an empty list of one kind is still a list.
    pub fn has_rows(&self) -> bool {
        fn held<T>(rows: &Option<Vec<T>>) -> bool {
            rows.as_ref().is_some_and(|rows| !rows.is_empty())
        }
        held(&self.playlists)
            || held(&self.shows)
            || held(&self.artists)
            || held(&self.albums)
            || held(&self.liked)
    }
}

#[cfg(test)]
mod tests;
