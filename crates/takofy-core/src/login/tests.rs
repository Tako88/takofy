use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{
    AuthError, LoginMethod, SourceError, SourceErrorKind, TokenSet, TokenSource, authenticate,
    authorize, resume,
};
use crate::credentials::fake::FakeStore;
use crate::credentials::{AccessToken, CredentialStore, RefreshToken};

fn access(value: &str) -> AccessToken {
    AccessToken::new(value.to_owned()).expect("non-empty access token")
}

fn refresh(value: &str) -> RefreshToken {
    RefreshToken::new(value.to_owned()).expect("non-empty refresh token")
}

fn other_error() -> SourceError {
    SourceError {
        kind: SourceErrorKind::Other,
    }
}

type Response = Result<TokenSet, SourceError>;

#[derive(Default)]
struct ScriptedSource {
    refresh_result: Mutex<Option<Response>>,
    interactive_result: Mutex<Option<Response>>,
    refresh_calls: AtomicUsize,
    interactive_calls: AtomicUsize,
}

impl ScriptedSource {
    fn new() -> Self {
        Self::default()
    }

    fn on_refresh(self, result: Response) -> Self {
        *self.refresh_result.lock().expect("lock") = Some(result);
        self
    }

    fn on_interactive(self, result: Response) -> Self {
        *self.interactive_result.lock().expect("lock") = Some(result);
        self
    }

    fn refresh_calls(&self) -> usize {
        self.refresh_calls.load(Ordering::SeqCst)
    }

    fn interactive_calls(&self) -> usize {
        self.interactive_calls.load(Ordering::SeqCst)
    }
}

impl TokenSource for ScriptedSource {
    fn refresh(&self, _token: &RefreshToken) -> Result<TokenSet, SourceError> {
        self.refresh_calls.fetch_add(1, Ordering::SeqCst);
        self.refresh_result
            .lock()
            .expect("lock")
            .take()
            .expect("refresh was scripted")
    }

    fn interactive(&self) -> Result<TokenSet, SourceError> {
        self.interactive_calls.fetch_add(1, Ordering::SeqCst);
        self.interactive_result
            .lock()
            .expect("lock")
            .take()
            .expect("interactive was scripted")
    }
}

#[test]
fn no_stored_token_goes_interactive_and_saves() {
    let store = FakeStore::new();
    let source = ScriptedSource::new().on_interactive(Ok(TokenSet {
        access: access("access-1"),
        refresh: Some(refresh("refresh-1")),
    }));

    let (token, method) = authenticate(&store, &source).expect("login succeeds");

    assert_eq!(token.expose(), "access-1");
    assert_eq!(method, LoginMethod::Interactive);
    assert_eq!(source.refresh_calls(), 0);
    assert_eq!(source.interactive_calls(), 1);
    assert_eq!(store.save_count(), 1);
    assert_eq!(
        store.load().expect("load").expect("token").expose(),
        "refresh-1"
    );
}

#[test]
fn stored_token_resumes_without_browser() {
    let store = FakeStore::seeded(refresh("stored"));
    let source = ScriptedSource::new().on_refresh(Ok(TokenSet {
        access: access("access-2"),
        refresh: None,
    }));

    let (token, method) = authenticate(&store, &source).expect("login succeeds");

    assert_eq!(token.expose(), "access-2");
    assert_eq!(method, LoginMethod::Resumed);
    assert_eq!(source.refresh_calls(), 1);
    assert_eq!(source.interactive_calls(), 0);
    assert_eq!(store.save_count(), 0);
    assert_eq!(
        store.load().expect("load").expect("token").expose(),
        "stored"
    );
}

#[test]
fn unchanged_resumed_token_is_not_saved() {
    let store = FakeStore::seeded(refresh("same"));
    let source = ScriptedSource::new().on_refresh(Ok(TokenSet {
        access: access("access-6"),
        refresh: Some(refresh("same")),
    }));

    let (_, method) = authenticate(&store, &source).expect("login succeeds");

    assert_eq!(method, LoginMethod::Resumed);
    assert_eq!(store.save_count(), 0);
    assert_eq!(store.load().expect("load").expect("token").expose(), "same");
}

#[test]
fn rotated_refresh_token_is_saved() {
    let store = FakeStore::seeded(refresh("old"));
    let source = ScriptedSource::new().on_refresh(Ok(TokenSet {
        access: access("access-3"),
        refresh: Some(refresh("new")),
    }));

    let (_, method) = authenticate(&store, &source).expect("login succeeds");

    assert_eq!(method, LoginMethod::Resumed);
    assert_eq!(store.save_count(), 1);
    assert_eq!(store.load().expect("load").expect("token").expose(), "new");
}

