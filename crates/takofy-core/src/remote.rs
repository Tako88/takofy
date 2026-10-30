//! librespot-backed rootlist source and playlist labelling.
//!
//! The only file in this crate that performs librespot I/O.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use futures_util::StreamExt;
use http::header::{ACCEPT, CONTENT_TYPE};
use http::{HeaderMap, Method};
use librespot::core::date::Date;
use librespot::core::spclient::{RequestStrategy, SpClient};
use librespot::core::{Session, SpotifyId, SpotifyUri};
use librespot::metadata::{Album, Artist, Lyrics, Metadata, Playlist, Show};
use librespot::protocol::extended_metadata::{
    BatchedEntityRequest, BatchedExtensionResponse, EntityRequest, ExtensionQuery,
};
use librespot::protocol::extension_kind::ExtensionKind;
use librespot::protocol::metadata::{
    Date as DateMessage, Episode as EpisodeMessage, Track as TrackMessage,
};
use protobuf::Message as _;
use takofy_model::{
    AlbumDetail, ArtistDetail, ArtistPortrait, CoverSet, CoverUrl, Episode, FollowedArtist,
    FollowedShow, LikedTrack, Move, PlaylistAccess, PlaylistCover, PlaylistEntry, PublishedAt,
    Removal, Revision, SavedAlbum, ShowDetail, TrackEntry, TrackLyrics, composed_from,
};

use crate::album::{AlbumError, detail, positions};
use crate::artist::{
    ArtistError, detail as artist_detail, related_uris, release_chunks, release_uris, releases_of,
    top_track_positions,
};
use crate::collected::{self, ArtistCard};
use crate::collection::{self, Item};
use crate::context::{
    ContextError, Page, PageSource, PageTrack, Resolved, page_from_proto, parse_page_json,
};
use crate::cover::{covers_in_group, covers_in_images};
use crate::followed::{self, FollowedError, ShowCard};
use crate::label_cache::UriCache;
use crate::lyrics::{self, LyricsError};
use crate::playlist_edit;
use crate::portrait::{portrait_from, portraits_of};
use crate::rootlist::{
    RootEntry, RootlistError, RootlistPage, RootlistSource, RootlistWire, parse_rootlist,
    rootlist_path,
};
use crate::show::{
    ShowError, detail as show_detail, details as show_details, episode_uris, expand_episodes,
};
use crate::status_tally::StatusTally;

/// The wire the live rootlist is read as; flip to `Json` if the binary parse fails.
pub(crate) const ROOTLIST_WIRE: RootlistWire = RootlistWire::Binary;

/// How many metadata reads one pass keeps in flight.
const REMOTE_FETCH_WIDTH: usize = 8;

/// How many shows one pooled episode read fetches at a time.
///
/// The shows are fetched in their own order, so this bounds the requests without
/// reordering the pool.
const SHOW_FANOUT: usize = 4;

/// Reads rootlist pages through librespot's spclient.
pub(crate) struct RemoteRootlist<'a> {
    pub(crate) session: &'a Session,
    pub(crate) wire: RootlistWire,
}

impl RootlistSource for RemoteRootlist<'_> {
    async fn page(&self, from: usize, len: usize) -> Result<RootlistPage, RootlistError> {
        let endpoint = rootlist_path(&self.session.username(), from, len);
        let bytes = match self.wire {
            RootlistWire::Binary => {
                self.session
                    .spclient()
                    .request(&Method::GET, &endpoint, None, None)
                    .await
            }
            RootlistWire::Json => {
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

/// Names each playlist, falling back to its URI when metadata is missing.
///
/// A playlist's cover is, in order: the picture the user set, or the cover of
/// the first Track-kind item. Those covers need one batched metadata request, for
/// every head a picture-less playlist brings.
pub(crate) async fn label_playlists(
    session: &Session,
    entries: Vec<RootEntry>,
    cache: &LabelCache,
) -> Vec<PlaylistEntry> {
    let uris: Vec<String> = entries.iter().map(|entry| entry.uri.clone()).collect();
    let metas = futures_util::stream::iter(uris)
        .map(|uri| async move { playlist_meta(session, &uri).await })
        .buffered(REMOTE_FETCH_WIDTH)
        .collect::<Vec<_>>()
        .await;

    let labels = fetch_labels(session, &head_candidates(&metas), cache).await;
    let user = session.username();

    entries
        .into_iter()
        .zip(metas)
        .map(|(entry, meta)| {
            let RootEntry { uri, saved_at } = entry;
            let PlaylistMeta {
                name,
                head,
                picture,
                description,
                tracks,
                owner,
                access,
            } = meta;
            let cover = playlist_cover(picture, head.first().map(String::as_str), &labels);
            let composed = composed_of(&head, cover.as_ref(), &labels);
            let (name, named) = playlist_label(&uri, name);
            PlaylistEntry {
                saved_at,
                uri,
                name,
                named,
                cover,
                composed,
                description,
                tracks,
                owner: playlist_edit::owner_of(owner.as_deref(), &user),
                access,
            }
        })
        .collect()
}

/// How many of a playlist's head rows are named.
///
/// A hard cap, so a head of a hundred rows does not turn one card into a hundred
/// labels. Twelve leaves room for a head that repeats one album and still tiles.
const HEAD_TRACKS: usize = 12;

/// Every head URI the label batch has to name, each once, in first-seen order.
fn head_candidates(metas: &[PlaylistMeta]) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    for meta in metas {
        for track in &meta.head {
            if seen.insert(track.clone()) {
                candidates.push(track.clone());
            }
        }
    }
    candidates
}

/// The composed covers a playlist's head tiles, from the labels it was named by.
fn composed_of(head: &[String], cover: Option<&PlaylistCover>, labels: &Labels) -> Vec<CoverSet> {
    composed_from(
        cover.map(|cover| cover.origin),
        head.iter()
            .map(|uri| labels.get(uri).and_then(|label| label.cover.as_ref())),
    )
}

/// What one playlist's metadata tells the listing.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PlaylistMeta {
    name: Option<String>,
    /// The first [`HEAD_TRACKS`] Track-kind item URIs, in list order.
    ///
    /// Empty when the playlist has a picture of its own: a mosaic may never
    /// replace that picture, so naming the head would be a request for nothing.
    head: Vec<String>,
    /// The picture the user set, when the playlist carries a usable one.
    picture: Option<CoverSet>,
    /// The playlist's own description, collapsed to a line, when it has one.
    description: Option<String>,
    /// The playlist's declared track count.
    tracks: Option<usize>,
    /// The playlist's owner as the wire names it, when it names one.
    owner: Option<String>,
    /// The revision and permission the same read carried.
    access: Option<PlaylistAccess>,
}

impl PlaylistMeta {
    fn empty() -> Self {
        Self {
            name: None,
            head: Vec::new(),
            picture: None,
            description: None,
            tracks: None,
            owner: None,
            access: None,
        }
    }
}

/// The text with its runs of whitespace collapsed, or `None` when nothing is left.
///
/// A wire string that is empty or only spaces is the wire saying nothing, which
/// must not become a blank line in the band.
fn non_blank(raw: &str) -> Option<String> {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed)
    }
}

/// The playlist's owner as the wire names it.
///
/// librespot keeps the owner only inside the playlist's `id`, and it fills the
/// user even when the wire gave an empty one, so a blank is filtered here rather
/// than drawn as `Made by `.
fn playlist_owner(uri: &SpotifyUri) -> Option<String> {
    match uri {
        SpotifyUri::Playlist {
            user: Some(user), ..
        } => non_blank(user),
        _ => None,
    }
}

