//! OS keychain adapter.

use keyring::Entry;
use takofy_core::credentials::{CredentialStore, RefreshToken, StoreError};

const SERVICE: &str = "takofy";
const USER: &str = "spotify-refresh-token";

/// Stores the refresh token in the OS keychain.
///
/// `keyring::Entry` has undeclared thread affinity, so one is constructed per
/// call and the store holds nothing.
pub struct KeychainStore;

impl KeychainStore {
    fn entry(&self) -> Result<Entry, StoreError> {
        Entry::new(SERVICE, USER).map_err(|_| StoreError::Unavailable)
    }
}

impl CredentialStore for KeychainStore {
    fn load(&self) -> Result<Option<RefreshToken>, StoreError> {
        match self.entry()?.get_password() {
            Ok(secret) => match RefreshToken::new(secret) {
                Some(token) => Ok(Some(token)),
                None => Err(StoreError::Corrupt),
            },
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(map_error(&error)),
        }
    }

    fn save(&self, token: &RefreshToken) -> Result<(), StoreError> {
        self.entry()?
            .set_password(token.expose())
            .map_err(|error| map_error(&error))
    }

    fn clear(&self) -> Result<(), StoreError> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(map_error(&error)),
        }
    }
}

fn map_error(error: &keyring::Error) -> StoreError {
    match error {
        keyring::Error::BadEncoding(_) => StoreError::Corrupt,
        keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_) => {
            StoreError::Unavailable
        }
        _ => StoreError::Other("keychain error"),
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::io;

    use super::map_error;
    use takofy_core::credentials::StoreError;

    fn platform(message: &str) -> Box<dyn Error + Send + Sync> {
        Box::new(io::Error::other(message))
    }

    #[test]
    fn bad_encoding_is_corrupt() {
        assert!(matches!(
            map_error(&keyring::Error::BadEncoding(vec![0xff])),
            StoreError::Corrupt
        ));
    }

    #[test]
    fn no_storage_access_is_unavailable() {
        assert!(matches!(
            map_error(&keyring::Error::NoStorageAccess(platform("locked"))),
            StoreError::Unavailable
        ));
    }

    #[test]
    fn platform_failure_is_unavailable() {
        assert!(matches!(
            map_error(&keyring::Error::PlatformFailure(platform("broken"))),
            StoreError::Unavailable
        ));
    }

    #[test]
    fn no_entry_is_other() {
        assert!(matches!(
            map_error(&keyring::Error::NoEntry),
            StoreError::Other(_)
        ));
    }

    #[test]
    fn no_default_store_is_other() {
        assert!(matches!(
            map_error(&keyring::Error::NoDefaultStore),
            StoreError::Other(_)
        ));
    }

    #[test]
    fn ambiguous_is_other() {
        assert!(matches!(
            map_error(&keyring::Error::Ambiguous(Vec::new())),
            StoreError::Other(_)
        ));
    }

    #[test]
    fn bad_data_format_is_other() {
        assert!(matches!(
            map_error(&keyring::Error::BadDataFormat(vec![0xff], platform("bad"))),
            StoreError::Other(_)
        ));
    }

    #[test]
    fn bad_store_format_is_other() {
        assert!(matches!(
            map_error(&keyring::Error::BadStoreFormat("bad".to_owned())),
            StoreError::Other(_)
        ));
    }

    #[test]
    fn too_long_is_other() {
        assert!(matches!(
            map_error(&keyring::Error::TooLong("user".to_owned(), 10)),
            StoreError::Other(_)
        ));
    }

    #[test]
    fn invalid_is_other() {
        assert!(matches!(
            map_error(&keyring::Error::Invalid(
                "user".to_owned(),
                "bad".to_owned()
            )),
            StoreError::Other(_)
        ));
    }

    #[test]
    fn not_supported_by_store_is_other() {
        assert!(matches!(
            map_error(&keyring::Error::NotSupportedByStore("nope".to_owned())),
            StoreError::Other(_)
        ));
    }
}
