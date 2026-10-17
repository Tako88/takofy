//! The vocabulary the window and its worker exchange.
//!
//! These types carry state, never credentials: `takofy-ui` has no path to a
//! token or an auth error, so the compiler keeps secrets out of a view.

/// A request from the window to the worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Begin the interactive sign-in flow.
    Authorize,
}

/// Why a sign-in attempt failed, with no field that could carry a secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The loopback callback port is already in use.
    PortBusy,
    /// The credential store could not be read or written.
    Keychain,
    /// The provider returned no refresh token.
    NoRefreshToken,
    /// Anything else; reproduce with `--headless` for the message.
    Other,
}

/// A state change sent from the worker to the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Update {
    /// A usable token is available.
    SignedIn,
    /// No token is stored, or the stored one could not be refreshed.
    NeedsAuthorization,
    /// An attempt failed and may be retried.
    SignInFailed(Failure),
}
