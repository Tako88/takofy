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
        Failure::Keychain => "Could not read or save your sign-in. Check your system keychain.",
        Failure::NoRefreshToken => "Spotify did not return a refresh token. Try again.",
        Failure::Other => "Sign-in failed. Try again, or run with --headless for details.",
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
            state.apply(update);
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

    #[test]
    fn failure_message_is_non_empty_and_distinct_per_failure() {
        let mut messages = Vec::new();
        for failure in [
            Failure::PortBusy,
            Failure::Keychain,
            Failure::NoRefreshToken,
            Failure::Other,
        ] {
            let message = failure_message(failure);
            assert!(!message.trim().is_empty(), "{failure:?} has no message");
            assert!(
                !messages.contains(&message),
                "{failure:?} repeats another message"
            );
            messages.push(message);
        }
    }
}