/// The playlist's declared length, when the wire's number is a count.
///
/// The declared length is authoritative even when the read truncated the
/// tracks; a negative number is a wire error rather than a length.
fn playlist_length(length: i32) -> Option<usize> {
    usize::try_from(length).ok()
}

/// Reads one playlist's name, description, owner, head and picture in a single
/// request.
async fn playlist_meta(session: &Session, uri: &str) -> PlaylistMeta {
    let Ok(id) = SpotifyUri::from_uri(uri) else {
        return PlaylistMeta::empty();
    };
    let Ok(playlist) = Playlist::get(session, &id).await else {
        return PlaylistMeta::empty();
    };
    let picture = CoverUrl::from_file_id(&playlist.attributes.picture).map(CoverSet::single);
    PlaylistMeta {
        name: Some(playlist.name().to_owned()),
        head: head_track_uris(picture.as_ref(), playlist.tracks()),
        picture,
        description: non_blank(&playlist.attributes.description),
        tracks: playlist_length(playlist.length),
        owner: playlist_owner(&playlist.id),
        access: playlist_edit::listing_access(
            &playlist.revision,
            playlist.capabilities.can_edit_items,
        ),
    }
}

/// The first [`HEAD_TRACKS`] Track-kind URIs of a picture-less playlist's head.
///
/// Reads the head only: a truncated list still begins with intact items, and the
/// derivation never pages. Episodes and local files are not tracks, so they
/// contribute nothing.
fn head_track_uris<'a>(
    picture: Option<&CoverSet>,
    uris: impl Iterator<Item = &'a SpotifyUri>,
) -> Vec<String> {
    if picture.is_some() {
        return Vec::new();
    }
    uris.filter_map(|uri| match uri {
        SpotifyUri::Track { .. } => uri.to_uri().ok(),
        _ => None,
    })
    .take(HEAD_TRACKS)
    .collect()
}

/// The picture as a chosen cover, else the first track's album cover as a
/// derived one.
fn playlist_cover(
    picture: Option<CoverSet>,
    first_track: Option<&str>,
    labels: &Labels,
) -> Option<PlaylistCover> {
    picture.map(PlaylistCover::chosen).or_else(|| {
        first_track
            .and_then(|track| labels.get(track))
            .and_then(|label| label.cover.clone())
            .map(PlaylistCover::derived)
    })
}

/// The display name and whether it is a real name rather than the URI fallback.
fn playlist_label(uri: &str, name: Option<String>) -> (String, bool) {
    match name {
        Some(name) => (name, true),
        None => (uri.to_owned(), false),
    }
}

/// What a playlist's own state says about writing to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlaylistState {
    pub(crate) revision: Revision,
    pub(crate) can_edit: bool,
    pub(crate) owned: bool,
    /// Every item's URI, in the list's own order.
    pub(crate) items: Vec<String>,
}

/// Reads the version, permission and owner a playlist page must know.
///
/// A state that cannot be read answers `None` rather than failing the page: an
/// unreadable list is one the page may not change.
pub(crate) async fn read_playlist_state(session: &Session, uri: &str) -> Option<PlaylistState> {
    let Ok(id @ SpotifyUri::Playlist { .. }) = SpotifyUri::from_uri(uri) else {
        return None;
    };
    let playlist = match Playlist::get(session, &id).await {
        Ok(playlist) => playlist,
        Err(_) => {
            // The error is not formatted: it can embed a response body.
            log::warn!("playlist state could not be read");
            return None;
        }
    };
    let Some(revision) = Revision::from_wire(&playlist.revision) else {
        log::warn!("playlist state named no revision");
        return None;
    };
    Some(PlaylistState {
        revision,
        can_edit: playlist.capabilities.can_edit_items,
        owned: playlist_edit::owned(playlist_owner(&playlist.id).as_deref(), &session.username()),
        items: item_uris(
            playlist
                .contents
                .items
                .iter()
                .map(|item| item.id.to_uri().ok()),
        ),
    })
}

/// Every item's URI, in the list's own order.
///
/// An item that cannot be spelled keeps its place as an empty string, so a
/// position the page cannot see still breaks the prefix comparison.
fn item_uris(uris: impl Iterator<Item = Option<String>>) -> Vec<String> {
    uris.map(Option::unwrap_or_default).collect()
}

/// The one header a playlist write wants: the protobuf media type.
///
/// `request` is used rather than `request_with_protobuf` so the single-attempt
/// strategy is the request's own; librespot still adds the access point, the
/// query, the length, the token and the client token.
fn playlist_headers() -> Option<HeaderMap> {
    let protobuf: http::HeaderValue = "application/x-protobuf".parse().ok()?;
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, protobuf);
    Some(headers)
}

/// Puts the shared client's retry budget back however the move ends.
struct Restore<'a>(&'a SpClient);

impl Drop for Restore<'_> {
    fn drop(&mut self) {
        self.0.set_strategy(RequestStrategy::TryTimes(10));
    }
}

/// Writes one change set, in exactly one attempt.
///
/// The reply's revision is what the next write names, so a reply that names
/// none leaves the caller with nothing to base the next one on.
async fn write_changes(
    session: &Session,
    endpoint: String,
    body: Vec<u8>,
) -> Result<Revision, playlist_edit::PlaylistError> {
    let bytes = post_protobuf(session, endpoint, body).await?;
    playlist_edit::reply_revision(&bytes).ok_or(playlist_edit::PlaylistError::Reply)
}

/// Posts one protobuf body, in exactly one attempt.
///
/// None of these writes is idempotent: a retry after a reply was lost would
/// apply the change twice, so the shared client is held to one attempt for this
/// call and put back on every exit path, cancellation included.
///
/// The one attempt is session-wide while in force: the worker runs one command at a time.
async fn post_protobuf(
    session: &Session,
    endpoint: String,
    body: Vec<u8>,
) -> Result<Vec<u8>, playlist_edit::PlaylistError> {
    let headers = playlist_headers().ok_or_else(|| {
        playlist_edit::PlaylistError::Source(
            "the playlist media type is not a valid header".to_owned(),
        )
    })?;
    let client = session.spclient();
    client.set_strategy(RequestStrategy::TryTimes(1));
    let _restore = Restore(client);
    let bytes = client
        .request(&Method::POST, &endpoint, Some(headers), Some(&body))
        .await
        .map_err(|error| playlist_edit::PlaylistError::Source(error.kind.to_string()))?;
    Ok(bytes.to_vec())
}

/// Creates a playlist called `name`, answering the URI the reply names.
pub(crate) async fn write_create(
    session: &Session,
    name: &str,
) -> Result<String, playlist_edit::PlaylistError> {
    playlist_edit::require_name(name)?;
    let bytes = post_protobuf(
        session,
        playlist_edit::CREATE_ENDPOINT.to_owned(),
        playlist_edit::create_body(name),
    )
    .await?;
    playlist_edit::created_uri(&bytes).ok_or(playlist_edit::PlaylistError::Reply)
}

/// Writes `moves` to a playlist, as one delta on one revision.
pub(crate) async fn write_move(
    session: &Session,
    uri: &str,
    revision: &Revision,
    moves: &[Move],
) -> Result<Revision, playlist_edit::PlaylistError> {
    playlist_edit::require_moves(moves)?;
    let endpoint = playlist_edit::changes_endpoint(uri)?;
    write_changes(session, endpoint, playlist_edit::move_body(revision, moves)).await
}

