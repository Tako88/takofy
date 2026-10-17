//! Session facade for the rootlist.

use librespot::core::authentication::Credentials;
use librespot::core::{Session, SessionConfig};

use crate::credentials::AccessToken;
use crate::model::PlaylistEntry;
use crate::remote::{ROOTLIST_WIRE, RemoteRootlist, label_playlists};
use crate::rootlist::{RootlistError, fetch_all};

/// Why an [`Engine`] operation failed.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("session error: {0}")]
    Session(#[from] librespot::core::Error),
    #[error(transparent)]
    Rootlist(#[from] RootlistError),
}

/// A connected Spotify session.
pub struct Engine {
    session: Session,
}

impl Engine {
    /// Connects with an already-issued access token.
    pub async fn connect(cfg: SessionConfig, token: &AccessToken) -> Result<Self, EngineError> {
        let session = Session::new(cfg, None);
        session
            .connect(Credentials::with_access_token(token.expose()), false)
            .await?;
        Ok(Self { session })
    }

    /// The user's playlists, labelled.
    pub async fn rootlist(&self) -> Result<Vec<PlaylistEntry>, EngineError> {
        let source = RemoteRootlist {
            session: &self.session,
            wire: ROOTLIST_WIRE,
        };
        let uris = fetch_all(&source).await?;
        Ok(label_playlists(&self.session, uris).await)
    }
}
