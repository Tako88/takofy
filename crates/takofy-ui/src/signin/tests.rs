use crate::message::DeviceUpdate;

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
fn a_portrait_reply_is_not_sign_in_news() {
    for update in [
        Update::Portraits {
            query: "slow tide".to_owned(),
            portraits: Vec::new(),
        },
        Update::PortraitsFailed {
            query: "slow tide".to_owned(),
        },
    ] {
        for mut state in [
            SignIn::Starting,
            SignIn::NeedsAuthorization,
            SignIn::Authorizing,
            SignIn::SignedIn,
            SignIn::Failed(Failure::PortBusy),
        ] {
            let before = state;
            state.apply(update.clone());
            assert_eq!(state, before, "{update:?} moved {before:?}");
        }
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
fn a_device_update_never_moves_the_sign_in() {
    for state in every_state() {
        let mut applied = state;
        applied.apply(Update::Devices(DeviceUpdate::Failed(Failure::Devices)));
        assert_eq!(applied, state, "{state:?} moved on a device update");
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
fn every_failure() -> [Failure; 20] {
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
        Failure::Related,
        Failure::SavedAlbums,
        Failure::Lyrics,
        Failure::Search,
        Failure::Devices,
        Failure::DevicesOff,
        Failure::Reconnect,
        Failure::Steer,
        Failure::Transfer,
        Failure::NoContext,
        Failure::Other,
    ]
}

#[test]
fn failure_message_for_reconnect_names_the_connection_and_no_secret() {
    let message = failure_message(Failure::Reconnect);
    assert!(
        message.contains("connect") && message.contains("restart"),
        "the copy must name what is being tried and what the listener can do: {message}"
    );
    for term in ["token", "refresh", "password", "auth code"] {
        assert!(
            !message.contains(term),
            "the copy would echo {term}: {message}"
        );
    }
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