/// Renames a playlist to `name`, as one delta on one revision.
///
/// A create cannot name a list, so this is what gives a fresh playlist its
/// title; it takes the revision the read gave and answers the next one.
pub(crate) async fn write_rename(
    session: &Session,
    uri: &str,
    revision: &Revision,
    name: &str,
) -> Result<Revision, playlist_edit::PlaylistError> {
    playlist_edit::require_name(name)?;
    let endpoint = playlist_edit::changes_endpoint(uri)?;
    write_changes(
        session,
        endpoint,
        playlist_edit::rename_body(revision, name),
    )
    .await
}

/// Removes `removals` from a playlist, as one delta on one revision.
pub(crate) async fn write_remove(
    session: &Session,
    uri: &str,
    revision: &Revision,
    removals: &[Removal],
) -> Result<Revision, playlist_edit::PlaylistError> {
    playlist_edit::require_removals(removals)?;
    let endpoint = playlist_edit::changes_endpoint(uri)?;
    write_changes(
        session,
        endpoint,
        playlist_edit::removals_body(revision, removals),
    )
    .await
}

/// Writes one move to the user's rootlist.
pub(crate) async fn write_rootlist_move(
    session: &Session,
    revision: &Revision,
    from: usize,
    to: usize,
) -> Result<Revision, playlist_edit::PlaylistError> {
    let endpoint = playlist_edit::rootlist_changes_endpoint(&session.username())?;
    let moves = [Move {
        from,
        length: 1,
        to,
    }];
    write_changes(
        session,
        endpoint,
        playlist_edit::move_body(revision, &moves),
    )
    .await
}

/// Puts `uri` at the front of the user's rootlist, based on `revision`.
pub(crate) async fn write_rootlist_add(
    session: &Session,
    revision: &Revision,
    uri: &str,
) -> Result<Revision, playlist_edit::PlaylistError> {
    playlist_edit::require_playlist(uri)?;
    let endpoint = playlist_edit::rootlist_changes_endpoint(&session.username())?;
    write_changes(
        session,
        endpoint,
        playlist_edit::add_body(revision, 0, &[uri.to_owned()]),
    )
    .await
}

/// Removes one row from the user's rootlist, based on `revision`.
///
/// The rootlist unlinks only: the playlist itself survives and another client
/// can put it back.
pub(crate) async fn write_rootlist_remove(
    session: &Session,
    revision: &Revision,
    index: usize,
) -> Result<Revision, playlist_edit::PlaylistError> {
    let endpoint = playlist_edit::rootlist_changes_endpoint(&session.username())?;
    write_changes(
        session,
        endpoint,
        playlist_edit::remove_body(revision, index, 1),
    )
    .await
}

/// Writes one saved album or show out of its set.
///
/// The reply is empty, so silence is not proof: the caller's confirmation is the
/// next read. The write is idempotent, so it keeps the client's retry budget.
pub(crate) async fn write_saved(
    session: &Session,
    uri: &str,
    saved: bool,
) -> Result<(), FollowedError> {
    let set = collection::saved_set(uri)?;
    let headers = collection_headers().ok_or_else(|| {
        FollowedError::Source("the collection media type is not a valid header".to_owned())
    })?;
    let username = session.username();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    let body = collection::items_body(
        &username,
        set,
        &[uri],
        saved.then_some(now),
        !saved,
        &client_update_id(),
    );
    session
        .spclient()
        .request(
            &Method::POST,
            "/collection/v2/write",
            Some(headers),
            Some(&body),
        )
        .await
        .map(|_| ())
        .map_err(|error| FollowedError::Source(error.to_string()))
}

/// Appends tracks to a playlist as one change set.
pub(crate) async fn write_add(
    session: &Session,
    uri: &str,
    revision: &Revision,
    from_index: usize,
    tracks: &[String],
) -> Result<Revision, playlist_edit::PlaylistError> {
    playlist_edit::require_tracks(tracks)?;
    let endpoint = playlist_edit::changes_endpoint(uri)?;
    write_changes(
        session,
        endpoint,
        playlist_edit::add_body(revision, from_index, tracks),
    )
    .await
}

/// Reads one album: its metadata, then the labels for its tracks.
///
/// The album is fetched by its own route rather than by walking a context: its
/// order and its numbering are the album's, not a playlist's. A URI that does not
/// name an album never reaches the network, and a request that fails hands the
/// caller the source's message so the page can show the failure with a retry.
pub(crate) async fn read_album(
    session: &Session,
    uri: &str,
    cache: &LabelCache,
) -> Result<AlbumDetail, AlbumError> {
    let id = SpotifyUri::from_uri(uri).map_err(|_| AlbumError::NotAnAlbum)?;
    if !matches!(id, SpotifyUri::Album { .. }) {
        return Err(AlbumError::NotAnAlbum);
    }
    let album = Album::get(session, &id)
        .await
        .map_err(|error| AlbumError::Source(error.to_string()))?;
    let tracks = label_tracks(session, positions(&album), cache).await;
    Ok(detail(uri, &album, tracks))
}

/// One artist with its top tracks, its discography and the followed state.
///
/// Four requests in one command: the artist itself, batched `ALBUM_V4` requests
/// over every release the payload names, one batched `TRACK_V4` over the top
/// tracks, and the collection probe the Follow label reads. The probe is
/// best-effort: a failure leaves the label at `Follow` and the page otherwise
/// intact.
///
/// The release metadata is asked for in chunks, and a failed or short chunk
/// leaves its own releases out rather than emptying the discography: the hero
/// and the top tracks are worth drawing on their own, and what arrived is
/// visible.
pub(crate) async fn read_artist(
    session: &Session,
    uri: &str,
    cache: &LabelCache,
) -> Result<(ArtistDetail, Option<bool>), ArtistError> {
    let id = SpotifyUri::from_uri(uri).map_err(|_| ArtistError::NotAnArtist)?;
    if !matches!(id, SpotifyUri::Artist { .. }) {
        return Err(ArtistError::NotAnArtist);
    }
    let artist = Artist::get(session, &id)
        .await
        .map_err(|error| ArtistError::Source(error.to_string()))?;

    let order = release_uris(&artist);
    let mut answers = Vec::new();
    for chunk in release_chunks(&order) {
        answers.push(batched_request(session, chunk, ExtensionKind::ALBUM_V4).await);
    }
    let releases = releases_of(&order, answers);
    if releases.len() < order.len() {
        log::warn!(
            "{} of {} releases came back from metadata",
            releases.len(),
            order.len()
        );
    }

    let country = session.country();
    let top_tracks = label_tracks(session, top_track_positions(&artist, &country), cache).await;
    let followed = read_follow(session, uri).await;
    let related = related_uris(&artist);
    if related.is_empty() {
        // Both counts, because "none" and "all of them unusable" mean different
        // things: the first says the wire no longer fills the field, the second
        // that this reader drops what it sends.
        log::warn!(
            "the artist payload sent {} related entries, {} of them usable",
            artist.related.len(),
            related.len()
        );
    }
    Ok((artist_detail(uri, &artist, top_tracks, releases), followed))
}

/// One release's track names, in the release's own order.
///
/// Unlike a position list this carries no added date: a release is a record, not
/// a membership. A URI that cannot be labelled falls back to itself, exactly as
/// a playlist row does.
pub(crate) async fn read_release_tracks(
    session: &Session,
    uris: &[String],
    cache: &LabelCache,
) -> Vec<TrackEntry> {
    let positions = uris
        .iter()
        .map(|uri| PageTrack {
            uri: uri.clone(),
            added_at: None,
        })
        .collect();
    label_tracks(session, positions, cache).await
}

