use super::super::signin::{authorize_update, failure_of};
use super::super::*;
use super::test_support::*;

fn auth_source(kind: SourceErrorKind) -> AuthError {
    AuthError::Source(SourceError { kind })
}

#[test]
fn failure_of_maps_each_error() {
    assert_eq!(
        failure_of(&AuthError::Store(StoreError::Unavailable)),
        Failure::Keychain
    );
    assert_eq!(
        failure_of(&AuthError::MissingRefreshToken),
        Failure::NoRefreshToken
    );
    assert_eq!(
        failure_of(&auth_source(SourceErrorKind::PortBusy)),
        Failure::PortBusy
    );
    assert_eq!(
        failure_of(&auth_source(SourceErrorKind::Other)),
        Failure::Other
    );
}

#[test]
fn resume_update_maps_the_outcomes() {
    assert_eq!(resume_update(Ok(Some(access("a")))), Update::SignedIn);
    assert_eq!(resume_update(Ok(None)), Update::NeedsAuthorization);
    assert_eq!(
        resume_update(Err(AuthError::Store(StoreError::Unavailable))),
        Update::SignInFailed(Failure::Keychain)
    );
    assert_eq!(
        resume_update(Err(auth_source(SourceErrorKind::PortBusy))),
        Update::SignInFailed(Failure::PortBusy)
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_store_that_cannot_be_read_reaches_the_window_as_actionable_text() {
    let store = MemoryStore::unavailable();
    let source = FixedSource::default();

    let error = resume(&store, &source).expect_err("the store load fails");

    assert_eq!(failure_of(&error), Failure::Keychain);
    let message = failure_message(Failure::Keychain);
    assert!(
        message.contains("org.freedesktop.secrets"),
        "the window text must name the D-Bus service: {message}"
    );
    for desktop in ["GNOME", "Plasma", "Hyprland", "i3", "sway"] {
        assert!(
            message.contains(desktop),
            "{desktop} must be distinguished: {message}"
        );
    }
}

#[test]
fn a_corrupt_entry_reaches_the_window_as_actionable_text() {
    let failure = failure_of(&AuthError::Store(StoreError::Corrupt));

    assert_eq!(failure, Failure::Keychain);
    assert!(
        failure_message(failure).contains("signing in again"),
        "a corrupt entry must offer a way out"
    );
}

#[test]
fn authorize_update_maps_the_outcomes() {
    assert_eq!(authorize_update(Ok(access("a"))), Update::SignedIn);
    assert_eq!(
        authorize_update(Err(AuthError::MissingRefreshToken)),
        Update::SignInFailed(Failure::NoRefreshToken)
    );
    assert_eq!(
        authorize_update(Err(auth_source(SourceErrorKind::Other))),
        Update::SignInFailed(Failure::Other)
    );
}
