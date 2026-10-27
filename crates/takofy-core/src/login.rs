//! Login: resume from a stored refresh token, or fall back to the browser.

use std::fmt;

use crate::credentials::{AccessToken, CredentialStore, RefreshToken, StoreError};

/// An access token plus the refresh token that came with it, if any.
#[derive(Debug)]
pub struct TokenSet {
    pub access: AccessToken,
    pub refresh: Option<RefreshToken>,
}

/// Which path produced the access token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginMethod {
    Resumed,
    Interactive,
}

/// Why a [`TokenSource`] exchange failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceErrorKind {
    PortBusy,
    Other,
}

/// A failed token exchange. The message it prints is chosen here, not by the
/// source, whose own text can embed a response body or a callback URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceError {
    pub kind: SourceErrorKind,
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.kind {
            SourceErrorKind::PortBusy => "the sign-in port is already in use",
            SourceErrorKind::Other => "sign-in failed",
        })
    }
}

impl std::error::Error for SourceError {}

/// Exchanges credentials for an access token.
pub trait TokenSource {
    fn refresh(&self, token: &RefreshToken) -> Result<TokenSet, SourceError>;
    fn interactive(&self) -> Result<TokenSet, SourceError>;
}

/// Why [`authenticate`] failed.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("{0}")]
    Store(#[from] StoreError),
    #[error("{0}")]
    Source(#[from] SourceError),
    #[error("the browser login returned no refresh token")]
    MissingRefreshToken,
}

/// Loads a stored refresh token and refreshes it.
///
/// `Ok(None)` means nothing was stored, or the refresh failed: the caller must
/// fall back to [`authorize`].
pub fn resume(
    store: &dyn CredentialStore,
    source: &dyn TokenSource,
) -> Result<Option<AccessToken>, AuthError> {
    let Some(stored) = store.load()? else {
        return Ok(None);
    };
    let Ok(set) = source.refresh(&stored) else {
        return Ok(None);
    };
    if let Some(fresh) = &set.refresh {
        persist(store, Some(&stored), fresh)?;
    }
    Ok(Some(set.access))
}

/// Runs the interactive flow and stores the refresh token it returns.
pub fn authorize(
    store: &dyn CredentialStore,
    source: &dyn TokenSource,
) -> Result<AccessToken, AuthError> {
    let set = source.interactive()?;
    let refresh = set.refresh.ok_or(AuthError::MissingRefreshToken)?;
    persist(store, None, &refresh)?;
    Ok(set.access)
}

/// Stores `fresh` unless it is the token already on disk as `previous`.
fn persist(
    store: &dyn CredentialStore,
    previous: Option<&RefreshToken>,
    fresh: &RefreshToken,
) -> Result<(), StoreError> {
    if previous.is_some_and(|token| token.expose() == fresh.expose()) {
        return Ok(());
    }
    store.save(fresh)
}

/// Loads a stored refresh token, refreshes it, and only opens a browser if that
/// fails. The stored token is replaced only after a successful exchange.
pub fn authenticate(
    store: &dyn CredentialStore,
    source: &dyn TokenSource,
) -> Result<(AccessToken, LoginMethod), AuthError> {
    if let Some(access) = resume(store, source)? {
        return Ok((access, LoginMethod::Resumed));
    }
    let access = authorize(store, source)?;
    Ok((access, LoginMethod::Interactive))
}

#[cfg(test)]
mod tests;
