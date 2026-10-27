//! The sign-in worker: starting it, and turning an auth result into an update.

use std::sync::Arc;

use takofy_core::connect::WITHDRAW_TIMEOUT;
use takofy_core::credentials::{AccessToken, CredentialStore};
use takofy_core::login::{AuthError, SourceErrorKind, TokenSource, authorize};
use takofy_ui::message::{Failure, Update};
use tokio::runtime::Handle;
use tokio::sync::mpsc::UnboundedSender;

use super::send_update;
use super::start_with;
use super::wake::{Notifier, UiEnd, WakeSlot, Worker};
use crate::recovery::Pacing;
use crate::session::SessionHost;

/// Starts the sign-in worker and kicks off the resume attempt immediately, so
/// it overlaps window creation.
pub(crate) fn start<H: SessionHost, S: TokenSource + Send + Sync + 'static>(
    handle: &Handle,
    host: H,
    store: Arc<dyn CredentialStore>,
    source: Arc<S>,
) -> (UiEnd, WakeSlot, Worker) {
    start_with(handle, host, store, source, WITHDRAW_TIMEOUT, Pacing::LIVE)
}

/// Runs the authorize flow off the runtime and hands the task its token.
pub(super) fn spawn_authorize<S: TokenSource + Send + Sync + 'static>(
    notifier: Notifier,
    tokens: UnboundedSender<AccessToken>,
    store: Arc<dyn CredentialStore>,
    source: Arc<S>,
) {
    std::thread::spawn(move || {
        let result = authorize(store.as_ref(), source.as_ref());
        if let Ok(token) = &result
            && tokens.send(token.clone()).is_err()
        {
            return;
        }
        send_update(&notifier, authorize_update(result));
    });
}

/// Collapses an auth failure to a variant that carries no detail, because some
/// source errors embed a response body.
pub(crate) fn failure_of(error: &AuthError) -> Failure {
    match error {
        AuthError::Store(_) => Failure::Keychain,
        AuthError::MissingRefreshToken => Failure::NoRefreshToken,
        AuthError::Source(source) if source.kind == SourceErrorKind::PortBusy => Failure::PortBusy,
        AuthError::Source(_) => Failure::Other,
    }
}

/// Turns a resume result into an update. The token is consumed, never shown.
pub(crate) fn resume_update(result: Result<Option<AccessToken>, AuthError>) -> Update {
    match result {
        Ok(Some(_)) => Update::SignedIn,
        Ok(None) => Update::NeedsAuthorization,
        Err(error) => Update::SignInFailed(failure_of(&error)),
    }
}

/// Turns an authorize result into an update. The token is consumed, never shown.
pub(crate) fn authorize_update(result: Result<AccessToken, AuthError>) -> Update {
    match result {
        Ok(_) => Update::SignedIn,
        Err(error) => Update::SignInFailed(failure_of(&error)),
    }
}
