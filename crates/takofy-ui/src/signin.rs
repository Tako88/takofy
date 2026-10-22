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
                | Update::SavedRemoved { .. }
                | Update::SavedRemoveFailed { .. }
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
                | Update::Cover { .. }
                | Update::CoverFailed { .. }
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
mod tests {
    use super::*;

    #[test]
    fn starting_then_signed_in_is_signed_in() {
        let mut state = SignIn::Starting;
        state.apply(Update::SignedIn);
        assert_eq!(state, SignIn::SignedIn);
    }

    #[test]
    fn needs_authorization_then_signed_in_is_signed_in() {
        let mut state = SignIn::NeedsAuthorization;
        state.apply(Update::SignedIn);
        assert_eq!(state, SignIn::SignedIn);
    }

    #[test]
    fn authorizing_then_signed_in_is_signed_in() {
        let mut state = SignIn::Authorizing;
        state.apply(Update::SignedIn);
        assert_eq!(state, SignIn::SignedIn);
    }

    #[test]
    fn failed_then_signed_in_is_signed_in() {
        let mut state = SignIn::Failed(Failure::Other);
        state.apply(Update::SignedIn);
        assert_eq!(state, SignIn::SignedIn);
    }

    #[test]
    fn starting_then_needs_authorization_is_needs_authorization() {
        let mut state = SignIn::Starting;
        state.apply(Update::NeedsAuthorization);
        assert_eq!(state, SignIn::NeedsAuthorization);
    }

    #[test]
    fn starting_then_sign_in_failed_is_failed() {
        let mut state = SignIn::Starting;
        state.apply(Update::SignInFailed(Failure::PortBusy));
        assert_eq!(state, SignIn::Failed(Failure::PortBusy));
    }

    #[test]
    fn sign_in_failed_while_authorizing_is_failed() {
        let mut state = SignIn::Authorizing;
        state.apply(Update::SignInFailed(Failure::Keychain));
        assert_eq!(state, SignIn::Failed(Failure::Keychain));
    }

    #[test]
    fn signed_in_ignores_everything_after() {
        for update in [
            Update::SignedIn,
            Update::NeedsAuthorization,
            Update::SignInFailed(Failure::Other),
        ] {
            let mut state = SignIn::SignedIn;
            state.apply(update.clone());
            assert_eq!(
                state,
                SignIn::SignedIn,
                "{update:?} moved a signed-in state"
            );
        }
    }

    #[test]
    fn needs_authorization_ignores_needs_authorization() {
        let mut state = SignIn::NeedsAuthorization;
        state.apply(Update::NeedsAuthorization);
        assert_eq!(state, SignIn::NeedsAuthorization);
    }

    #[test]
    fn needs_authorization_is_ignored_while_authorizing() {
        let mut state = SignIn::Authorizing;
        state.apply(Update::NeedsAuthorization);
        assert_eq!(state, SignIn::Authorizing);
    }

    #[test]
    fn needs_authorization_is_ignored_after_a_failure() {
        let mut state = SignIn::Failed(Failure::PortBusy);
        state.apply(Update::NeedsAuthorization);
        assert_eq!(state, SignIn::Failed(Failure::PortBusy));
    }

    #[test]
    fn sign_in_failed_is_ignored_while_waiting_for_authorization() {
        let mut state = SignIn::NeedsAuthorization;
        state.apply(Update::SignInFailed(Failure::PortBusy));
        assert_eq!(state, SignIn::NeedsAuthorization);
    }

    #[test]
    fn sign_in_failed_is_ignored_after_a_failure() {
        let mut state = SignIn::Failed(Failure::PortBusy);
        state.apply(Update::SignInFailed(Failure::Other));
        assert_eq!(state, SignIn::Failed(Failure::PortBusy));
    }

    #[test]
    fn authorize_only_from_needs_authorization_or_failed() {
        for mut state in [
            SignIn::NeedsAuthorization,
            SignIn::Failed(Failure::PortBusy),
        ] {
            assert_eq!(state.authorize(), Some(Command::Authorize));
            assert_eq!(state, SignIn::Authorizing, "{state:?} did not start");
        }

        for mut state in [SignIn::Starting, SignIn::Authorizing, SignIn::SignedIn] {
            assert_eq!(state.authorize(), None, "{state:?} accepted a command");
        }
    }

    #[test]
    fn authorize_twice_yields_one_command() {
        let mut state = SignIn::NeedsAuthorization;
        assert_eq!(state.authorize(), Some(Command::Authorize));
        assert_eq!(state.authorize(), None);
        assert_eq!(state, SignIn::Authorizing);
    }