/// Reads one show: its metadata, then the labels for its episodes.
///
/// The show is fetched by its own route rather than by walking a context: its
/// order is the show's own. A URI that does not name a show never reaches the
/// network, and a request that fails hands the caller the source's message so the
/// page can show the failure with a retry.
pub(crate) async fn read_show(session: &Session, uri: &str) -> Result<ShowDetail, ShowError> {
    let id = parse_show_uri(uri)?;
    let show = Show::get(session, &id)
        .await
        .map_err(|error| ShowError::Source(error.to_string()))?;
    let episodes = fetch_episodes(session, episode_uris(&show)).await;
    Ok(show_detail(uri, &show, episodes))
}

/// Reads several shows: each show's metadata, then the labels for every episode
/// of all of them in one read.
///
/// The shows are fetched `SHOW_FANOUT` at a time in their own order, and the
/// labels are fetched once over the pooled episode URIs, so a shelf that draws
/// from eight shows costs one label read instead of eight. A show that does not
/// answer is left out, and only a read where every show failed is an error: a
/// partial answer is still the shelf's.
pub(crate) async fn read_episodes(
    session: &Session,
    shows: &[String],
) -> Result<Vec<ShowDetail>, ShowError> {
    let reads: Vec<(String, Result<Show, ShowError>)> = futures_util::stream::iter(shows.to_vec())
        .map(|uri| async move {
            let read = match parse_show_uri(&uri) {
                Ok(id) => Show::get(session, &id)
                    .await
                    .map_err(|source| ShowError::Source(source.to_string())),
                Err(error) => Err(error),
            };
            (uri, read)
        })
        .buffered(SHOW_FANOUT)
        .collect()
        .await;

    let mut first_error = None;
    let mut read = Vec::new();
    for (uri, show) in reads {
        match show {
            Ok(show) => read.push((uri, show)),
            Err(error) => {
                log::warn!("show {uri} could not be read: {error}");
                first_error.get_or_insert(error);
            }
        }
    }
    if read.is_empty() && !shows.is_empty() {
        return match first_error {
            Some(error) => Err(error),
            None => Ok(Vec::new()),
        };
    }

    let mut uris: Vec<String> = Vec::new();
    for (_, show) in &read {
        uris.extend(episode_uris(show));
    }
    let found = fetch_episode_labels(session, &uris).await;
    Ok(show_details(&read, &found))
}

/// The show URI `uri` names, or [`ShowError::NotAShow`].
///
/// Read before any request, so a playlist or album URI costs no round trip.
fn parse_show_uri(uri: &str) -> Result<SpotifyUri, ShowError> {
    let id = SpotifyUri::from_uri(uri).map_err(|_| ShowError::NotAShow)?;
    if !matches!(id, SpotifyUri::Show { .. }) {
        return Err(ShowError::NotAShow);
    }
    Ok(id)
}

/// Reads one track's lyrics.
///
/// The lyrics endpoint is its own route, and it answers a track without lyrics
/// with a not-found rather than an empty list; that answer is settled into empty
/// lyrics, so the pane says the track has none instead of offering a retry. A URI
/// that does not name a track never reaches the network.
pub(crate) async fn read_lyrics(session: &Session, uri: &str) -> Result<TrackLyrics, LyricsError> {
    let id = match SpotifyUri::from_uri(uri) {
        Ok(SpotifyUri::Track { id, .. }) => id,
        _ => return Err(LyricsError::NotATrack),
    };
    lyrics::settled(Lyrics::get(session, &id).await)
}

/// Whether the artist is in the user's followed set, or `None` when the probe
/// did not answer.
async fn read_follow(session: &Session, uri: &str) -> Option<bool> {
    probe_membership(session, collection::ARTIST_SET, uri).await
}

/// Whether one URI is in the set its kind belongs to, or `None` when the probe
/// did not answer.
///
/// The set is the same one the write names, so an album is asked about the
/// collection and a show about the show set. A URI with no set — a track's like
/// is the songs list, not this route — is refused rather than guessed at.
pub(crate) async fn read_saved_state(session: &Session, uri: &str) -> Option<bool> {
    let set = collection::saved_set(uri).ok()?;
    probe_membership(session, set, uri).await
}

async fn probe_membership(session: &Session, set: &str, uri: &str) -> Option<bool> {
    let headers = collection_headers()?;
    let username = session.username();
    let body = collection::contains_body(&username, set, &[uri]);
    match session
        .spclient()
        .request(
            &Method::POST,
            "/collection/v2/contains",
            Some(headers),
            Some(&body),
        )
        .await
    {
        Ok(bytes) => collection::parse_contains(&bytes).into_iter().next(),
        Err(error) => {
            log::warn!("collection probe failed: {error}");
            None
        }
    }
}

/// Writes the artist into the followed set, or out of it.
pub(crate) async fn write_follow(
    session: &Session,
    uri: &str,
    follow: bool,
) -> Result<(), ArtistError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    let username = session.username();
    let body = if follow {
        collection::write_body(&username, uri, Some(now), false)
    } else {
        collection::write_body(&username, uri, None, true)
    };
    let headers = collection_headers().ok_or_else(|| {
        ArtistError::Source("the collection media type is not a valid header".to_owned())
    })?;
    session
        .spclient()
        .request(
            &Method::POST,
            "/collection/v2/write",
            Some(headers),
            Some(&body),
        )
        .await
        .map(|_| ())
        .map_err(|error| ArtistError::Source(error.to_string()))
}

/// How many URIs one like write names.
///
/// The write window is many seconds wide on a busy connection, so a large pick
/// is cut into a few requests rather than one long one; a failure stops the
/// rest and the confirming read is what heals the part that landed.
const LIKE_BATCH: usize = 100;

/// One collection set, paged through the collection endpoint until an answer
/// carries no next token.
///
/// It is the one place a collection sweep picks its cap, so a read of the
/// collection set is never cut short by the followed lists' smaller cap.
///
/// A page that cannot be read fails the whole read rather than drawing a short
/// list as if it were complete. The most answers the read accepts is the pager's.
async fn read_set(
    session: &Session,
    set: &str,
    mut pager: followed::Pager,
) -> Result<Vec<Item>, FollowedError> {
    let headers = collection_headers().ok_or_else(|| {
        FollowedError::Source("the collection media type is not a valid header".to_owned())
    })?;
    let username = session.username();
    let mut token: Option<String> = None;
    let mut items = Vec::new();
    loop {
        let body = collection::paging_body(&username, set, token.as_deref(), followed::PAGE_LIMIT);
        let bytes = session
            .spclient()
            .request(
                &Method::POST,
                "/collection/v2/paging",
                Some(headers.clone()),
                Some(&body),
            )
            .await
            .map_err(|error| FollowedError::Source(error.to_string()))?;
        let page = collection::parse_page(&bytes).ok_or_else(|| {
            FollowedError::Source("the collection answer was not readable".to_owned())
        })?;
        items.extend(page.items);
        match pager.accept(page.next)? {
            Some(next) => token = Some(next),
            None => break,
        }
    }
    Ok(items)
}

