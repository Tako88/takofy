//! Session facade for the rootlist.

use librespot::core::authentication::Credentials;
use librespot::core::{Session, SessionConfig};

use crate::album::AlbumError;
use crate::artist::ArtistError;
use crate::context::{ContextError, Walker};
use crate::credentials::AccessToken;
use crate::playback::{Playback, PlaybackError, PlaybackEvent};
use crate::remote::{
    ROOTLIST_WIRE, RemoteContext, RemoteRootlist, label_playlists, label_tracks, read_album,
    read_artist, read_release_tracks, write_follow,
};
use crate::rootlist::{RootlistError, fetch_all};
use takofy_model::{AlbumDetail, ArtistDetail, CoverUrl, PlaylistEntry, TrackEntry};

/// Why an [`Engine`] operation failed.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("session error: {0}")]
    Session(#[from] librespot::core::Error),
    #[error(transparent)]
    Rootlist(#[from] RootlistError),
    #[error(transparent)]
    Context(#[from] ContextError),
    #[error(transparent)]
    Album(#[from] AlbumError),
    #[error(transparent)]
    Artist(#[from] ArtistError),
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

    /// Starts a player over this session.
    ///
    /// Needs an active Tokio runtime: the event forwarder is spawned on it.
    pub fn playback(
        &self,
    ) -> Result<
        (
            Playback,
            tokio::sync::mpsc::UnboundedReceiver<PlaybackEvent>,
        ),
        PlaybackError,
    > {
        Playback::start(self.session.clone())
    }

    /// A paged, labelled view of a playlist's tracks.
    pub fn tracks(&self, playlist_uri: &str) -> TrackPages<'_> {
        TrackPages {
            session: &self.session,
            walker: Walker::new(
                RemoteContext {
                    session: &self.session,
                },
                playlist_uri,
            ),
        }
    }

    /// One album, its tracks labelled.
    ///
    /// Unlike a playlist this is a single request and a single labelling pass:
    /// an album's order comes from the album, so there is no context to walk.
    pub async fn album(&self, album_uri: &str) -> Result<AlbumDetail, EngineError> {
        Ok(read_album(&self.session, album_uri).await?)
    }

    /// One artist: its page data plus whether the user follows it.
    pub async fn artist(
        &self,
        artist_uri: &str,
    ) -> Result<(ArtistDetail, Option<bool>), EngineError> {
        Ok(read_artist(&self.session, artist_uri).await?)
    }

    /// One release's track names, in the release's own order.
    ///
    /// Cannot fail: a track that cannot be labelled falls back to its URI.
    pub async fn release_tracks(&self, tracks: &[String]) -> Vec<TrackEntry> {
        read_release_tracks(&self.session, tracks).await
    }

    /// Writes the artist into the user's followed set, or out of it.
    pub async fn set_follow(&self, artist_uri: &str, follow: bool) -> Result<(), EngineError> {
        Ok(write_follow(&self.session, artist_uri, follow).await?)
    }
}

/// The track pages of one playlist.
pub struct TrackPages<'a> {
    session: &'a Session,
    walker: Walker<RemoteContext<'a>>,
}

impl TrackPages<'_> {
    /// The next page of labelled tracks, or `None` at the end.
    pub async fn next(&mut self) -> Result<Option<Vec<TrackEntry>>, EngineError> {
        let Some(page) = self.walker.next_page().await? else {
            return Ok(None);
        };
        Ok(Some(label_tracks(self.session, page.tracks).await))
    }

    /// The context's own image, once the first page has been resolved.
    pub fn cover(&self) -> Option<CoverUrl> {
        self.walker.cover()
    }
}