    #[test]
    fn can_authorize_matches_authorize() {
        for mut state in [
            SignIn::Starting,
            SignIn::NeedsAuthorization,
            SignIn::Authorizing,
            SignIn::SignedIn,
            SignIn::Failed(Failure::Other),
        ] {
            let expected = state.can_authorize();
            let got = state.authorize();
            assert_eq!(expected, got.is_some(), "{state:?}");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn keychain_failure_names_the_service_and_both_desktops() {
        let message = failure_message(Failure::Keychain);
        assert!(
            message.contains("org.freedesktop.secrets"),
            "the message must name the D-Bus service: {message}"
        );
        for desktop in ["GNOME", "Plasma", "Hyprland", "i3", "sway"] {
            assert!(
                message.contains(desktop),
                "{desktop} must be distinguished: {message}"
            );
        }
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn keychain_failure_names_no_linux_service_off_linux() {
        assert!(
            !failure_message(Failure::Keychain).contains("org.freedesktop.secrets"),
            "the fallback must not name a Linux service"
        );
    }

    #[test]
    fn keychain_failure_carries_no_secret() {
        let message = failure_message(Failure::Keychain);
        for forbidden in ["token", "refresh", "password", "auth code"] {
            assert!(
                !message.contains(forbidden),
                "the message must not carry {forbidden}: {message}"
            );
        }
    }

    #[test]
    fn failure_message_is_non_empty_and_distinct_per_failure() {
        let mut messages = Vec::new();
        for failure in every_failure() {
            let message = failure_message(failure);
            assert!(!message.trim().is_empty(), "{failure:?} has no message");
            assert!(
                !messages.contains(&message),
                "{failure:?} repeats another message"
            );
            messages.push(message);
        }
    }

    /// Every `SignIn` value there is, so a test can sweep them all.
    fn every_state() -> Vec<SignIn> {
        let mut states = vec![
            SignIn::Starting,
            SignIn::NeedsAuthorization,
            SignIn::Authorizing,
            SignIn::SignedIn,
        ];
        states.extend(every_failure().into_iter().map(SignIn::Failed));
        states
    }

    /// Every `Failure` value there is, so a test can sweep them all.
    fn every_failure() -> [Failure; 13] {
        [
            Failure::PortBusy,
            Failure::Keychain,
            Failure::NoRefreshToken,
            Failure::NoAudioDevice,
            Failure::Load,
            Failure::Save,
            Failure::Rename,
            Failure::Followed,
            Failure::FollowedArtists,
            Failure::SavedAlbums,
            Failure::Lyrics,
            Failure::Search,
            Failure::Other,
        ]
    }

    #[test]
    fn failure_message_for_followed_is_its_own() {
        let message = failure_message(Failure::Followed);
        assert!(
            message.contains("shows") && message.contains("follow"),
            "the copy must name the list that failed: {message}"
        );
        assert!(
            !message.contains("library"),
            "the playlists beside it still draw, so the library did not fail: {message}"
        );
    }

    #[test]
    fn failure_message_for_followed_artists_is_its_own() {
        let message = failure_message(Failure::FollowedArtists);
        assert!(
            message.contains("artists") && message.contains("follow"),
            "the copy must name the list that failed: {message}"
        );
        assert!(
            !message.contains("library") && !message.contains("shows"),
            "the playlists and shows beside it still draw: {message}"
        );
    }

    #[test]
    fn failure_message_for_saved_albums_is_its_own() {
        let message = failure_message(Failure::SavedAlbums);
        assert!(
            message.contains("albums") && message.contains("saved"),
            "the copy must name the list that failed: {message}"
        );
        assert!(
            !message.contains("library") && !message.contains("shows"),
            "the playlists and shows beside it still draw: {message}"
        );
    }

    #[test]
    fn a_save_failure_says_the_change_did_not_stick() {
        let message = failure_message(Failure::Save);
        assert!(
            message.contains("save"),
            "the copy must name the write that failed: {message}"
        );
        assert!(
            !message.contains("Sign-in") && !message.contains("sign in"),
            "a save failure is not a sign-in failure: {message}"
        );
    }

    #[test]
    fn every_state_has_its_copy_decided() {
        // `copy` has no wildcard arm, so a new `SignIn` variant does not
        // compile until it is decided. This pins the decision for the states
        // that exist today: the prompt and the failures draw words, the two
        // progress states and the signed-in shell draw none.
        for state in every_state() {
            let expected = matches!(state, SignIn::NeedsAuthorization | SignIn::Failed(_));
            assert_eq!(
                copy(state).is_some(),
                expected,
                "{state:?} has the wrong copy presence"
            );
        }

        assert_eq!(
            copy(SignIn::NeedsAuthorization),
            Some(Words {
                heading: "Takofy",
                body: "Sign in with Spotify to continue.",
                label: "Authorize",
            })
        );
    }

    #[test]
    fn failed_copy_carries_the_failure_message() {
        for failure in every_failure() {
            let words = copy(SignIn::Failed(failure)).expect("a failure has copy");
            assert_eq!(
                words.body,
                failure_message(failure),
                "{failure:?} must show what failure_message writes"
            );
        }
    }

    #[test]
    fn copy_carries_no_secret_term() {
        // `copy` is `&'static str`, so no runtime value can be interpolated;
        // the words are compiled in. The remaining risk is a credential-shaped
        // field name or literal in the wording. `refresh token` as a phrase is
        // allowed: `NoRefreshToken` names the missing thing, not its value.
        let forbidden = [
            "Bearer",
            "access_token",
            "refresh_token",
            "client_secret",
            "authorization code",
            "password",
            "eyJ",
        ];
        for state in every_state() {
            let Some(words) = copy(state) else {
                continue;
            };
            for text in [words.heading, words.body, words.label] {
                for term in forbidden {
                    assert!(!text.contains(term), "{state:?} would echo {term}: {text}");
                }
            }
        }
    }
}
