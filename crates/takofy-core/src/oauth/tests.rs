use std::io;
use std::net::SocketAddr;
use std::time::Instant;

use librespot::core::SessionConfig;
use librespot::oauth::{OAuthError, OAuthToken};

use super::{
    OAUTH_LOG_TARGET, REDIRECT_LOOPBACK, REDIRECT_PASTE, SCOPES, has_explicit_port,
    map_oauth_error, token_set,
};
use crate::login::{AuthError, SourceErrorKind};

fn oauth_token(access: &str, refresh: &str) -> OAuthToken {
    OAuthToken {
        access_token: access.to_owned(),
        refresh_token: refresh.to_owned(),
        expires_at: Instant::now(),
        token_type: "Bearer".to_owned(),
        scopes: Vec::new(),
    }
}

#[test]
fn loopback_redirect_has_explicit_port() {
    assert!(has_explicit_port(REDIRECT_LOOPBACK));
}

#[test]
fn paste_redirect_has_no_port() {
    assert!(!has_explicit_port(REDIRECT_PASTE));
}

#[test]
fn scopes_include_streaming() {
    assert!(SCOPES.contains(&"streaming"));
}

#[test]
fn empty_refresh_token_maps_to_none() {
    let set = token_set(oauth_token("access", "")).expect("non-empty access");
    assert!(set.refresh.is_none());
}

#[test]
fn empty_access_token_maps_to_error() {
    let error = token_set(oauth_token("", "refresh")).expect_err("empty access");
    assert_eq!(error.kind, SourceErrorKind::Other);
}

#[test]
fn listener_bind_error_maps_to_port_busy() {
    let addr: SocketAddr = "127.0.0.1:8898".parse().expect("socket address");
    let error = OAuthError::AuthCodeListenerBind {
        addr,
        e: io::Error::new(io::ErrorKind::AddrInUse, "busy"),
    };
    assert_eq!(map_oauth_error(&error).kind, SourceErrorKind::PortBusy);
}

/// The exchange's response text can carry the code and the token, so the
/// message a failed exchange prints is authored here instead.
#[test]
fn a_failed_exchange_never_surfaces_the_response_text() {
    let error = OAuthError::ExchangeCode {
        e: "code=AUTHCODE123 access_token=TOKEN456".to_owned(),
    };

    let source = map_oauth_error(&error);
    let printed = AuthError::Source(source.clone()).to_string();

    for text in [source.to_string(), printed] {
        assert_eq!(text, "sign-in failed");
        assert!(
            !text.contains("AUTHCODE123"),
            "must not print the code: {text}"
        );
        assert!(
            !text.contains("TOKEN456"),
            "must not print the token: {text}"
        );
    }
}

#[test]
fn default_client_id_is_the_registered_one() {
    assert_eq!(
        SessionConfig::default().client_id,
        "65b708073fc0480ea92a077233ca87bd"
    );
}

#[test]
fn log_target_matches_the_crate() {
    let name = std::any::type_name::<OAuthError>();
    assert!(name.starts_with("librespot_oauth::"));
    assert_eq!(
        OAUTH_LOG_TARGET,
        name.split("::").next().expect("crate path")
    );
}
