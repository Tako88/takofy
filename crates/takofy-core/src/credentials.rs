//! Credential port.

use std::fmt;

/// OAuth refresh token. Redacted on `Debug`; deliberately no `Display`.
#[derive(Clone)]
pub struct RefreshToken(String);

impl RefreshToken {
    /// `None` for empty or whitespace-only input.
    ///
    /// A non-empty value is stored verbatim, whitespace included: a secret is
    /// never silently mangled, so a padded token fails at refresh instead of
    /// being quietly altered here. Adapters mapping a keychain entry should
    /// surface such a value as [`StoreError::Corrupt`] rather than trimming it.
    pub fn new(value: String) -> Option<Self> {
        if value.trim().is_empty() {
            None
        } else {
            Some(Self(value))
        }
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for RefreshToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RefreshToken(<redacted>)")
    }
}

/// Spotify access token. Redacted on `Debug`; deliberately no `Display`.
#[derive(Clone)]
pub struct AccessToken(String);

impl AccessToken {
    /// `None` for empty or whitespace-only input.
    pub fn new(value: String) -> Option<Self> {
        if value.trim().is_empty() {
            None
        } else {
            Some(Self(value))
        }
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccessToken(<redacted>)")
    }
}

/// Failure of a [`CredentialStore`] operation.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("credential store unavailable")]
    Unavailable,
    #[error("stored credential is corrupt")]
    Corrupt,
    #[error("{0}")]
    Other(&'static str),
}

/// Persistence port for the refresh token.
pub trait CredentialStore: Send + Sync {
    fn load(&self) -> Result<Option<RefreshToken>, StoreError>;
    fn save(&self, token: &RefreshToken) -> Result<(), StoreError>;
    fn clear(&self) -> Result<(), StoreError>;
}

#[cfg(test)]
pub(crate) mod fake {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::{CredentialStore, RefreshToken, StoreError};

    #[derive(Default)]
    pub(crate) struct FakeStore {
        token: Mutex<Option<RefreshToken>>,
        fail_load: AtomicBool,
        fail_save: AtomicBool,
        saves: AtomicUsize,
    }

    impl FakeStore {
        pub(crate) fn new() -> Self {
            Self::default()
        }

        /// Seeds a token without going through [`CredentialStore::save`], so
        /// resume-path tests start with a credential and a zero save count.
        pub(crate) fn seeded(token: RefreshToken) -> Self {
            Self {
                token: Mutex::new(Some(token)),
                ..Self::default()
            }
        }

        pub(crate) fn fail_load(&self) {
            self.fail_load.store(true, Ordering::SeqCst);
        }

        pub(crate) fn fail_save(&self) {
            self.fail_save.store(true, Ordering::SeqCst);
        }

        pub(crate) fn save_count(&self) -> usize {
            self.saves.load(Ordering::SeqCst)
        }
    }

    impl CredentialStore for FakeStore {
        fn load(&self) -> Result<Option<RefreshToken>, StoreError> {
            if self.fail_load.load(Ordering::SeqCst) {
                return Err(StoreError::Unavailable);
            }
            let guard = self
                .token
                .lock()
                .map_err(|_| StoreError::Other("lock poisoned"))?;
            Ok(guard.clone())
        }

        fn save(&self, token: &RefreshToken) -> Result<(), StoreError> {
            if self.fail_save.load(Ordering::SeqCst) {
                return Err(StoreError::Unavailable);
            }
            let mut guard = self
                .token
                .lock()
                .map_err(|_| StoreError::Other("lock poisoned"))?;
            *guard = Some(token.clone());
            self.saves.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn clear(&self) -> Result<(), StoreError> {
            let mut guard = self
                .token
                .lock()
                .map_err(|_| StoreError::Other("lock poisoned"))?;
            *guard = None;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeStore;
    use super::{AccessToken, CredentialStore, RefreshToken, StoreError};

    fn token(value: &str) -> RefreshToken {
        RefreshToken::new(value.to_owned()).expect("non-empty token")
    }

    #[test]
    fn debug_hides_the_secret() {
        let refresh = token("super-secret-refresh");
        assert_eq!(format!("{refresh:?}"), "RefreshToken(<redacted>)");

        let access = AccessToken::new("super-secret-access".to_owned()).expect("non-empty");
        assert_eq!(format!("{access:?}"), "AccessToken(<redacted>)");
    }

    #[test]
    fn empty_refresh_token_is_rejected() {
        assert!(RefreshToken::new(String::new()).is_none());
        assert!(RefreshToken::new("  \n\t ".to_owned()).is_none());
    }

    #[test]
    fn load_on_empty_is_none() {
        let store = FakeStore::new();
        assert!(store.load().expect("load succeeds").is_none());
    }

    #[test]
    fn save_then_load_round_trips() {
        let store = FakeStore::new();
        store.save(&token("abc")).expect("save succeeds");
        let loaded = store.load().expect("load succeeds").expect("token present");
        assert_eq!(loaded.expose(), "abc");
        assert_eq!(store.save_count(), 1);
    }

    #[test]
    fn seeded_token_loads_without_counting_a_save() {
        let store = FakeStore::seeded(token("seeded"));
        let loaded = store.load().expect("load succeeds").expect("token present");
        assert_eq!(loaded.expose(), "seeded");
        assert_eq!(store.save_count(), 0);
    }

    #[test]
    fn save_overwrites() {
        let store = FakeStore::new();
        store.save(&token("first")).expect("save succeeds");
        store.save(&token("second")).expect("save succeeds");
        let loaded = store.load().expect("load succeeds").expect("token present");
        assert_eq!(loaded.expose(), "second");
    }

    #[test]
    fn clear_is_idempotent() {
        let store = FakeStore::new();
        store.save(&token("abc")).expect("save succeeds");
        store.clear().expect("clear succeeds");
        assert!(store.load().expect("load succeeds").is_none());
        store.clear().expect("second clear succeeds");
        assert!(store.load().expect("load succeeds").is_none());
    }

    #[test]
    fn failing_store_surfaces_its_error() {
        let store = FakeStore::new();
        store.fail_load();
        assert!(matches!(store.load(), Err(StoreError::Unavailable)));

        let store = FakeStore::new();
        store.fail_save();
        assert!(matches!(
            store.save(&token("abc")),
            Err(StoreError::Unavailable)
        ));
    }
}
