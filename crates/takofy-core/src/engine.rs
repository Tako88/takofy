//! Session facade for the rootlist.

use librespot::core::authentication::Credentials;
use librespot::core::{Session, SessionConfig};

use crate::album::AlbumError;
use crate::artist::ArtistError;
use crate::context::{ContextError, Page, PageSource, Walker};
use crate::credentials::AccessToken;
use crate::followed::FollowedError;
use crate::lyrics::LyricsError;
use crate::playback::{Playback, PlaybackError, PlaybackEvent};
use crate::playlist_edit::{self, PlaylistError};
use crate::remote::{
    ROOTLIST_WIRE, RemoteContext, RemoteRootlist, label_playlists, label_tracks, read_album,
    read_artist, read_followed_artists, read_followed_shows, read_liked_tracks, read_lyrics,
    read_playlist_state, read_related, read_release_tracks, read_saved_albums, read_show,
    write_add, write_create, write_follow, write_like, write_move, write_remove,
    write_rootlist_add, write_rootlist_move,
};
use crate::rootlist::{RootlistError, fetch_all};
use crate::search::search_uri;
use crate::show::ShowError;
use takofy_model::{
    AlbumDetail, ArtistDetail, CoverUrl, FollowedArtist, FollowedShow, Move, PlaylistAccess,
    PlaylistEntry, Removal, Revision, SavedAlbum, ShowDetail, TrackEntry, TrackLyrics,
};

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
    #[error(transparent)]
    Show(#[from] ShowError),
    #[error(transparent)]
    Followed(#[from] FollowedError),
    #[error(transparent)]
    Lyrics(#[from] LyricsError),
    #[error(transparent)]
    Playlist(#[from] PlaylistError),
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

    /// The user's playlists, labelled, with what the column may write.
    pub async fn rootlist(&self) -> Result<RootlistRead, EngineError> {
        let source = RemoteRootlist {
            session: &self.session,
            wire: ROOTLIST_WIRE,
        };
        let walked = fetch_all(&source).await?;
        Ok(RootlistRead {
            entries: label_playlists(&self.session, walked.playlists).await,
            access: walked.access,
        })
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

    /// The first page of a playlist's tracks, with what the page may change.
    ///
    /// The rows and the list's own revision are read together: one walk of the
    /// context's first page, and one request for the state a write must name. A
    /// state that cannot be read leaves the page with no access to offer.
    pub async fn playlist_page(&self, playlist_uri: &str) -> Result<PlaylistPage, EngineError> {
        let mut pages = self.tracks(playlist_uri);
        let (page, state) = tokio::join!(
            pages.next(),
            read_playlist_state(&self.session, playlist_uri)
        );
        let tracks = page?.unwrap_or_default();
        let shown = tracks
            .iter()
            .map(|track| track.uri.clone())
            .collect::<Vec<_>>();
        let access = state.and_then(|state| {
            playlist_edit::access(
                Some(state.revision),
                state.can_edit,
                state.owned,
                &state.items,
                &shown,
            )
        });
        Ok(PlaylistPage {
            tracks,
            cover: pages.cover(),
            access,
        })
    }

    /// Writes one move to a playlist, answering the revision to hold next.
    pub async fn move_track(
        &self,
        playlist_uri: &str,
        revision: &Revision,
        moves: &[Move],
    ) -> Result<Revision, EngineError> {
        Ok(write_move(&self.session, playlist_uri, revision, moves).await?)
    }

    /// Removes rows from a playlist, answering the revision to hold next.
    pub async fn remove_tracks(
        &self,
        playlist_uri: &str,
        revision: &Revision,
        removals: &[Removal],
    ) -> Result<Revision, EngineError> {
        Ok(write_remove(&self.session, playlist_uri, revision, removals).await?)
    }

    /// Writes one move to the user's rootlist, answering the revision to hold
    /// next.
    pub async fn move_playlist(
        &self,
        revision: &Revision,
        from: usize,
        to: usize,
    ) -> Result<Revision, EngineError> {
        Ok(write_rootlist_move(&self.session, revision, from, to).await?)
    }

    /// Creates a playlist called `name`, answering its URI.
    ///
    /// The playlist is not on the rootlist until [`Self::link_playlist`] puts it
    /// there, and nothing else can find it before that.
    pub async fn make_playlist(&self, name: &str) -> Result<String, EngineError> {
        Ok(write_create(&self.session, name).await?)
    }

    /// Puts `playlist_uri` at the front of the user's rootlist, answering the
    /// revision to hold next.
    pub async fn link_playlist(
        &self,
        revision: &Revision,
        playlist_uri: &str,
    ) -> Result<Revision, EngineError> {
        Ok(write_rootlist_add(&self.session, revision, playlist_uri).await?)
    }

    /// What a playlist read says about writing to it, when the read answers.
    ///
    /// Best effort: a list that cannot be read yet is one the sidebar may not
    /// write to, which is the fail-safe the caller draws.
    pub async fn playlist_state(&self, playlist_uri: &str) -> Option<PlaylistAccess> {
        let state = read_playlist_state(&self.session, playlist_uri).await?;
        playlist_edit::listing_access(state.revision.as_bytes(), state.can_edit)
    }

    /// Appends tracks to a playlist, answering the revision to hold next.
    pub async fn add_tracks(
        &self,
        playlist_uri: &str,
        revision: &Revision,
        from_index: usize,
        track_uris: &[String],
    ) -> Result<Revision, EngineError> {
        Ok(write_add(
            &self.session,
            playlist_uri,
            revision,
            from_index,
            track_uris,
        )
        .await?)
    }

    /// The tracks one search query answers, labelled, in the wire's own order.
    ///
    /// One page only: the search route answers twenty tracks and offers no
    /// paging, so this never walks. A blank query asks for nothing and answers
    /// nothing without a request.
    pub async fn search(&self, query: &str) -> Result<Vec<TrackEntry>, EngineError> {
        let source = RemoteContext {
            session: &self.session,
        };
        let Some(page) = search_page(source, query).await? else {
            return Ok(Vec::new());
        };
        Ok(label_tracks(&self.session, page.tracks).await)
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

    /// The related artists an artist page shows as cards, in the order given.
    ///
    /// A URI whose artist cannot be read keeps its URI and stays in the row.
    pub async fn related(&self, uris: Vec<String>) -> Result<Vec<FollowedArtist>, EngineError> {
        Ok(read_related(&self.session, uris).await?)
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

    /// One show: its metadata and every episode, labelled in the show's order.
    ///
    /// A URI that does not name a show fails before any request is made.
    pub async fn show(&self, show_uri: &str) -> Result<ShowDetail, EngineError> {
        Ok(read_show(&self.session, show_uri).await?)
    }

    /// One track's lyrics, empty when the track has none.
    ///
    /// A track without lyrics is an empty success rather than an error: the
    /// endpoint's not-found is the answer, not a failure. A URI that does not
    /// name a track fails before any request is made.
    pub async fn lyrics(&self, track_uri: &str) -> Result<TrackLyrics, EngineError> {
        Ok(read_lyrics(&self.session, track_uri).await?)
    }

    /// The shows the user follows, newest follow first, each as its card draws it.
    pub async fn followed_shows(&self) -> Result<Vec<FollowedShow>, EngineError> {
        Ok(read_followed_shows(&self.session).await?)
    }

    /// The artists the user follows, newest follow first, each as its card draws it.
    pub async fn followed_artists(&self) -> Result<Vec<FollowedArtist>, EngineError> {
        Ok(read_followed_artists(&self.session).await?)
    }

    /// The albums the user saves, newest save first, each as its card draws it.
    pub async fn saved_albums(&self) -> Result<Vec<SavedAlbum>, EngineError> {
        Ok(read_saved_albums(&self.session).await?)
    }

    /// The tracks the user has liked, newest save first.
    pub async fn liked_tracks(&self) -> Result<Vec<String>, EngineError> {
        Ok(read_liked_tracks(&self.session).await?)
    }

    /// Writes one pick into the liked-songs set, or out of it.
    pub async fn set_liked(&self, tracks: Vec<String>, liked: bool) -> Result<(), EngineError> {
        Ok(write_like(&self.session, tracks, liked).await?)
    }
}

/// The rootlist: its entries, and what the column may write with.
///
/// `access` is `None` when the read named no revision, and its `editable` is
/// false whenever the read dropped an item, so a row index stops being the
/// wire's index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RootlistRead {
    pub entries: Vec<PlaylistEntry>,
    pub access: Option<PlaylistAccess>,
}

/// One playlist page: its rows, the context's own image, and what it may change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlaylistPage {
    pub tracks: Vec<TrackEntry>,
    pub cover: Option<CoverUrl>,
    pub access: Option<PlaylistAccess>,
}

/// The first page one search query answers, through any page source.
///
/// Split out of [`Engine::search`] so the URI the walker resolves can be
/// asserted without a session. A blank query resolves nothing, and a search
/// answers at most one page.
async fn search_page<S: PageSource>(source: S, query: &str) -> Result<Option<Page>, ContextError> {
    let Some(uri) = search_uri(query) else {
        return Ok(None);
    };
    Walker::new(source, uri).next_page().await
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::search_page;
    use crate::context::{ContextError, Page, PageSource, PageTrack, Resolved};

    /// A source that records every URI it is asked to resolve and answers one
    /// page with one track.
    #[derive(Clone, Default)]
    struct Recording {
        resolved: Arc<Mutex<Vec<String>>>,
    }

    impl Recording {
        fn resolved(&self) -> Vec<String> {
            self.resolved.lock().expect("lock").clone()
        }
    }

    impl PageSource for Recording {
        async fn resolve(&self, uri: &str) -> Result<Resolved, ContextError> {
            self.resolved.lock().expect("lock").push(uri.to_owned());
            Ok(Resolved {
                pages: vec![Page {
                    tracks: vec![PageTrack {
                        uri: "spotify:track:a".to_owned(),
                        added_at: None,
                    }],
                    next: None,
                }],
                cover: None,
            })
        }

        async fn next(&self, url: &str) -> Result<Page, ContextError> {
            panic!("a search resolves once and never pages (asked for {url})");
        }
    }

    #[tokio::test]
    async fn a_search_walks_a_search_context_uri() {
        let source = Recording::default();
        let page = search_page(source.clone(), "never gonna")
            .await
            .expect("walk")
            .expect("one page");

        assert_eq!(
            source.resolved(),
            vec!["spotify:search:never+gonna".to_owned()],
            "the walker must be given a context URI, not a bare query"
        );
        assert_eq!(
            page,
            Page {
                tracks: vec![PageTrack {
                    uri: "spotify:track:a".to_owned(),
                    added_at: None,
                }],
                next: None,
            }
        );
    }

    #[tokio::test]
    async fn a_blank_query_resolves_nothing() {
        let source = Recording::default();
        assert_eq!(
            search_page(source.clone(), "  \t ").await.expect("walk"),
            None
        );
        assert_eq!(source.resolved(), Vec::<String>::new());
    }
}
