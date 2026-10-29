//! The sign-in state machine and the messages it shows.
//!
//! The state is kept apart from drawing so the transitions can be tested
//! without a display, and so the view only ever reads a value it was handed.

use crate::message::{Command, Failure, Update};

/// Where the sign-in flow is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SignIn {
    /// Resuming a stored token; nothing to show but progress.
    #[default]
    Starting,
    /// No token; the user must authorize.
    NeedsAuthorization,
    /// An authorization is in flight.
    Authorizing,
    /// A usable token is available.
    SignedIn,
    /// The last attempt failed and may be retried.
    Failed(Failure),
}

impl SignIn {
    /// Folds one worker update into the state.
    ///
    /// `SignedIn` is terminal: a success wins from any other state, and nothing
    /// moves a signed-in window back. Failures only matter before the first
    /// success, and a stray one never overrides a state that is already
    /// waiting for the user or being retried.
    pub fn apply(&mut self, update: Update) {
        match (update, *self) {
            (Update::SignedIn, SignIn::SignedIn) => {}
            (Update::SignedIn, _) => *self = SignIn::SignedIn,
            (Update::NeedsAuthorization, SignIn::Starting) => {
                *self = SignIn::NeedsAuthorization;
            }
            (Update::NeedsAuthorization, _) => {}
            (Update::SignInFailed(failure), SignIn::Starting | SignIn::Authorizing) => {
                *self = SignIn::Failed(failure);
            }
            (Update::SignInFailed(_), _) => {}
            (
                Update::Playlists(_)
                | Update::PlaylistFailed(_)
                | Update::RootlistAccess(_)
                | Update::PlaylistMoved { .. }
                | Update::PlaylistMoveFailed
                | Update::PlaylistRemoved { .. }
                | Update::PlaylistRemoveFailed { .. }
                | Update::PlaylistLinked { .. }
                | Update::PlaylistLinkFailed { .. }
                | Update::Saved { .. }
                | Update::SavedState { .. }
                | Update::SavedFailed { .. }
                | Update::TrackAdded { .. }
                | Update::AddFailed { .. }
                | Update::PlaylistCreated { .. }
                | Update::PlaylistCreateFailed
                | Update::PlaylistUnlinked
                | Update::PlaylistUnfilled { .. }
                | Update::PlaylistUnnamed { .. }
                | Update::PlaylistRenamed { .. }
                | Update::PlaylistRenameFailed { .. }
                | Update::Tracks { .. }
                | Update::TrackMoved { .. }
                | Update::MoveFailed { .. }
                | Update::TracksRemoved { .. }
                | Update::RemoveFailed { .. }
                | Update::TracksFailed { .. }
                | Update::Album(_)
                | Update::AlbumFailed { .. }
                | Update::Artist(_)
                | Update::ArtistFailed { .. }
                | Update::Show(_)
                | Update::ShowFailed { .. }
                | Update::FollowedShows(_)
                | Update::FollowedShowsFailed(_)
                | Update::FollowedArtists(_)
                | Update::FollowedArtistsFailed(_)
                | Update::SavedAlbums(_)
                | Update::SavedAlbumsFailed(_)
                | Update::LikedTracks(_)
                | Update::LikedTracksFailed(_)
                | Update::SavedLabelled { .. }
                | Update::SavedLabelFailed { .. }
                | Update::Home(_)
                | Update::HomeFailed { .. }
                | Update::Liked { .. }
                | Update::LikeFailed { .. }
                | Update::Related { .. }
                | Update::RelatedFailed { .. }
                | Update::ReleaseTracks { .. }
                | Update::ReleaseTracksFailed { .. }
                | Update::Followed { .. }
                | Update::FollowFailed { .. }
                | Update::Lyrics { .. }
                | Update::LyricsFailed { .. }
                | Update::Search { .. }
                | Update::SearchFailed { .. }
                | Update::Portraits { .. }
                | Update::PortraitsFailed { .. }
                | Update::Cover { .. }
                | Update::CoverFailed { .. }
                | Update::Devices(_)
                | Update::Remote(_)
                | Update::Transport(_),
                _,
            ) => {}
        }
    }