/// The whole collection set, paged with the collection cap.
async fn read_collection(session: &Session) -> Result<Vec<Item>, FollowedError> {
    read_set(
        session,
        collection::COLLECTION_SET,
        followed::Pager::with_cap(followed::COLLECTION_MAX_PAGES),
    )
    .await
}

/// The liked tracks, newest save first, each with the date it was saved.
///
/// The set holds saved albums and episodes too, so only track items are kept.
pub(crate) async fn read_liked_tracks(session: &Session) -> Result<Vec<LikedTrack>, FollowedError> {
    Ok(collected::liked_tracks(collected::track_dated(
        read_collection(session).await?,
    )))
}

/// Writes one pick into the liked-songs set, or out of it.
///
/// A pick is cut into batches of [`LIKE_BATCH`] URIs and written in order; the
/// first failure ends the rest. A non-track URI is refused before any request.
pub(crate) async fn write_like(
    session: &Session,
    uris: Vec<String>,
    liked: bool,
) -> Result<(), FollowedError> {
    collection::require_tracks(&uris)?;
    let headers = collection_headers().ok_or_else(|| {
        FollowedError::Source("the collection media type is not a valid header".to_owned())
    })?;
    let username = session.username();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    let refs: Vec<&str> = uris.iter().map(String::as_str).collect();
    for batch in collection::like_batches(&refs, LIKE_BATCH) {
        let body = collection::items_body(
            &username,
            collection::COLLECTION_SET,
            batch,
            liked.then_some(now),
            !liked,
            &client_update_id(),
        );
        session
            .spclient()
            .request(
                &Method::POST,
                "/collection/v2/write",
                Some(headers.clone()),
                Some(&body),
            )
            .await
            .map_err(|error| FollowedError::Source(error.to_string()))?;
    }
    Ok(())
}

/// A label no other write of this run carries, so the endpoint does not treat a
/// second like as the first one repeated.
fn client_update_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    update_id(millis, SEQ.fetch_add(1, Ordering::Relaxed))
}

/// The label's shape: the millisecond orders it in a log and the counter makes
/// it unique, because a write's batches are sent within one millisecond.
fn update_id(millis: u128, seq: u64) -> String {
    format!("takofy-{millis}-{seq}")
}

/// The shows the user follows, newest follow first, each as its card draws it.
///
/// An empty set asks for no metadata at all.
///
/// The cards are read `REMOTE_FETCH_WIDTH` at a time. A show whose metadata
/// does not answer, or answers without a name, keeps its URI and stays in the
/// list: one unreadable show must not hide the others.
pub(crate) async fn read_followed_shows(
    session: &Session,
) -> Result<Vec<FollowedShow>, FollowedError> {
    let items = read_set(session, collection::SHOW_SET, followed::Pager::new()).await?;

    let order = followed::show_order(items);
    if !needs_labels(&order) {
        return Ok(Vec::new());
    }

    let cards: HashMap<String, ShowCard> = futures_util::stream::iter(order.iter().cloned())
        .map(|uri| async move {
            let card = show_meta(session, &uri).await;
            (uri, card)
        })
        .buffered(REMOTE_FETCH_WIDTH)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|(uri, card)| card.map(|card| (uri, card)))
        .collect();
    let shows = followed::entries(&order, &cards);
    let unnamed = shows.iter().filter(|show| !show.named).count();
    if unnamed > 0 {
        log::warn!(
            "{unnamed} of {} followed shows could not be named",
            shows.len()
        );
    }
    Ok(shows)
}

/// Whether the followed order is worth a metadata request at all: an empty set
/// asks for nothing.
fn needs_labels(order: &[String]) -> bool {
    !order.is_empty()
}

/// One followed show's card, or `None` when the show cannot be read or has no
/// name. The caller then falls back to the URI.
async fn show_meta(session: &Session, uri: &str) -> Option<ShowCard> {
    let id = SpotifyUri::from_uri(uri).ok()?;
    let show = Show::get(session, &id).await.ok()?;
    show_card(&show)
}

/// One followed artist's card, or `None` when the artist cannot be read or has
/// no name. The caller then falls back to the URI.
async fn artist_meta(session: &Session, uri: &str) -> Option<ArtistCard> {
    let id = SpotifyUri::from_uri(uri).ok()?;
    let artist = Artist::get(session, &id).await.ok()?;
    collected::artist_card(&artist)
}

/// The artists the user follows, newest follow first, each as its card draws it.
///
/// An empty set asks for no metadata at all. An artist whose metadata does not
/// answer, or answers without a name, keeps its URI and stays in the list: one
/// unreadable artist must not hide the others.
pub(crate) async fn read_followed_artists(
    session: &Session,
) -> Result<Vec<FollowedArtist>, FollowedError> {
    let items = read_set(session, collection::ARTIST_SET, followed::Pager::new()).await?;
    let order = collected::artist_order(items);
    if !needs_labels(&order) {
        return Ok(Vec::new());
    }

    let cards: HashMap<String, ArtistCard> = futures_util::stream::iter(order.iter().cloned())
        .map(|uri| async move {
            let card = artist_meta(session, &uri).await;
            (uri, card)
        })
        .buffered(REMOTE_FETCH_WIDTH)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|(uri, card)| card.map(|card| (uri, card)))
        .collect();
    let artists = collected::artist_entries(&order, &cards);
    let unnamed = artists.iter().filter(|artist| !artist.named).count();
    if unnamed > 0 {
        log::warn!(
            "{unnamed} of {} followed artists could not be named",
            artists.len()
        );
    }
    Ok(artists)
}

/// How long a speculative portrait read may take before it is abandoned.
///
/// A surface asks for at most ten artists in one request, so this is generous
/// for the bytes and short enough that the queue behind it barely notices.
const PORTRAIT_LIMIT: Duration = Duration::from_secs(3);

/// The future's answer, or `None` when `limit` passes first.
///
/// librespot's HTTP client has no request timeout, so this is the only bound a
/// speculative read has.
async fn bounded<T>(future: impl Future<Output = T>, limit: Duration) -> Option<T> {
    tokio::time::timeout(limit, future).await.ok()
}

/// The portraits for `artists`, in the order asked for, from one batched read.
///
/// An empty list asks for nothing and answers nothing. The read is bounded: it
/// is speculative, so a stalled request must not hold a play behind it. A failed
/// or abandoned request is `None`; a partial answer logs how short it was and is
/// otherwise drawn as it came.
pub(crate) async fn read_portraits(
    session: &Session,
    artists: &[String],
) -> Option<Vec<ArtistPortrait>> {
    if artists.is_empty() {
        return Some(Vec::new());
    }
    let uris = distinct_uris(artists);
    let answered = bounded(
        request_parsed(session, &uris, ExtensionKind::ARTIST_V4, portrait_from),
        PORTRAIT_LIMIT,
    )
    .await;
    let Some(found) = answered else {
        log::debug!("portrait read abandoned after {PORTRAIT_LIMIT:?}");
        return None;
    };
    let (found, _) = found?;
    let portraits = portraits_of(&uris, &found);
    if portraits.len() < uris.len() {
        log::warn!("{} of {} portraits came back", portraits.len(), uris.len());
    }
    Some(portraits)
}

