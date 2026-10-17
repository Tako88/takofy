//! librespot-backed rootlist source and playlist labelling.
//!
//! The only file in this crate that performs librespot I/O.

use futures_util::StreamExt;
use librespot::core::{Session, SpotifyUri};
use librespot::metadata::{Metadata, Playlist, Track};

use crate::context::{ContextError, Page, PageSource, page_from_proto, parse_page_json};
use crate::model::{PlaylistEntry, TrackEntry};
use crate::rootlist::{RootlistError, RootlistPage, RootlistSource, RootlistWire, parse_rootlist};

/// The wire the live rootlist is read as; flip to `Json` if the binary parse fails.
pub(crate) const ROOTLIST_WIRE: RootlistWire = RootlistWire::Binary;

/// Reads rootlist pages through librespot's spclient.
pub(crate) struct RemoteRootlist<'a> {
    pub(crate) session: &'a Session,
    pub(crate) wire: RootlistWire,
}

impl RootlistSource for RemoteRootlist<'_> {
    async fn page(&self, from: usize, len: usize) -> Result<RootlistPage, RootlistError> {
        let bytes = match self.wire {
            RootlistWire::Binary => self.session.spclient().get_rootlist(from, Some(len)).await,
            RootlistWire::Json => {
                let endpoint = rootlist_endpoint(self.session, from, len);
                // `hyper::Method` is not re-exported by librespot, so infer it here.
                let method = "GET"
                    .parse()
                    .map_err(|_| RootlistError::Source("invalid HTTP method".to_owned()))?;
                self.session
                    .spclient()
                    .request_as_json(&method, &endpoint, None, None)
                    .await
            }
        }
        .map_err(|error| RootlistError::Source(error.to_string()))?;
        parse_rootlist(&bytes, self.wire)
    }
}

/// The same endpoint `get_rootlist` builds; `request_as_json` needs it inline.
fn rootlist_endpoint(session: &Session, from: usize, len: usize) -> String {
    let user = session.username();
    format!(
        "/playlist/v2/user/{user}/rootlist?decorate=revision,attributes,length,owner,capabilities,status_code&from={from}&length={len}"
    )
}

/// Names each playlist, falling back to its URI when metadata is missing.
pub(crate) async fn label_playlists(session: &Session, uris: Vec<String>) -> Vec<PlaylistEntry> {
    futures_util::stream::iter(uris)
        .map(|uri| async move {
            let name = playlist_name(session, &uri).await;
            PlaylistEntry { uri, name }
        })
        .buffered(8)
        .collect::<Vec<_>>()
        .await
}

async fn playlist_name(session: &Session, uri: &str) -> String {
    let Ok(id) = SpotifyUri::from_uri(uri) else {
        return uri.to_owned();
    };
    match Playlist::get(session, &id).await {
        Ok(playlist) => playlist.name().to_owned(),
        Err(_) => uri.to_owned(),
    }
}

/// Reads context pages through librespot's spclient.
pub(crate) struct RemoteContext<'a> {
    pub(crate) session: &'a Session,
}

impl PageSource for RemoteContext<'_> {
    async fn resolve(&self, uri: &str) -> Result<Vec<Page>, ContextError> {
        let context = self
            .session
            .spclient()
            .get_context(uri)
            .await
            .map_err(|error| ContextError::Source(error.to_string()))?;
        Ok(context.pages.iter().map(page_from_proto).collect())
    }

    async fn next(&self, url: &str) -> Result<Page, ContextError> {
        let bytes = self
            .session
            .spclient()
            .get_next_page(url)
            .await
            .map_err(|error| ContextError::Source(error.to_string()))?;
        parse_page_json(&bytes)
    }
}

/// Names each track, falling back to its URI when metadata is missing.
pub(crate) async fn label_tracks(session: &Session, uris: Vec<String>) -> Vec<TrackEntry> {
    futures_util::stream::iter(uris)
        .map(|uri| async move { label_track(session, uri).await })
        .buffered(8)
        .collect::<Vec<_>>()
        .await
}

async fn label_track(session: &Session, uri: String) -> TrackEntry {
    let Ok(id) = SpotifyUri::from_uri(&uri) else {
        return TrackEntry {
            title: uri.clone(),
            artists: String::new(),
            uri,
        };
    };
    match Track::get(session, &id).await {
        Ok(track) => TrackEntry {
            title: track.name.clone(),
            artists: track
                .artists
                .iter()
                .map(|artist| artist.name.clone())
                .collect::<Vec<_>>()
                .join(", "),
            uri,
        },
        Err(_) => TrackEntry {
            title: uri.clone(),
            artists: String::new(),
            uri,
        },
    }
}