    /// Starts an authorization, unless one is already running or the window is
    /// already signed in.
    pub fn authorize(&mut self) -> Option<Command> {
        if self.can_authorize() {
            *self = SignIn::Authorizing;
            Some(Command::Authorize)
        } else {
            None
        }
    }

    /// Whether an "Authorize" affordance should be shown.
    pub fn can_authorize(&self) -> bool {
        matches!(self, SignIn::NeedsAuthorization | SignIn::Failed(_))
    }
}

/// Operator-facing text for the last failure.
pub fn failure_message(failure: Failure) -> &'static str {
    match failure {
        Failure::PortBusy => "The sign-in port is in use. Close the other app and try again.",
        Failure::Keychain => keychain_message(),
        Failure::NoRefreshToken => "Spotify did not return a refresh token. Try again.",
        Failure::NoAudioDevice => "No audio output device is available.",
        Failure::Load => "Could not load the library. Check your connection and try again.",
        Failure::Save => "Could not save that change. Try again.",
        Failure::Rename => "Could not rename that playlist. Try again.",
        Failure::Followed => {
            "Could not load the shows you follow. Check your connection and try again."
        }
        Failure::FollowedArtists => {
            "Could not load the artists you follow. Check the connection and try again."
        }
        Failure::SavedAlbums => {
            "Could not load your saved albums. Check the connection and try again."
        }
        Failure::Lyrics => "Could not load the lyrics. Check your connection and try again.",
        Failure::Search => "Could not search. Check your connection and try again.",
        Failure::Related => {
            "Could not load the artists this one is heard with. Check the connection and try again."
        }
        Failure::Devices => "Could not reach your devices. Check your connection and try again.",
        Failure::DevicesOff => "Could not reach your devices. Restart Takofy to try again.",
        Failure::Reconnect => {
            "Lost the connection to Spotify. Takofy keeps trying to reconnect; if this stays, \
             restart Takofy."
        }
        Failure::Steer => "That device did not respond. Try again.",
        Failure::Transfer => "Could not move the sound. Try again.",
        Failure::NoContext => "Play a list first: there is nothing to hand over.",
        Failure::Other => "Sign-in failed. Try again, or run with --headless for details.",
    }
}

/// Linux has a D-Bus service to name; other targets have nothing verifiable to add.
#[cfg(target_os = "linux")]
fn keychain_message() -> &'static str {
    "Try signing in again. The system keychain needs the Secret Service provider \
     org.freedesktop.secrets on D-Bus: check it is enabled on GNOME or Plasma, or \
     start one on a bare compositor such as Hyprland, i3 or sway. A provider that \
     is running but locked can also cause this."
}

#[cfg(not(target_os = "linux"))]
fn keychain_message() -> &'static str {
    "Try signing in again. The system keychain could not be used."
}

/// The words the sign-in screen shows for one state.
///
/// Every field is `&'static str`: the copy is compiled in and never
/// interpolated from runtime state, so no field can carry a credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    /// The heading above the body.
    pub heading: &'static str,
    /// The explanatory line under the heading.
    pub body: &'static str,
    /// The text on the control that advances the flow.
    pub label: &'static str,
}

/// The copy for one sign-in state, or `None` where the state draws no words.
///
/// `Starting` and `Authorizing` show a spinner and nothing else, and `SignedIn`
/// hands the window to the shell. The match has no wildcard arm, so adding a
/// `SignIn` variant is a compile error until its copy is decided here.
pub fn copy(state: SignIn) -> Option<Words> {
    match state {
        SignIn::Starting | SignIn::Authorizing | SignIn::SignedIn => None,
        SignIn::NeedsAuthorization => Some(Words {
            heading: "Takofy",
            body: "Sign in with Spotify to continue.",
            label: "Authorize",
        }),
        SignIn::Failed(failure) => Some(Words {
            heading: "Could not sign in",
            body: failure_message(failure),
            label: "Try again",
        }),
    }
}

#[cfg(test)]
mod tests;