#[test]
fn refresh_failure_falls_back_and_overwrites() {
    let store = FakeStore::seeded(refresh("old"));
    let source = ScriptedSource::new()
        .on_refresh(Err(other_error()))
        .on_interactive(Ok(TokenSet {
            access: access("access-4"),
            refresh: Some(refresh("new")),
        }));

    let (_, method) = authenticate(&store, &source).expect("login succeeds");

    assert_eq!(method, LoginMethod::Interactive);
    assert_eq!(source.refresh_calls(), 1);
    assert_eq!(source.interactive_calls(), 1);
    assert_eq!(store.load().expect("load").expect("token").expose(), "new");
}

#[test]
fn failed_interactive_leaves_old_token_in_place() {
    let store = FakeStore::seeded(refresh("old"));
    let source = ScriptedSource::new()
        .on_refresh(Err(other_error()))
        .on_interactive(Err(other_error()));

    let error = authenticate(&store, &source).expect_err("login fails");

    assert!(matches!(error, AuthError::Source(_)));
    assert_eq!(store.load().expect("load").expect("token").expose(), "old");
    assert_eq!(store.save_count(), 0);
}

#[test]
fn interactive_without_refresh_token_errors_and_saves_nothing() {
    let store = FakeStore::new();
    let source = ScriptedSource::new().on_interactive(Ok(TokenSet {
        access: access("access-5"),
        refresh: None,
    }));

    let error = authenticate(&store, &source).expect_err("login fails");

    assert!(matches!(error, AuthError::MissingRefreshToken));
    assert_eq!(store.save_count(), 0);
}

#[test]
fn store_load_failure_never_touches_the_source() {
    let store = FakeStore::new();
    store.fail_load();
    let source = ScriptedSource::new();

    let error = authenticate(&store, &source).expect_err("login fails");

    assert!(matches!(error, AuthError::Store(_)));
    assert_eq!(source.refresh_calls(), 0);
    assert_eq!(source.interactive_calls(), 0);
}

#[test]
fn store_save_failure_is_fatal() {
    let store = FakeStore::new();
    store.fail_save();
    let source = ScriptedSource::new().on_interactive(Ok(TokenSet {
        access: access("access-6"),
        refresh: Some(refresh("refresh-6")),
    }));

    let error = authenticate(&store, &source).expect_err("login fails");

    assert!(matches!(error, AuthError::Store(_)));
}

#[test]
fn resume_path_save_failure_is_fatal() {
    let store = FakeStore::seeded(refresh("old"));
    store.fail_save();
    let source = ScriptedSource::new().on_refresh(Ok(TokenSet {
        access: access("access-7"),
        refresh: Some(refresh("new")),
    }));

    let error = authenticate(&store, &source).expect_err("login fails");

    assert!(matches!(error, AuthError::Store(_)));
}

#[test]
fn debug_hides_token_secrets() {
    let token_set = TokenSet {
        access: access("super-secret-access"),
        refresh: Some(refresh("super-secret-refresh")),
    };

    let debug = format!("{token_set:?}");

    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("super-secret"));
}

#[test]
fn source_errors_display_an_authored_message_per_kind() {
    let busy = SourceError {
        kind: SourceErrorKind::PortBusy,
    };
    let other = SourceError {
        kind: SourceErrorKind::Other,
    };

    assert_eq!(busy.to_string(), "the sign-in port is already in use");
    assert_eq!(other.to_string(), "sign-in failed");
}