/// The related artists an artist page shows as cards, in the payload's order.
///
/// An empty list asks for no metadata at all. The cards are read
/// `REMOTE_FETCH_WIDTH` at a time. An artist whose metadata does not answer, or
/// answers without a name, keeps its URI and stays in the row: one unreadable
/// relative must not shrink it.
pub(crate) async fn read_related(
    session: &Session,
    uris: Vec<String>,
) -> Result<Vec<FollowedArtist>, ArtistError> {
    if !needs_labels(&uris) {
        return Ok(Vec::new());
    }

    let cards: HashMap<String, ArtistCard> = futures_util::stream::iter(uris.iter().cloned())
        .map(|uri| async move {
            let card = artist_meta(session, &uri).await;
            (uri, card)
        })
        .buffered(REMOTE_FETCH_WIDTH)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|(uri, card)| card.map(|card| (uri, card)))
        .collect();
    let related = collected::artist_entries(&uris, &cards);
    let unnamed = related.iter().filter(|artist| !artist.named).count();
    if unnamed > 0 {
        log::warn!(
            "{unnamed} of {} related artists could not be named",
            related.len()
        );
    }
    Ok(related)
}

/// The albums the user saves, newest save first, each as its card draws it.
///
/// An empty set asks for no metadata at all. A chunk whose request fails costs
/// only its albums, which stay in the list unnamed.
pub(crate) async fn read_saved_albums(session: &Session) -> Result<Vec<SavedAlbum>, FollowedError> {
    let dated = collected::album_dated(read_collection(session).await?);
    let order: Vec<String> = dated.iter().map(|dated| dated.uri.clone()).collect();
    if !needs_labels(&order) {
        return Ok(Vec::new());
    }

    let mut answers = Vec::new();
    for chunk in release_chunks(&order) {
        answers.push(batched_request(session, chunk, ExtensionKind::ALBUM_V4).await);
    }
    let albums = collected::stamp_albums(collected::saved_album_entries(&order, answers), &dated);
    let named = albums.iter().filter(|album| album.named).count();
    if named < albums.len() {
        log::warn!("{named} of {} saved albums came back named", albums.len());
    }
    Ok(albums)
}

/// The two headers both collection endpoints want: the vendor media type on
/// `Content-Type` and `Accept`.
///
/// `request` is used rather than `request_with_protobuf` because the latter
/// hardcodes `application/x-protobuf`, which these endpoints refuse.
fn collection_headers() -> Option<HeaderMap> {
    let vendor: http::HeaderValue = collection::VENDOR.parse().ok()?;
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, vendor.clone());
    headers.insert(ACCEPT, vendor);
    Some(headers)
}

/// Reads context pages through librespot's spclient.
pub(crate) struct RemoteContext<'a> {
    pub(crate) session: &'a Session,
}

impl PageSource for RemoteContext<'_> {
    async fn resolve(&self, uri: &str) -> Result<Resolved, ContextError> {
        let context = self
            .session
            .spclient()
            .get_context(uri)
            .await
            .map_err(|error| ContextError::Source(error.to_string()))?;
        Ok(Resolved {
            pages: context.pages.iter().map(page_from_proto).collect(),
            cover: context_cover(&context.metadata),
        })
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

/// The context's own image: `image_url`, else `header_image_url_desktop`.
///
/// A value that fails the host allowlist falls through to the next key rather
/// than ending the search.
fn context_cover(metadata: &HashMap<String, String>) -> Option<CoverSet> {
    ["image_url", "header_image_url_desktop"]
        .iter()
        .filter_map(|key| metadata.get(*key))
        .find_map(|value| CoverUrl::from_wire(value).map(CoverSet::single))
}

/// What the wire says about one track: the name and artists the row already
/// draws, plus the album and length it can now draw too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrackLabel {
    title: String,
    artists: String,
    album: Option<String>,
    album_uri: Option<String>,
    artist_uris: Vec<String>,
    artist_names: Vec<String>,
    duration_ms: Option<u32>,
    cover: Option<CoverSet>,
}

/// Labels for tracks, keyed by URI.
type Labels = HashMap<String, TrackLabel>;

/// What one session remembers of the labels it has already read.
pub(crate) type LabelCache = UriCache<TrackLabel>;

