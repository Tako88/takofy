//! librespot-oauth adapter: redirect constants, scopes and error mapping.

use librespot::oauth::{OAuthClient, OAuthClientBuilder, OAuthError, OAuthToken};

use crate::credentials::{AccessToken, RefreshToken};
use crate::login::{SourceError, SourceErrorKind, TokenSet, TokenSource};

pub const REDIRECT_LOOPBACK: &str = "http://127.0.0.1:8898/login";
pub const REDIRECT_PASTE: &str = "http://127.0.0.1/login";

/// Least-privilege subset of librespot's own scope list.
pub const SCOPES: [&str; 4] = [
    "streaming",
    "playlist-read-private",
    "playlist-read-collaborative",
    "user-library-read",
];

/// The log target librespot-oauth logs tokens and auth codes under.
pub const OAUTH_LOG_TARGET: &str = "librespot_oauth";

const CALLBACK_MESSAGE: &str = "Login complete. You can close this tab and return to Takofy.";

/// Which redirect URI the browser flow uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectMode {
    Loopback,
    Paste,
}

impl RedirectMode {
    fn uri(self) -> &'static str {
        match self {
            RedirectMode::Loopback => REDIRECT_LOOPBACK,
            RedirectMode::Paste => REDIRECT_PASTE,
        }
    }
}

/// A [`TokenSource`] backed by librespot's PKCE flow.
pub struct SpotifyOAuth {
    client_id: String,
    redirect: RedirectMode,
}

impl SpotifyOAuth {
    pub fn new(client_id: &str, redirect: RedirectMode) -> Self {
        Self {
            client_id: client_id.to_owned(),
            redirect,
        }
    }

    fn client(&self, open: bool) -> Result<OAuthClient, SourceError> {
        let builder =
            OAuthClientBuilder::new(&self.client_id, self.redirect.uri(), SCOPES.to_vec());
        let builder = if open {
            builder
                .open_in_browser()
                .with_custom_message(CALLBACK_MESSAGE)
        } else {
            builder
        };
        builder.build().map_err(|error| map_oauth_error(&error))
    }
}

impl TokenSource for SpotifyOAuth {
    fn refresh(&self, token: &RefreshToken) -> Result<TokenSet, SourceError> {
        self.client(false)?
            .refresh_token(token.expose())
            .map_err(|error| map_oauth_error(&error))
            .and_then(token_set)
    }

    fn interactive(&self) -> Result<TokenSet, SourceError> {
        self.client(true)?
            .get_access_token()
            .map_err(|error| map_oauth_error(&error))
            .and_then(token_set)
    }
}

/// Maps an empty refresh token to `None`; an empty access token is an error.
fn token_set(token: OAuthToken) -> Result<TokenSet, SourceError> {
    let access = AccessToken::new(token.access_token).ok_or(SourceError {
        kind: SourceErrorKind::Other,
    })?;
    Ok(TokenSet {
        access,
        refresh: RefreshToken::new(token.refresh_token),
    })
}

fn map_oauth_error(error: &OAuthError) -> SourceError {
    let kind = match error {
        OAuthError::AuthCodeListenerBind { .. } => SourceErrorKind::PortBusy,
        _ => SourceErrorKind::Other,
    };
    SourceError { kind }
}

/// Mirrors librespot-oauth's `get_socket_address`: only `http` URIs with an
/// explicit numeric port get a loopback listener; portless ones fall back to
/// the stdin prompt.
#[cfg(test)]
fn has_explicit_port(uri: &str) -> bool {
    let Some(rest) = uri.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && !port.is_empty() => {
            port.chars().all(|c| c.is_ascii_digit())
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests;