#[test]
fn port_busy_kind_is_preserved() {
    let store = FakeStore::new();
    let source = ScriptedSource::new().on_interactive(Err(SourceError {
        kind: SourceErrorKind::PortBusy,
    }));

    let error = authenticate(&store, &source).expect_err("login fails");

    match error {
        AuthError::Source(source) => assert_eq!(source.kind, SourceErrorKind::PortBusy),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn resume_without_stored_token_is_none_and_never_calls_source() {
    let store = FakeStore::new();
    let source = ScriptedSource::new();

    let token = resume(&store, &source).expect("resume succeeds");

    assert!(token.is_none());
    assert_eq!(source.refresh_calls(), 0);
    assert_eq!(source.interactive_calls(), 0);
    assert_eq!(store.save_count(), 0);
}

#[test]
fn resume_refresh_failure_is_none_and_store_untouched() {
    let store = FakeStore::seeded(refresh("old"));
    let source = ScriptedSource::new().on_refresh(Err(other_error()));

    let token = resume(&store, &source).expect("resume succeeds");

    assert!(token.is_none());
    assert_eq!(source.refresh_calls(), 1);
    assert_eq!(source.interactive_calls(), 0);
    assert_eq!(store.save_count(), 0);
    assert_eq!(store.load().expect("load").expect("token").expose(), "old");
}

#[test]
fn resume_saves_only_a_rotated_token() {
    let unchanged = FakeStore::seeded(refresh("same"));
    let source = ScriptedSource::new().on_refresh(Ok(TokenSet {
        access: access("access-1"),
        refresh: Some(refresh("same")),
    }));

    let token = resume(&unchanged, &source).expect("resume succeeds");

    assert_eq!(token.expect("resumed token").expose(), "access-1");
    assert_eq!(unchanged.save_count(), 0);

    let rotated = FakeStore::seeded(refresh("old"));
    let source = ScriptedSource::new().on_refresh(Ok(TokenSet {
        access: access("access-2"),
        refresh: Some(refresh("new")),
    }));

    let token = resume(&rotated, &source).expect("resume succeeds");

    assert_eq!(token.expect("resumed token").expose(), "access-2");
    assert_eq!(rotated.save_count(), 1);
    assert_eq!(
        rotated.load().expect("load").expect("token").expose(),
        "new"
    );
}

#[test]
fn resume_save_failure_is_an_error() {
    let store = FakeStore::seeded(refresh("old"));
    store.fail_save();
    let source = ScriptedSource::new().on_refresh(Ok(TokenSet {
        access: access("access-1"),
        refresh: Some(refresh("new")),
    }));

    let error = resume(&store, &source).expect_err("resume fails");

    assert!(matches!(error, AuthError::Store(_)));
}

#[test]
fn resume_load_failure_is_an_error() {
    let store = FakeStore::new();
    store.fail_load();
    let source = ScriptedSource::new();

    let error = resume(&store, &source).expect_err("resume fails");

    assert!(matches!(error, AuthError::Store(_)));
    assert_eq!(source.refresh_calls(), 0);
}

#[test]
fn authorize_saves_even_if_the_token_equals_the_stored_one() {
    let store = FakeStore::seeded(refresh("same"));
    let source = ScriptedSource::new().on_interactive(Ok(TokenSet {
        access: access("access-1"),
        refresh: Some(refresh("same")),
    }));

    let token = authorize(&store, &source).expect("authorize succeeds");

    assert_eq!(token.expose(), "access-1");
    assert_eq!(store.save_count(), 1);
    assert_eq!(store.load().expect("load").expect("token").expose(), "same");
}

#[test]
fn authorize_without_refresh_token_saves_nothing() {
    let store = FakeStore::seeded(refresh("old"));
    let source = ScriptedSource::new().on_interactive(Ok(TokenSet {
        access: access("access-1"),
        refresh: None,
    }));

    let error = authorize(&store, &source).expect_err("authorize fails");

    assert!(matches!(error, AuthError::MissingRefreshToken));
    assert_eq!(store.save_count(), 0);
    assert_eq!(store.load().expect("load").expect("token").expose(), "old");
}

#[test]
fn authorize_overwrites_stale_token() {
    let store = FakeStore::seeded(refresh("old"));
    let source = ScriptedSource::new().on_interactive(Ok(TokenSet {
        access: access("access-1"),
        refresh: Some(refresh("new")),
    }));

    let token = authorize(&store, &source).expect("authorize succeeds");

    assert_eq!(token.expose(), "access-1");
    assert_eq!(store.save_count(), 1);
    assert_eq!(store.load().expect("load").expect("token").expose(), "new");
}

#[test]
fn resume_then_authorize_matches_the_old_fallback() {
    let store = FakeStore::seeded(refresh("old"));
    let source = ScriptedSource::new()
        .on_refresh(Err(other_error()))
        .on_interactive(Ok(TokenSet {
            access: access("access-4"),
            refresh: Some(refresh("new")),
        }));

    let resumed = resume(&store, &source).expect("resume succeeds");
    assert!(resumed.is_none());
    let access = authorize(&store, &source).expect("authorize succeeds");

    assert_eq!(access.expose(), "access-4");
    assert_eq!(source.refresh_calls(), 1);
    assert_eq!(source.interactive_calls(), 1);
    assert_eq!(store.load().expect("load").expect("token").expose(), "new");
}
