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
mod tests;