/// A label carrying only a title, so a test can seat one in a cache.
#[cfg(test)]
pub(crate) fn titled_label(title: &str) -> TrackLabel {
    TrackLabel {
        title: title.to_owned(),
        artists: String::new(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        duration_ms: None,
        cover: None,
    }
}

/// How many distinct URIs one metadata request asks for.
///
/// The server accepted 500 without complaint, but it deduplicates within a
/// request, so that run could not report where the real ceiling is. 250 keeps
/// clear of it, and a short answer is retried in smaller pieces anyway.
const LABEL_CHUNK: usize = 250;

/// Names each position, falling back to its URI when metadata is missing.
///
/// The URIs are deduplicated for the network request only: the result keeps one
/// entry per position, with that position's own added date.
pub(crate) async fn label_tracks(
    session: &Session,
    items: Vec<PageTrack>,
    cache: &LabelCache,
) -> Vec<TrackEntry> {
    let uris: Vec<String> = items.iter().map(|item| item.uri.clone()).collect();
    let labels = fetch_labels(session, &distinct_uris(&uris), cache).await;
    let unnamed = uris.iter().filter(|uri| !labels.contains_key(*uri)).count();
    if unnamed > 0 {
        log::warn!("{unnamed} of {} tracks could not be named", uris.len());
    }
    expand_labels(&items, &labels)
}

/// The wire's episode prefix: everything else is asked for as a track.
const EPISODE_PREFIX: &str = "spotify:episode:";

/// Splits distinct URIs into `(tracks, episodes)`.
///
/// Only the prefix decides, so a local file or a URI this build does not know is
/// asked for as a track, exactly as it was before episodes had a kind. Order is
/// kept within each group, because a chunk's answer is what the split retries.
fn partition_by_kind(uris: &[String]) -> (Vec<String>, Vec<String>) {
    let mut tracks = Vec::new();
    let mut episodes = Vec::new();
    for uri in uris {
        if uri.starts_with(EPISODE_PREFIX) {
            episodes.push(uri.clone());
        } else {
            tracks.push(uri.clone());
        }
    }
    (tracks, episodes)
}

/// The distinct URIs, in first-seen order.
fn distinct_uris(uris: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    uris.iter()
        .filter(|uri| seen.insert((*uri).clone()))
        .cloned()
        .collect()
}

/// One named track per input position. A position with no name falls back to
/// its URI, which is visible rather than plausible.
///
/// The added date is read from the position, never looked up by URI: one track
/// may sit in the same playlist twice with two different dates.
fn expand_labels(items: &[PageTrack], labels: &Labels) -> Vec<TrackEntry> {
    items
        .iter()
        .map(|item| match labels.get(&item.uri) {
            Some(label) => TrackEntry {
                uri: item.uri.clone(),
                title: label.title.clone(),
                artists: label.artists.clone(),
                album: label.album.clone(),
                album_uri: label.album_uri.clone(),
                artist_uris: label.artist_uris.clone(),
                artist_names: label.artist_names.clone(),
                added_at: item.added_at,
                duration_ms: label.duration_ms,
                named: true,
                cover: label.cover.clone(),
            },
            None => TrackEntry {
                uri: item.uri.clone(),
                title: item.uri.clone(),
                artists: String::new(),
                album: None,
                album_uri: None,
                artist_uris: Vec::new(),
                artist_names: Vec::new(),
                added_at: item.added_at,
                duration_ms: None,
                named: false,
                cover: None,
            },
        })
        .collect()
}

/// Fetches names for distinct track and episode URIs, reusing the labels this
/// session has already read.
async fn fetch_labels(session: &Session, distinct: &[String], cache: &LabelCache) -> Labels {
    cache
        .through_cache(distinct.to_vec(), |missing: Vec<String>| {
            fetch_labels_wire(session, missing)
        })
        .await
}

/// The wire read behind [`fetch_labels`].
///
/// The one label seam branches on URI kind: tracks keep the `TRACK_V4` batch they
/// always used, and episodes get `EPISODE_V4`. The two batches are independent,
/// so one failing does not lose the other.
async fn fetch_labels_wire(session: &Session, distinct: Vec<String>) -> Labels {
    let (tracks, episodes) = partition_by_kind(&distinct);
    let (tracks, mut tally) =
        fetch_chunked(session, &tracks, ExtensionKind::TRACK_V4, track_label_from).await;
    let (episodes, episode_tally) =
        fetch_chunked(session, &episodes, ExtensionKind::EPISODE_V4, episode_from).await;
    tally.merge(episode_tally);
    if let Some(summary) = tally.summary() {
        log::warn!("{summary}");
    }
    merge_labels(tracks, episode_labels(episodes))
}

/// The episode labels, keyed by URI, from the episode batch's answers.
fn episode_labels(found: HashMap<String, Episode>) -> Labels {
    found
        .into_iter()
        .map(|(uri, episode)| (uri, label_from_episode(&episode)))
        .collect()
}

/// An episode as the one label a row draws, through its own [`Episode::entry`].
///
/// This is the whole episode-to-label conversion: an episode's show stands in
/// for the artists, and it carries no album or membership date.
fn label_from_episode(episode: &Episode) -> TrackLabel {
    let entry = episode.entry();
    TrackLabel {
        title: entry.title,
        artists: entry.artists,
        album: entry.album,
        album_uri: entry.album_uri,
        artist_uris: entry.artist_uris,
        artist_names: entry.artist_names,
        duration_ms: entry.duration_ms,
        cover: entry.cover,
    }
}

/// The two batches' labels as one map.
///
/// The batches are independent: a failed episode batch arrives as an empty map
/// and leaves every track label intact, and the other way round. Their keys are
/// disjoint by construction, because a URI is in exactly one partition.
fn merge_labels(tracks: Labels, episodes: Labels) -> Labels {
    let mut merged = tracks;
    merged.extend(episodes);
    merged
}

/// Fetches the episodes for `uris`, one per requested URI in input order.
///
/// An unanswered URI falls back to itself and reads as unnamed, exactly as a
/// track does, so a show page still draws every episode the wire listed.
async fn fetch_episodes(session: &Session, uris: Vec<String>) -> Vec<Episode> {
    let found = fetch_episode_labels(session, &uris).await;
    expand_episodes(&uris, &found)
}

/// The episode labels for `uris`, keyed by URI, from one read.
///
/// A pooled read asks for every show's episodes at once, so the caller expands
/// each show's own order from this map rather than getting another list.
async fn fetch_episode_labels(session: &Session, uris: &[String]) -> HashMap<String, Episode> {
    let distinct = distinct_uris(uris);
    let (found, tally) =
        fetch_chunked(session, &distinct, ExtensionKind::EPISODE_V4, episode_from).await;
    if let Some(summary) = tally.summary() {
        log::warn!("{summary}");
    }
    let unnamed = distinct
        .iter()
        .filter(|uri| !found.contains_key(*uri))
        .count();
    if unnamed > 0 {
        log::warn!(
            "{unnamed} of {} episodes could not be named",
            distinct.len()
        );
    }
    found
}

/// Fetches values for distinct URIs with one extension kind, splitting a chunk
/// whenever the server answers only part of it.
///
/// `parse` reads one answer's bytes together with the entity URI that answer
/// echoed, so a value can be named by the URI the server answered for.
async fn fetch_chunked<T>(
    session: &Session,
    distinct: &[String],
    kind: ExtensionKind,
    parse: fn(&[u8], &str) -> Option<T>,
) -> (HashMap<String, T>, StatusTally) {
    fetch_chunked_with(distinct, |chunk| async move {
        request_parsed(session, &chunk, kind, parse).await
    })
    .await
}

/// How many chunks one pass keeps in flight.
const CHUNK_FANOUT: usize = 3;

/// Fetches values for distinct URIs, one request per chunk, `CHUNK_FANOUT`
/// chunks at a time.
///
/// A chunk the server answered only part of is split in half and asked again,
/// one at a time: the truncation is the rare answer, so the retries do not need
/// the fan-out's extra requests.
async fn fetch_chunked_with<T, F, Fut>(
    distinct: &[String],
    request: F,
) -> (HashMap<String, T>, StatusTally)
where
    F: Fn(Vec<String>) -> Fut,
    Fut: Future<Output = Option<(HashMap<String, T>, StatusTally)>>,
{
    let mut found: HashMap<String, T> = HashMap::new();
    let mut tally = StatusTally::default();
    let mut retries: Vec<Vec<String>> = Vec::new();
    let chunks: Vec<Vec<String>> = distinct
        .chunks(LABEL_CHUNK)
        .map(<[String]>::to_vec)
        .collect();
    let answers = futures_util::stream::iter(chunks)
        .map(|chunk: Vec<String>| {
            let asked = chunk.clone();
            let request = &request;
            async move {
                let answer = request(asked).await;
                (chunk, answer)
            }
        })
        .buffered(CHUNK_FANOUT)
        .collect::<Vec<_>>()
        .await;
    for (chunk, answer) in answers {
        absorb(&chunk, answer, &mut found, &mut tally, &mut retries);
    }
    while let Some(chunk) = retries.pop() {
        let answer = request(chunk.clone()).await;
        absorb(&chunk, answer, &mut found, &mut tally, &mut retries);
    }
    (found, tally)
}

/// Folds one chunk's answer into the pass, queueing the part it left unanswered.
fn absorb<T>(
    chunk: &[String],
    answer: Option<(HashMap<String, T>, StatusTally)>,
    found: &mut HashMap<String, T>,
    tally: &mut StatusTally,
    retries: &mut Vec<Vec<String>>,
) {
    let Some((answers, chunk_tally)) = answer else {
        return;
    };
    tally.merge(chunk_tally);
    let answered = !answers.is_empty();
    let mut missing: Vec<String> = chunk
        .iter()
        .filter(|uri| !answers.contains_key(*uri))
        .cloned()
        .collect();
    found.extend(answers);
    if missing.is_empty() || !answered || missing.len() == 1 {
        // Nothing arrived at all means the provider refused the chunk rather
        // than truncated it, and retrying one URI at a time is the request
        // storm this exists to avoid. A single unanswered URI is simply
        // unknown. Either way the caller falls back to the URI.
        return;
    }
    log::debug!(
        "{} of {} entities came back; retrying the rest in smaller requests",
        chunk.len() - missing.len(),
        chunk.len()
    );
    let right = missing.split_off(missing.len() / 2);
    retries.push(right);
    retries.push(missing);
}

/// One batched extended-metadata request over `uris`.
///
/// `None` when the request itself failed, which asking for less will not fix.
/// The answer's order is not the request's, so callers key their entities by the
/// `entity_uri` the answer echoes.
async fn batched_request(
    session: &Session,
    uris: &[String],
    extension: ExtensionKind,
) -> Option<BatchedExtensionResponse> {
    let request = BatchedEntityRequest {
        entity_request: uris
            .iter()
            .map(|uri| EntityRequest {
                entity_uri: uri.clone(),
                query: vec![ExtensionQuery {
                    extension_kind: extension.into(),
                    ..Default::default()
                }],
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };

    match session.spclient().get_extended_metadata(request).await {
        Ok(response) => Some(response),
        Err(error) => {
            log::warn!(
                "metadata request for {} entities failed: {error}",
                uris.len()
            );
            None
        }
    }
}

/// One batched request for `uris` under `kind`. `None` when the request itself
/// failed, which asking for less will not fix.
///
/// The answer is keyed by the `entity_uri` it echoes, and each value is read by
/// `parse`, which is the only part that knows the kind's wire shape. That echoed
/// URI is handed to `parse` too, so a value can carry the URI the server answered
/// for instead of rebuilding one from its own id.
async fn request_parsed<T>(
    session: &Session,
    chunk: &[String],
    kind: ExtensionKind,
    parse: fn(&[u8], &str) -> Option<T>,
) -> Option<(HashMap<String, T>, StatusTally)> {
    let response = batched_request(session, chunk, kind).await?;
    let tally = tally_of(&response);

    let mut found = HashMap::new();
    for array in &response.extended_metadata {
        for data in &array.extension_data {
            let Some(any) = data.extension_data.as_ref() else {
                continue;
            };
            if let Some(value) = parse(&any.value, &data.entity_uri) {
                found.insert(data.entity_uri.clone(), value);
            }
        }
    }
    Some((found, tally))
}

/// The statuses one batched answer reports: the array's provider code, and each
/// entity's own code under the URI it echoed.
fn tally_of(response: &BatchedExtensionResponse) -> StatusTally {
    let mut tally = StatusTally::default();
    for array in &response.extended_metadata {
        tally.record_provider(array.header.provider_error_status);
        for data in &array.extension_data {
            tally.record_entity(&data.entity_uri, data.header.status_code);
        }
    }
    tally
}

/// The label an encoded `TRACK_V4` answer carries, or `None` when it is not a
/// named track.
///
/// A track's label is already built from its own album and artist ids, so the
/// echoed URI is not read here; the parameter is the seam's shape.
fn track_label_from(bytes: &[u8], _entity_uri: &str) -> Option<TrackLabel> {
    TrackMessage::parse_from_bytes(bytes)
        .ok()
        .and_then(|track| label_of(&track))
}

/// The episode an encoded `EPISODE_V4` answer carries, or `None` when it is not
/// a named episode.
///
/// `entity_uri` is the URI the answer echoed, so it names the episode even when
/// the message's own id is unusable.
fn episode_from(bytes: &[u8], entity_uri: &str) -> Option<Episode> {
    EpisodeMessage::parse_from_bytes(bytes)
        .ok()
        .and_then(|episode| episode_of(&episode, entity_uri))
}

/// The label a parsed track message carries, or `None` when it has no name.
///
/// This is the whole mapping from the wire's `Track` into a `TrackLabel`, kept
/// pure so it can be exercised with a constructed message and no session.
fn label_of(track: &TrackMessage) -> Option<TrackLabel> {
    let title = track.name.clone().filter(|name| !name.is_empty())?;
    let artists = track
        .artist
        .iter()
        .filter_map(|artist| artist.name.clone())
        .collect::<Vec<_>>()
        .join(", ");
    let album = track
        .album
        .as_ref()
        .and_then(|album| album.name.clone())
        .filter(|name| !name.is_empty());
    let album_uri = track
        .album
        .as_ref()
        .and_then(|album| uri_of(album.gid.as_deref(), |id| SpotifyUri::Album { id }));
    // One pass builds both lists, so a name can never be paired with another
    // artist's URI or outlive the list it describes.
    let credits: Vec<(String, String)> = track
        .artist
        .iter()
        .map_while(|artist| {
            let name = artist.name.as_deref().filter(|name| !name.is_empty())?;
            let uri = uri_of(artist.gid.as_deref(), |id| SpotifyUri::Artist { id })?;
            Some((uri, name.to_owned()))
        })
        .collect();
    let (artist_uris, artist_names) = credits.into_iter().unzip();
    Some(TrackLabel {
        title,
        artists,
        album,
        album_uri,
        artist_uris,
        artist_names,
        duration_ms: duration_ms(track.duration),
        cover: track
            .album
            .as_ref()
            .and_then(|album| covers_in_group(album.cover_group.as_ref())),
    })
}

/// The episode a parsed wire message carries, or `None` when it has no name.
///
/// This is the whole mapping from the wire's `Episode` into an [`Episode`], kept
/// pure so it can be exercised with a constructed message and no session.
/// `entity_uri` is the URI the answer echoed, which names the episode; the
/// message's own gid is only needed for the show link.
fn episode_of(episode: &EpisodeMessage, entity_uri: &str) -> Option<Episode> {
    let title = episode.name.clone().filter(|name| !name.is_empty())?;
    let show = episode.show.as_ref();
    Some(Episode {
        // The echoed URI names the episode, so an answer whose gid is unusable
        // still keeps the title the server did send.
        uri: entity_uri.to_owned(),
        title,
        // A blank show name is no name: it would otherwise put a link to a
        // nameless show in the row's credit.
        show: show
            .and_then(|show| show.name.as_deref())
            .and_then(non_blank)
            .unwrap_or_default(),
        show_uri: show.and_then(|show| uri_of(show.gid.as_deref(), |id| SpotifyUri::Show { id })),
        description: non_blank(episode.description.as_deref().unwrap_or_default()),
        published: published_at(episode.publish_time.as_ref()),
        duration_ms: duration_ms(episode.duration),
        cover: covers_in_group(episode.cover_image.as_ref()),
        named: true,
    })
}

/// The publish time as epoch milliseconds, or `None` when the wire's date fails
/// to convert (an absent `publish_time` included) or lands at or before zero.
fn published_at(publish_time: Option<&DateMessage>) -> Option<PublishedAt> {
    let date = Date::try_from(publish_time?).ok()?;
    PublishedAt::from_millis(date.as_timestamp_ms())
}

/// A show as its followed-list card draws it, or `None` when it has no name.
///
/// The card does not draw the description, so it is not read. A nameless show is
/// no card, which lets the caller fall back to the URI.
fn show_card(show: &Show) -> Option<ShowCard> {
    let name = Some(show.name.clone()).filter(|name| !name.is_empty())?;
    Some(ShowCard {
        name,
        publisher: non_blank(&show.publisher),
        cover: covers_in_images(&show.covers),
        episodes: show.episodes.len(),
    })
}

/// The URI a raw id names, when the bytes are a well-formed id for `page`.
///
/// A wrong length is not an id, so it gives no URI rather than an approximate
/// one; the caller then has nothing to link to and draws plain text.
pub(crate) fn uri_of(gid: Option<&[u8]>, page: fn(SpotifyId) -> SpotifyUri) -> Option<String> {
    page(SpotifyId::from_raw(gid?).ok()?).to_uri().ok()
}

/// A wire length in milliseconds, or `None` when the wire did not give one.
///
/// The field is a signed 32-bit count, and the model stores an unsigned one. A
/// negative value is not a length, so it becomes `None` rather than wrapping to
/// nearly fifty days. Every non-negative value already fits a `u32`, so no
/// non-negative value can wrap; an implausibly large one still reads as a large
/// `m:ss` rather than a small one, and this does not invent a plausibility cap.
fn duration_ms(duration: Option<i32>) -> Option<u32> {
    duration.and_then(|milliseconds| u32::try_from(milliseconds).ok())
}

#[cfg(test)]
mod tests;
