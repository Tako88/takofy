//! Session errors mapped onto the failures the window can name.

use takofy_core::playback::PlaybackError;
use takofy_ui::message::Failure;

use crate::session::SessionError;

/// The failure the window shows for a likeness read: its kind reaches the log,
/// nothing else does.
pub(super) fn related_failure(error: &SessionError) -> Failure {
    log::warn!("the likeness read failed: {}", error.kind());
    Failure::Related
}

/// Whether this failure means the device can no longer play at all.
///
/// One track that will not play is not that: the player is up and the next one
/// may play, so the announced stage must stay where it was.
pub(super) fn ends_playability(error: &SessionError) -> bool {
    matches!(
        error,
        SessionError::NotConnected | SessionError::Playback(PlaybackError::NoAudioDevice)
    )
}

/// A playback failure the bar can name, or `Other` when it cannot. The reason
/// is for the log, never the window.
pub(crate) fn playback_failure(error: &SessionError) -> Failure {
    log::warn!("playback failure: {}", error.kind());
    match error {
        SessionError::Playback(PlaybackError::NoAudioDevice) => Failure::NoAudioDevice,
        SessionError::LinkLost => Failure::Reconnect,
        _ => Failure::Other,
    }
}

/// Every session-side failure reads the same to the window; the reason is for
/// the log.
pub(crate) fn library_failure(error: &SessionError) -> Failure {
    log::warn!("session failure: {}", error.kind());
    Failure::Load
}

/// The followed-shows list has its own failure: the playlists beside it draw
/// fine, so the library did not fail. The reason is for the log, never the
/// window.
pub(crate) fn followed_failure(error: &SessionError) -> Failure {
    log::warn!("session failure: {}", error.kind());
    Failure::Followed
}

/// The followed-artists list has its own failure: the playlists beside it draw
/// fine, so the library did not fail. The reason is for the log, never the
/// window.
pub(crate) fn artists_failure(error: &SessionError) -> Failure {
    log::warn!("session failure: {}", error.kind());
    Failure::FollowedArtists
}

/// The saved-albums list has its own failure: the playlists beside it draw
/// fine, so the library did not fail. The reason is for the log, never the
/// window.
pub(crate) fn albums_failure(error: &SessionError) -> Failure {
    log::warn!("session failure: {}", error.kind());
    Failure::SavedAlbums
}

/// A liked-songs read has no surface of its own; its failure is the library's
/// load. The reason is for the log, never the window.
pub(crate) fn liked_failure(error: &SessionError) -> Failure {
    log::warn!("session failure: {}", error.kind());
    Failure::Load
}

/// The lyrics pane has its own failure: the page around it may draw fine, and
/// its message names only the lyrics. The reason is for the log, never the
/// window.
pub(crate) fn lyrics_failure(error: &SessionError) -> Failure {
    log::warn!("session failure: {}", error.kind());
    Failure::Lyrics
}

/// A search has its own failure: the menu and the page name only the search,
/// and the field stays editable so typing retries. The reason is for the log,
/// never the window.
pub(crate) fn search_failure(error: &SessionError) -> Failure {
    log::warn!("search failure: {}", error.kind());
    Failure::Search
}
