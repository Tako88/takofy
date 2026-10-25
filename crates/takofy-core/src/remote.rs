//! librespot-backed rootlist source and playlist labelling.
//!
//! The only file in this crate that performs librespot I/O.

use std::collections::{HashMap, HashSet};
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
    FollowedShow, Move, PlaylistAccess, PlaylistCover, PlaylistEntry, PublishedAt, Removal,
    Revision, SavedAlbum, ShowDetail, TrackEntry, TrackLyrics, composed_from,
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
use crate::portrait::{bounded, portrait_from, portraits_of};
use crate::rootlist::{RootlistError, RootlistPage, RootlistSource, RootlistWire, parse_rootlist};
use crate::show::{ShowError, detail as show_detail, episode_uris};
use crate::status_tally::StatusTally;

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
///
/// A playlist's cover is, in order: the picture the user set, or the cover of
/// the first Track-kind item. Those covers need one batched metadata request, for
/// every head a picture-less playlist brings.
pub(crate) async fn label_playlists(
    session: &Session,
    uris: Vec<String>,
    cache: &LabelCache,
) -> Vec<PlaylistEntry> {
    let metas = futures_util::stream::iter(uris.iter().cloned())
        .map(|uri| async move { playlist_meta(session, &uri).await })
        .buffered(8)
        .collect::<Vec<_>>()
        .await;

    let labels = fetch_labels(session, &head_candidates(&metas), cache).await;
    let user = session.username();

    uris.into_iter()
        .zip(metas)
        .map(|(uri, meta)| {
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

/// The liked tracks' URIs, newest save first.
///
/// The set holds saved albums and episodes too, so only track items are kept.
pub(crate) async fn read_liked_tracks(session: &Session) -> Result<Vec<String>, FollowedError> {
    Ok(collected::track_order(read_collection(session).await?))
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
/// The cards are read one show at a time, `buffered(8)`, as the playlist listing
/// does. A show whose metadata does not answer, or answers without a name, keeps
/// its URI and stays in the list: one unreadable show must not hide the others.
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
        .buffered(8)
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
        .buffered(8)
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

/// The portraits for `artists`, in the order asked for, from one batched read.
///
/// An empty list asks for nothing and answers nothing. The read is bounded: it
/// is speculative, and the worker answers commands one at a time, so a stalled
/// request must not hold the queue behind it. A failed or abandoned request is
/// `None`; a partial answer logs how short it was and is otherwise drawn as it
/// came.
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
/// An empty list asks for no metadata at all. The cards are read one artist at a
/// time, `buffered(8)`, as the followed-artists list does. An artist whose
/// metadata does not answer, or answers without a name, keeps its URI and stays
/// in the row: one unreadable relative must not shrink it.
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
        .buffered(8)
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
    let items = read_collection(session).await?;
    let order = collected::album_order(items);
    if !needs_labels(&order) {
        return Ok(Vec::new());
    }

    let mut answers = Vec::new();
    for chunk in release_chunks(&order) {
        answers.push(batched_request(session, chunk, ExtensionKind::ALBUM_V4).await);
    }
    let albums = collected::saved_album_entries(&order, answers);
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
    let distinct = distinct_uris(&uris);
    let (found, tally) =
        fetch_chunked(session, &distinct, ExtensionKind::EPISODE_V4, episode_from).await;
    if let Some(summary) = tally.summary() {
        log::warn!("{summary}");
    }
    let unnamed = uris.iter().filter(|uri| !found.contains_key(*uri)).count();
    if unnamed > 0 {
        log::warn!("{} of {} episodes could not be named", unnamed, uris.len());
    }
    expand_episodes(&uris, &found)
}

/// One episode per input position, falling back to the URI when it was not
/// answered. The wire's order is the page's order, so this never sorts.
///
/// The position's own URI is what it draws: the answer is looked up by that URI,
/// and an answer that carried a different one cannot move the episode to another
/// position.
fn expand_episodes(uris: &[String], found: &HashMap<String, Episode>) -> Vec<Episode> {
    uris.iter()
        .map(|uri| match found.get(uri) {
            Some(episode) => Episode {
                uri: uri.clone(),
                ..episode.clone()
            },
            None => Episode {
                uri: uri.clone(),
                title: uri.clone(),
                show: String::new(),
                show_uri: None,
                description: None,
                published: None,
                duration_ms: None,
                cover: None,
                named: false,
            },
        })
        .collect()
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
    let mut found: HashMap<String, T> = HashMap::new();
    let mut tally = StatusTally::default();
    let mut queue: Vec<Vec<String>> = distinct
        .chunks(LABEL_CHUNK)
        .map(<[String]>::to_vec)
        .collect();
    queue.reverse();
    while let Some(chunk) = queue.pop() {
        let Some((answers, chunk_tally)) = request_parsed(session, &chunk, kind, parse).await
        else {
            continue;
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
            continue;
        }
        log::debug!(
            "{} of {} entities came back; retrying the rest in smaller requests",
            chunk.len() - missing.len(),
            chunk.len()
        );
        let right = missing.split_off(missing.len() / 2);
        queue.push(right);
        queue.push(missing);
    }
    (found, tally)
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
fn uri_of(gid: Option<&[u8]>, page: fn(SpotifyId) -> SpotifyUri) -> Option<String> {
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
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use librespot::core::{Session, SessionConfig, SpotifyId, SpotifyUri};
    use librespot::metadata::Show;
    use librespot::protocol::entity_extension_data::{
        EntityExtensionData, EntityExtensionDataHeader,
    };
    use librespot::protocol::extended_metadata::{
        BatchedExtensionResponse, EntityExtensionDataArray, EntityExtensionDataArrayHeader,
    };
    use librespot::protocol::metadata::image::Size;
    use librespot::protocol::metadata::{
        Album, Artist, Date as DateMessage, Episode as EpisodeMessage, Image, ImageGroup,
        Show as ShowMessage, Track as TrackMessage,
    };
    use protobuf::{Message as _, MessageField};
    use takofy_model::{
        AddedAt, CoverOrigin, CoverSet, CoverUrl, Episode, PlaylistCover, PublishedAt,
    };

    use crate::context::PageTrack;
    use crate::playlist_edit::owned as owned_by;
    use crate::show::ShowError;

    use super::{
        HEAD_TRACKS, LabelCache, Labels, PlaylistMeta, TrackLabel, client_update_id, composed_of,
        context_cover, distinct_uris, duration_ms, episode_from, episode_labels, episode_of,
        expand_episodes, expand_labels, head_candidates, head_track_uris, item_uris,
        label_from_episode, label_of, label_tracks, merge_labels, needs_labels, non_blank,
        parse_show_uri, partition_by_kind, playlist_cover, playlist_headers, playlist_label,
        playlist_length, playlist_owner, published_at, show_card, tally_of, titled_label,
        track_label_from, update_id,
    };

    const TRACK_ID: &str = "4uLU6hMCjMI75M1A2tKUQC";
    const EPISODE_ID: &str = "6rqhFgbbKwnb9MLmUQDhG6";
    const PLAYLIST_ID: &str = "37i9dQZF1DXcBWIGoYBM5M";

    fn cover_url() -> CoverUrl {
        CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url")
    }

    fn cover_set() -> CoverSet {
        CoverSet::single(cover_url())
    }

    fn single(url: &str) -> CoverSet {
        CoverSet::single(CoverUrl::from_wire(url).expect("a cover url"))
    }

    fn first_track() -> SpotifyUri {
        SpotifyUri::Track {
            id: SpotifyId::from_base62(TRACK_ID).expect("a track id"),
        }
    }

    fn track_uri(index: u8) -> SpotifyUri {
        let mut raw = [0u8; 16];
        raw[15] = index + 1;
        SpotifyUri::Track {
            id: SpotifyId::from_raw(&raw).expect("a track id"),
        }
    }

    /// A label naming one cover, so a head can be tiled in a test.
    fn covered_label(uri: &str, tail: &str) -> (String, TrackLabel) {
        let mut label = titled_label(uri);
        label.cover =
            CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).map(CoverSet::single);
        (uri.to_owned(), label)
    }

    fn episode() -> SpotifyUri {
        SpotifyUri::Episode {
            id: SpotifyId::from_base62(EPISODE_ID).expect("an episode id"),
        }
    }

    fn local() -> SpotifyUri {
        SpotifyUri::Local {
            artist: "Artist".to_owned(),
            album_title: "Album".to_owned(),
            track_title: "Title".to_owned(),
            duration: Duration::from_secs(3),
        }
    }

    fn owned(uris: &[&str]) -> Vec<String> {
        uris.iter().map(|uri| (*uri).to_owned()).collect()
    }

    fn items(uris: &[&str]) -> Vec<PageTrack> {
        uris.iter()
            .map(|uri| PageTrack {
                uri: (*uri).to_owned(),
                added_at: None,
            })
            .collect()
    }

    fn dated(uri: &str, millis: u64) -> PageTrack {
        PageTrack {
            uri: uri.to_owned(),
            added_at: AddedAt::from_wire(&millis.to_string()),
        }
    }

    fn named(pairs: &[(&str, &str, &str)]) -> Labels {
        pairs
            .iter()
            .map(|(uri, title, artists)| {
                (
                    (*uri).to_owned(),
                    TrackLabel {
                        title: (*title).to_owned(),
                        artists: (*artists).to_owned(),
                        album: None,
                        album_uri: None,
                        artist_uris: Vec::new(),
                        artist_names: Vec::new(),
                        duration_ms: None,
                        cover: None,
                    },
                )
            })
            .collect()
    }

    fn labelled(
        uri: &str,
        title: &str,
        artists: &str,
        album: Option<&str>,
        duration_ms: Option<u32>,
    ) -> Labels {
        let mut labels = Labels::new();
        labels.insert(
            uri.to_owned(),
            TrackLabel {
                title: title.to_owned(),
                artists: artists.to_owned(),
                album: album.map(str::to_owned),
                album_uri: None,
                artist_uris: Vec::new(),
                artist_names: Vec::new(),
                duration_ms,
                cover: None,
            },
        );
        labels
    }

    #[test]
    fn distinct_uris_drops_repeats_and_keeps_first_seen_order() {
        let uris = owned(&["a", "b", "a", "c", "b", "a"]);
        assert_eq!(distinct_uris(&uris), owned(&["a", "b", "c"]));
    }

    #[test]
    fn distinct_uris_of_nothing_is_nothing() {
        assert!(distinct_uris(&[]).is_empty());
    }

    /// The control for the seeded pass: with nothing cached, the same read
    /// reaches for the wire, gets nothing from a session that is not connected,
    /// and falls back to the URI.
    #[tokio::test]
    async fn an_uncached_pass_leaves_the_row_unnamed_and_stores_nothing() {
        let cache = LabelCache::new();
        let session = Session::new(SessionConfig::default(), None);

        let rows = label_tracks(&session, items(&["spotify:track:a"]), &cache).await;

        assert!(!rows[0].named);
        assert_eq!(rows[0].title, "spotify:track:a");
        assert_eq!(cache.len(), 0);
    }

    /// A pass over URIs the cache already holds names the rows without a wire
    /// read: only the cache can answer, because the session is not connected.
    #[tokio::test]
    async fn a_cached_labelling_pass_names_the_rows_without_the_wire() {
        let cache = LabelCache::new();
        cache.store(&named(&[(
            "spotify:track:a",
            "Cached Title",
            "Cached Band",
        )]));
        let session = Session::new(SessionConfig::default(), None);

        let rows = label_tracks(&session, items(&["spotify:track:a"]), &cache).await;

        assert!(rows[0].named);
        assert_eq!(rows[0].title, "Cached Title");
        assert_eq!(rows[0].artists, "Cached Band");
        assert_eq!(cache.len(), 1);
    }

    /// Two labelling-shaped passes over the same URIs, through the same cache,
    /// ask a stand-in wire read for those URIs exactly once.
    #[tokio::test]
    async fn a_second_labelling_pass_over_the_same_uris_makes_no_second_fetch() {
        let asked = std::sync::Mutex::new(Vec::new());
        let cache = LabelCache::new();
        let uris = owned(&["spotify:track:a", "spotify:track:b"]);

        let mut passes = Vec::new();
        for _ in 0..2 {
            passes.push(
                cache
                    .through_cache(uris.clone(), |missing: Vec<String>| {
                        asked.lock().expect("lock").extend(missing.clone());
                        async move {
                            missing
                                .into_iter()
                                .map(|uri| (uri, titled_label("A Title")))
                                .collect::<Labels>()
                        }
                    })
                    .await,
            );
        }

        assert_eq!(*asked.lock().expect("lock"), uris);
        assert_eq!(passes[1], passes[0]);
    }

    #[test]
    fn every_position_is_named_when_a_track_repeats() {
        let entries = expand_labels(
            &items(&["a", "b", "a"]),
            &named(&[("a", "One", "Band"), ("b", "Two", "Band")]),
        );
        let titles: Vec<&str> = entries.iter().map(|entry| entry.title.as_str()).collect();
        assert_eq!(titles, vec!["One", "Two", "One"]);
    }

    #[test]
    fn a_track_repeated_six_times_is_named_six_times_from_one_entity() {
        let entries = expand_labels(&items(&["a"; 6]), &named(&[("a", "One", "Band")]));
        assert_eq!(entries.len(), 6);
        assert!(entries.iter().all(|entry| entry.title == "One"));
    }

    #[test]
    fn an_unnamed_track_falls_back_to_its_uri() {
        let entries = expand_labels(&items(&["a", "b"]), &named(&[("a", "One", "Band")]));
        assert_eq!(entries[1].title, "b");
        assert!(entries[1].artists.is_empty());
        assert_eq!(entries[1].uri, "b");
        assert_eq!(entries[1].album, None);
        assert_eq!(entries[1].duration_ms, None);
        assert!(!entries[1].named);
    }

    #[test]
    fn a_named_track_keeps_its_uri() {
        let entries = expand_labels(&items(&["a"]), &named(&[("a", "One", "Band")]));
        assert_eq!(entries[0].uri, "a");
        assert_eq!(entries[0].artists, "Band");
        assert!(entries[0].named);
    }

    #[test]
    fn a_labelled_track_carries_its_album_and_length() {
        let labels = labelled("a", "One", "Band", Some("Record"), Some(222_000));
        let entries = expand_labels(&items(&["a"]), &labels);
        assert_eq!(entries[0].album.as_deref(), Some("Record"));
        assert_eq!(entries[0].duration_ms, Some(222_000));
        assert!(entries[0].named);
    }

    #[test]
    fn a_track_with_no_album_or_length_carries_neither() {
        let entries = expand_labels(&items(&["a"]), &named(&[("a", "One", "Band")]));
        assert_eq!(entries[0].album, None);
        assert_eq!(entries[0].duration_ms, None);
        assert!(entries[0].named);
    }

    #[test]
    fn expand_labels_copies_the_album_and_artist_uris() {
        let mut labels = Labels::new();
        labels.insert(
            "a".to_owned(),
            TrackLabel {
                title: "One".to_owned(),
                artists: "Band".to_owned(),
                album: Some("Record".to_owned()),
                album_uri: Some("spotify:album:x".to_owned()),
                artist_uris: vec!["spotify:artist:y".to_owned()],
                artist_names: vec!["Band".to_owned()],
                duration_ms: None,
                cover: None,
            },
        );
        let entries = expand_labels(&items(&["a", "b"]), &labels);
        assert_eq!(entries[0].album_uri.as_deref(), Some("spotify:album:x"));
        assert_eq!(entries[0].artist_uris, vec!["spotify:artist:y".to_owned()]);
        assert_eq!(entries[0].artist_names, vec!["Band".to_owned()]);
        assert_eq!(entries[1].album_uri, None);
        assert!(entries[1].artist_uris.is_empty());
        assert!(entries[1].artist_names.is_empty());
    }

    #[test]
    fn a_repeated_track_keeps_each_positions_own_date() {
        let positions = vec![
            dated("a", 1_700_000_000_000),
            dated("b", 1_710_000_000_000),
            dated("a", 1_720_000_000_000),
        ];
        let entries = expand_labels(
            &positions,
            &named(&[("a", "One", "Band"), ("b", "Two", "Band")]),
        );
        let dates: Vec<Option<u64>> = entries
            .iter()
            .map(|entry| entry.added_at.map(AddedAt::as_millis))
            .collect();
        assert_eq!(
            dates,
            vec![
                Some(1_700_000_000_000),
                Some(1_710_000_000_000),
                Some(1_720_000_000_000)
            ],
            "a URI-keyed date would repeat one of the three"
        );
    }

    #[test]
    fn an_unnamed_track_keeps_its_date() {
        let entries = expand_labels(&[dated("a", 1_700_000_000_000)], &Labels::new());
        assert!(!entries[0].named);
        assert_eq!(entries[0].uri, "a");
        assert_eq!(
            entries[0].added_at.map(AddedAt::as_millis),
            Some(1_700_000_000_000)
        );
    }

    #[test]
    fn a_negative_wire_length_is_not_a_length() {
        assert_eq!(duration_ms(Some(-1)), None);
        assert_eq!(duration_ms(Some(i32::MIN)), None);
        assert_eq!(duration_ms(None), None);
    }

    #[test]
    fn a_non_negative_wire_length_is_kept() {
        assert_eq!(duration_ms(Some(0)), Some(0));
        assert_eq!(duration_ms(Some(222_000)), Some(222_000));
        assert_eq!(duration_ms(Some(i32::MAX)), Some(i32::MAX as u32));
    }

    fn wire_track(album: Option<&str>, duration: Option<i32>) -> TrackMessage {
        TrackMessage {
            name: Some("One".to_owned()),
            artist: vec![Artist {
                name: Some("Band".to_owned()),
                ..Default::default()
            }],
            album: album
                .map(|name| Album {
                    name: Some(name.to_owned()),
                    ..Default::default()
                })
                .into(),
            duration,
            ..Default::default()
        }
    }

    #[test]
    fn label_of_reads_an_album_that_is_present() {
        let label = label_of(&wire_track(Some("Record"), Some(222_000))).unwrap();
        assert_eq!(label.album.as_deref(), Some("Record"));
        assert_eq!(label.duration_ms, Some(222_000));
        assert_eq!(label.title, "One");
        assert_eq!(label.artists, "Band");
    }

    #[test]
    fn label_of_reads_no_album_when_the_album_is_absent() {
        let label = label_of(&wire_track(None, Some(222_000))).unwrap();
        assert_eq!(label.album, None);
    }

    #[test]
    fn label_of_reads_no_album_when_the_album_name_is_empty() {
        let label = label_of(&wire_track(Some(""), Some(222_000))).unwrap();
        assert_eq!(label.album, None);
    }

    fn base62(gid: &[u8]) -> String {
        SpotifyId::from_raw(gid)
            .expect("a valid id")
            .to_base62()
            .expect("a base62 id")
    }

    fn artist(name: Option<&str>, gid: Option<Vec<u8>>) -> Artist {
        Artist {
            name: name.map(str::to_owned),
            gid,
            ..Default::default()
        }
    }

    fn album(name: Option<&str>, gid: Option<Vec<u8>>) -> Album {
        Album {
            name: name.map(str::to_owned),
            gid,
            ..Default::default()
        }
    }

    fn track_with(album: Album, artists: Vec<Artist>) -> TrackMessage {
        TrackMessage {
            name: Some("One".to_owned()),
            artist: artists,
            album: Some(album).into(),
            ..Default::default()
        }
    }

    #[test]
    fn label_of_reads_the_album_and_artist_uris() {
        let album_gid: Vec<u8> = (1..=16).collect();
        let artist_gid: Vec<u8> = (17..=32).collect();
        let track = track_with(
            album(Some("Record"), Some(album_gid.clone())),
            vec![artist(Some("Band"), Some(artist_gid.clone()))],
        );
        let label = label_of(&track).expect("a named track");
        let album_uri = format!("spotify:album:{}", base62(&album_gid));
        let artist_uri = format!("spotify:artist:{}", base62(&artist_gid));
        assert_eq!(label.album_uri.as_deref(), Some(album_uri.as_str()));
        assert_eq!(label.artist_uris, vec![artist_uri.clone()]);
        // Cross-check the round trip against the raw id, not just the variant
        // and not just the base62 formatting the implementation also uses.
        let Ok(SpotifyUri::Album { id }) = SpotifyUri::from_uri(&album_uri) else {
            panic!("an album uri parses as an album uri");
        };
        assert_eq!(id, SpotifyId::from_raw(&album_gid).expect("a valid id"));
        let Ok(SpotifyUri::Artist { id }) = SpotifyUri::from_uri(&artist_uri) else {
            panic!("an artist uri parses as an artist uri");
        };
        assert_eq!(id, SpotifyId::from_raw(&artist_gid).expect("a valid id"));
    }

    #[test]
    fn label_of_reads_every_artist_uri_in_order() {
        let first_gid: Vec<u8> = (17..=32).collect();
        let second_gid: Vec<u8> = (33..=48).collect();
        let track = track_with(
            album(Some("Record"), None),
            vec![
                artist(Some("A"), Some(first_gid.clone())),
                artist(Some("B"), Some(second_gid.clone())),
            ],
        );
        let label = label_of(&track).expect("a named track");
        assert_eq!(label.artists, "A, B");
        assert_eq!(
            label.artist_uris,
            vec![
                format!("spotify:artist:{}", base62(&first_gid)),
                format!("spotify:artist:{}", base62(&second_gid)),
            ]
        );
    }

    #[test]
    fn a_gidless_artist_mid_list_keeps_only_the_uris_before_it() {
        let first_gid: Vec<u8> = (17..=32).collect();
        let track = track_with(
            album(Some("Record"), None),
            vec![
                artist(Some("A"), Some(first_gid.clone())),
                artist(Some("B"), None),
                artist(Some("C"), Some(vec![49; 16])),
            ],
        );
        let label = label_of(&track).expect("a named track");
        assert_eq!(label.artists, "A, B, C");
        assert_eq!(
            label.artist_uris,
            vec![format!("spotify:artist:{}", base62(&first_gid))]
        );
    }

    #[test]
    fn a_gid_of_the_wrong_length_gives_no_uri() {
        let track = track_with(
            album(Some("Record"), Some(vec![1; 15])),
            vec![artist(Some("Band"), Some(vec![2; 17]))],
        );
        let label = label_of(&track).expect("a named track");
        assert_eq!(label.album_uri, None);
        assert!(label.artist_uris.is_empty());
    }

    #[test]
    fn an_absent_gid_gives_no_uri() {
        let track = track_with(
            album(Some("Record"), None),
            vec![artist(Some("Band"), None)],
        );
        let label = label_of(&track).expect("a named track");
        assert_eq!(label.album_uri, None);
        assert!(label.artist_uris.is_empty());
    }

    #[test]
    fn an_artist_without_a_gid_ends_the_artist_uris() {
        let track = track_with(
            album(Some("Record"), None),
            vec![
                artist(Some("A"), None),
                artist(Some("B"), Some(vec![3; 16])),
            ],
        );
        let label = label_of(&track).expect("a named track");
        assert_eq!(label.artists, "A, B");
        assert!(label.artist_uris.is_empty());
    }

    #[test]
    fn an_artist_without_a_name_ends_the_artist_uris_despite_a_gid() {
        let track = track_with(
            album(Some("Record"), None),
            vec![
                artist(Some("A"), Some(vec![5; 16])),
                artist(None, Some(vec![6; 16])),
                artist(Some("C"), Some(vec![7; 16])),
            ],
        );
        let label = label_of(&track).expect("a named track");
        assert_eq!(label.artists, "A, C");
        assert_eq!(label.artist_uris.len(), 1, "only A's uri survives");
    }

    #[test]
    fn an_artist_with_an_empty_name_ends_the_artist_uris() {
        let track = track_with(
            album(Some("Record"), None),
            vec![artist(Some(""), Some(vec![4; 16]))],
        );
        let label = label_of(&track).expect("a named track");
        assert!(label.artist_uris.is_empty());
    }

    #[test]
    fn label_of_reads_every_artist_name_in_order_with_its_uri() {
        let first_gid: Vec<u8> = (17..=32).collect();
        let second_gid: Vec<u8> = (33..=48).collect();
        let track = track_with(
            album(Some("Record"), None),
            vec![
                artist(Some("A"), Some(first_gid)),
                artist(Some("B"), Some(second_gid)),
            ],
        );
        let label = label_of(&track).expect("a named track");
        assert_eq!(label.artist_names, vec!["A".to_owned(), "B".to_owned()]);
        assert_eq!(label.artist_names.len(), label.artist_uris.len());
    }

    #[test]
    fn a_gidless_artist_ends_the_artist_names_with_the_uris() {
        let first_gid: Vec<u8> = (17..=32).collect();
        let track = track_with(
            album(Some("Record"), None),
            vec![
                artist(Some("A"), Some(first_gid)),
                artist(Some("B"), None),
                artist(Some("C"), Some(vec![49; 16])),
            ],
        );
        let label = label_of(&track).expect("a named track");
        assert_eq!(label.artist_names, vec!["A".to_owned()]);
        assert_eq!(label.artist_names.len(), label.artist_uris.len());
    }

    #[test]
    fn a_nameless_artist_ends_the_artist_names_with_the_uris() {
        let track = track_with(
            album(Some("Record"), None),
            vec![
                artist(Some("A"), Some(vec![5; 16])),
                artist(None, Some(vec![6; 16])),
                artist(Some("C"), Some(vec![7; 16])),
            ],
        );
        let label = label_of(&track).expect("a named track");
        assert_eq!(label.artist_names, vec!["A".to_owned()]);
        assert_eq!(label.artist_names.len(), label.artist_uris.len());
    }

    #[test]
    fn an_unlabelled_fallback_has_no_artist_names_or_uris() {
        let entries = expand_labels(&items(&["a"]), &Labels::new());
        assert!(entries[0].artist_names.is_empty());
        assert!(entries[0].artist_uris.is_empty());
    }

    #[test]
    fn an_album_uri_survives_an_empty_album_name() {
        let album_gid: Vec<u8> = (33..=48).collect();
        let track = track_with(
            album(Some(""), Some(album_gid.clone())),
            vec![artist(Some("Band"), None)],
        );
        let label = label_of(&track).expect("a named track");
        let album_uri = format!("spotify:album:{}", base62(&album_gid));
        assert_eq!(label.album, None, "an empty name is not an album name");
        assert_eq!(label.album_uri.as_deref(), Some(album_uri.as_str()));

        let mut labels = Labels::new();
        labels.insert("a".to_owned(), label);
        let entries = expand_labels(&items(&["a"]), &labels);
        assert_eq!(entries[0].album, None);
        assert_eq!(entries[0].album_uri.as_deref(), Some(album_uri.as_str()));
    }

    #[test]
    fn label_of_reads_a_length_that_is_present() {
        let label = label_of(&wire_track(None, Some(222_000))).unwrap();
        assert_eq!(label.duration_ms, Some(222_000));
    }

    #[test]
    fn label_of_reads_no_length_when_it_is_negative_or_absent() {
        let negative = label_of(&wire_track(None, Some(-1))).unwrap();
        assert_eq!(negative.duration_ms, None);
        let absent = label_of(&wire_track(None, None)).unwrap();
        assert_eq!(absent.duration_ms, None);
    }

    #[test]
    fn label_of_is_none_when_the_track_has_no_name() {
        let unnamed = TrackMessage {
            name: Some(String::new()),
            ..Default::default()
        };
        assert_eq!(label_of(&unnamed), None);
    }

    #[test]
    fn playlist_label_marks_a_real_name() {
        let (name, named) = playlist_label("spotify:playlist:x", Some("Mix".to_owned()));
        assert_eq!(name, "Mix");
        assert!(named);
    }

    #[test]
    fn playlist_label_falls_back_to_the_uri() {
        let (name, named) = playlist_label("spotify:playlist:x", None);
        assert_eq!(name, "spotify:playlist:x");
        assert!(!named);
    }

    fn wire_track_with_image(size: Size, file_id: &[u8]) -> TrackMessage {
        let mut image = Image::new();
        image.set_size(size);
        image.set_file_id(file_id.to_vec());
        TrackMessage {
            name: Some("One".to_owned()),
            artist: vec![Artist {
                name: Some("Band".to_owned()),
                ..Default::default()
            }],
            album: Some(Album {
                cover_group: Some(ImageGroup {
                    image: vec![image],
                    ..Default::default()
                })
                .into(),
                ..Default::default()
            })
            .into(),
            ..Default::default()
        }
    }

    #[test]
    fn label_of_reads_a_grouped_cover() {
        let label = label_of(&wire_track_with_image(Size::DEFAULT, &[7; 20])).unwrap();
        let cover = label.cover.expect("a cover");
        assert!(
            cover.variants()[0]
                .url
                .as_str()
                .ends_with("0707070707070707070707070707070707070707")
        );
    }

    #[test]
    fn label_of_reads_no_cover_when_the_group_is_absent() {
        let label = label_of(&wire_track(Some("Record"), Some(222_000))).unwrap();
        assert_eq!(label.cover, None);
    }

    #[test]
    fn context_cover_prefers_image_url() {
        let mut metadata = HashMap::new();
        metadata.insert(
            "image_url".to_owned(),
            "https://i.scdn.co/image/a".to_owned(),
        );
        metadata.insert(
            "header_image_url_desktop".to_owned(),
            "https://i.scdn.co/image/b".to_owned(),
        );
        assert_eq!(
            context_cover(&metadata),
            Some(single("https://i.scdn.co/image/a"))
        );
    }

    #[test]
    fn context_cover_falls_back_to_the_header() {
        let mut metadata = HashMap::new();
        metadata.insert(
            "header_image_url_desktop".to_owned(),
            "https://i.scdn.co/image/b".to_owned(),
        );
        assert_eq!(
            context_cover(&metadata),
            Some(single("https://i.scdn.co/image/b"))
        );
    }

    #[test]
    fn context_cover_skips_an_off_allowlist_image_url() {
        let mut metadata = HashMap::new();
        metadata.insert(
            "image_url".to_owned(),
            "https://evil.example/image/a".to_owned(),
        );
        metadata.insert(
            "header_image_url_desktop".to_owned(),
            "https://pickasso.spotifycdn.com/image/b".to_owned(),
        );
        assert_eq!(
            context_cover(&metadata),
            Some(single("https://pickasso.spotifycdn.com/image/b"))
        );
    }

    #[test]
    fn context_cover_is_none_without_a_key_or_with_a_non_https_value() {
        assert_eq!(context_cover(&HashMap::new()), None);
        let mut metadata = HashMap::new();
        metadata.insert("image_url".to_owned(), "/image/a".to_owned());
        assert_eq!(context_cover(&metadata), None);
    }

    #[test]
    fn the_head_skips_episodes_and_locals() {
        let uris = [episode(), local(), first_track()];
        assert_eq!(
            head_track_uris(None, uris.iter()),
            vec![format!("spotify:track:{TRACK_ID}")]
        );
    }

    #[test]
    fn the_head_is_capped_at_twelve_track_uris() {
        let uris: Vec<SpotifyUri> = (0..20).map(track_uri).collect();
        let head = head_track_uris(None, uris.iter());
        assert_eq!(head.len(), HEAD_TRACKS);
        assert_eq!(
            head.last().map(String::as_str),
            uris[HEAD_TRACKS - 1].to_uri().ok().as_deref(),
            "the cap keeps the first twelve, in head order"
        );
    }

    #[test]
    fn a_playlist_with_a_chosen_picture_costs_no_head_labels() {
        let uris = [first_track(), track_uri(1), track_uri(2)];
        assert!(
            head_track_uris(Some(&cover_set()), uris.iter()).is_empty(),
            "a chosen picture is never replaced, so its head is never named"
        );
        let chosen = PlaylistMeta {
            picture: Some(cover_set()),
            ..PlaylistMeta::empty()
        };
        assert!(head_candidates(&[chosen]).is_empty());
    }

    #[test]
    fn an_episode_only_head_composes_nothing() {
        let uris = [episode(), local()];
        let head = head_track_uris(None, uris.iter());
        assert!(
            head.is_empty(),
            "neither an episode nor a local file is a track"
        );
        assert!(
            composed_of(&head, None, &Labels::new()).is_empty(),
            "no tracks, no mosaic: today's cover stands"
        );
    }

    #[test]
    fn a_head_of_repeated_albums_composes_fewer_than_four() {
        let head: Vec<String> = (0..HEAD_TRACKS)
            .map(|index| format!("spotify:track:{index}"))
            .collect();

        let labels: Labels = head
            .iter()
            .enumerate()
            .map(|(index, uri)| covered_label(uri, if index % 2 == 0 { "a" } else { "b" }))
            .collect();
        let composed = composed_of(&head, None, &labels);
        assert_eq!(
            composed.len(),
            2,
            "two albums across twelve rows tile two quadrants: {composed:?}"
        );

        let one_album: Labels = head.iter().map(|uri| covered_label(uri, "a")).collect();
        assert!(
            composed_of(&head, None, &one_album).is_empty(),
            "one album repeated is one picture, so the card keeps the single cover"
        );
    }

    #[tokio::test]
    async fn every_playlists_head_is_named_in_one_batch() {
        let metas = [
            PlaylistMeta {
                head: owned(&["a", "b"]),
                ..PlaylistMeta::empty()
            },
            PlaylistMeta {
                head: owned(&["b", "c"]),
                ..PlaylistMeta::empty()
            },
            PlaylistMeta {
                head: owned(&["c", "d"]),
                ..PlaylistMeta::empty()
            },
            PlaylistMeta {
                picture: Some(cover_set()),
                ..PlaylistMeta::empty()
            },
        ];
        let candidates = head_candidates(&metas);
        assert_eq!(
            candidates,
            owned(&["a", "b", "c", "d"]),
            "every head, each name once, in first-seen order"
        );

        let asked = std::sync::Mutex::new(Vec::new());
        let cache = LabelCache::new();
        let labels = cache
            .through_cache(candidates.clone(), |missing: Vec<String>| {
                asked.lock().expect("lock").extend(missing.clone());
                async move {
                    missing
                        .into_iter()
                        .map(|uri| (uri, titled_label("A title")))
                        .collect::<Labels>()
                }
            })
            .await;

        assert_eq!(
            *asked.lock().expect("lock"),
            candidates,
            "one cache pass asks the wire for every head at once"
        );
        assert_eq!(labels.len(), candidates.len());
    }

    #[test]
    fn item_uris_keeps_a_spellable_items_uri() {
        let uris = [first_track()];
        assert_eq!(
            item_uris(uris.iter().map(|uri| uri.to_uri().ok())),
            vec![format!("spotify:track:{TRACK_ID}")]
        );
    }

    #[test]
    fn item_uris_keeps_an_unspellable_item_in_place() {
        let uris = [
            Some("spotify:track:a".to_owned()),
            None,
            Some("spotify:track:b".to_owned()),
        ];
        assert_eq!(
            item_uris(uris.into_iter()),
            vec!["spotify:track:a", "", "spotify:track:b"]
        );
    }

    #[test]
    fn playlist_cover_prefers_the_picture() {
        let mut labels = Labels::new();
        labels.insert(
            "t".to_owned(),
            TrackLabel {
                title: "One".to_owned(),
                artists: String::new(),
                album: None,
                album_uri: None,
                artist_uris: Vec::new(),
                artist_names: Vec::new(),
                duration_ms: None,
                cover: Some(CoverSet::single(cover_url())),
            },
        );
        assert_eq!(
            playlist_cover(Some(cover_set()), Some("t"), &labels),
            Some(PlaylistCover::chosen(cover_url()))
        );
    }

    #[test]
    fn playlist_cover_falls_back_to_the_first_track() {
        let mut labels = Labels::new();
        labels.insert(
            "t".to_owned(),
            TrackLabel {
                title: "One".to_owned(),
                artists: String::new(),
                album: None,
                album_uri: None,
                artist_uris: Vec::new(),
                artist_names: Vec::new(),
                duration_ms: None,
                cover: Some(CoverSet::single(cover_url())),
            },
        );
        let cover = playlist_cover(None, Some("t"), &labels).expect("a derived cover");
        assert_eq!(cover, PlaylistCover::derived(cover_url()));
        assert_eq!(cover.origin, CoverOrigin::Derived);
    }

    #[test]
    fn playlist_cover_is_none_without_a_picture_or_a_known_first_track() {
        let labels = Labels::new();
        assert_eq!(playlist_cover(None, None, &labels), None);
        assert_eq!(playlist_cover(None, Some("t"), &labels), None);
    }

    fn playlist_uri(user: Option<&str>) -> SpotifyUri {
        SpotifyUri::Playlist {
            user: user.map(str::to_owned),
            id: SpotifyId::from_base62(PLAYLIST_ID).expect("a playlist id"),
        }
    }

    #[test]
    fn non_blank_collapses_whitespace_and_drops_a_blank() {
        assert_eq!(non_blank(""), None);
        assert_eq!(non_blank("  \n"), None);
        assert_eq!(non_blank(" x "), Some("x".to_owned()));
        assert_eq!(non_blank("a\n b"), Some("a b".to_owned()));
    }

    #[test]
    fn playlist_headers_name_the_protobuf_media_type() {
        let headers = playlist_headers().expect("headers");
        assert_eq!(
            headers
                .get(http::header::CONTENT_TYPE)
                .expect("a content type"),
            "application/x-protobuf"
        );
    }

    #[test]
    fn playlist_owner_reads_a_named_owner_and_skips_a_blank_one() {
        assert_eq!(
            playlist_owner(&playlist_uri(Some("alice"))),
            Some("alice".to_owned())
        );
        assert_eq!(playlist_owner(&playlist_uri(Some(""))), None);
        assert_eq!(playlist_owner(&playlist_uri(Some("  "))), None);
        assert_eq!(playlist_owner(&playlist_uri(None)), None);
        assert_eq!(playlist_owner(&first_track()), None);
    }

    #[test]
    fn a_playlist_uri_owner_is_compared_as_playlist_owner_spells_it() {
        assert!(owned_by(
            playlist_owner(&playlist_uri(Some("alice"))).as_deref(),
            "alice"
        ));
        assert!(!owned_by(
            playlist_owner(&playlist_uri(Some("Alice"))).as_deref(),
            "alice"
        ));
        assert!(!owned_by(
            playlist_owner(&playlist_uri(Some(""))).as_deref(),
            ""
        ));
        assert!(!owned_by(
            playlist_owner(&playlist_uri(None)).as_deref(),
            "alice"
        ));
        assert!(!owned_by(
            playlist_owner(&first_track()).as_deref(),
            "alice"
        ));
    }

    #[test]
    fn a_collapsed_owner_name_is_not_owned() {
        // `owned` compares byte-exact against the collapsed owner `playlist_owner` yields.
        assert!(!owned_by(
            playlist_owner(&playlist_uri(Some("a  b"))).as_deref(),
            "a  b"
        ));
        assert!(owned_by(
            playlist_owner(&playlist_uri(Some("a  b"))).as_deref(),
            "a b"
        ));
    }

    #[test]
    fn empty_playlist_meta_has_no_description_owner_or_access() {
        let meta = PlaylistMeta::empty();
        assert_eq!(meta.description, None);
        assert_eq!(meta.owner, None);
        assert_eq!(meta.tracks, None);
        assert_eq!(meta.access, None, "a meta that read nothing writes nothing");
    }

    #[test]
    fn a_declared_length_is_the_count_and_a_negative_one_is_not() {
        assert_eq!(playlist_length(62), Some(62));
        assert_eq!(playlist_length(0), Some(0), "an empty playlist counts zero");
        assert_eq!(playlist_length(-1), None, "a wire error is not a length");
    }

    /// A 16-byte gid, the only length `SpotifyId::from_raw` accepts.
    fn gid(byte: u8) -> Vec<u8> {
        vec![byte; 16]
    }

    fn episode_uri(byte: u8) -> String {
        SpotifyUri::Episode {
            id: SpotifyId::from_raw(&gid(byte)).expect("a 16-byte id"),
        }
        .to_uri()
        .expect("a uri")
    }

    fn show_uri(byte: u8) -> String {
        SpotifyUri::Show {
            id: SpotifyId::from_raw(&gid(byte)).expect("a 16-byte id"),
        }
        .to_uri()
        .expect("a uri")
    }

    /// An episode message with every field the label reads set.
    fn wire_episode(name: &str) -> EpisodeMessage {
        let mut episode = EpisodeMessage::default();
        episode.set_gid(gid(2));
        episode.set_name(name.to_owned());
        episode.set_description("A description".to_owned());
        episode.set_duration(2_773_461);
        let mut date = DateMessage::default();
        date.set_year(2026);
        date.set_month(10);
        date.set_day(16);
        episode.publish_time = Some(date).into();
        let mut show = ShowMessage::default();
        show.set_gid(gid(3));
        show.set_name("A Show".to_owned());
        episode.show = Some(show).into();
        episode
    }

    fn grouped_cover(size: Size, file_id: &[u8]) -> ImageGroup {
        let mut image = Image::new();
        image.set_size(size);
        image.set_file_id(file_id.to_vec());
        ImageGroup {
            image: vec![image],
            ..Default::default()
        }
    }

    #[test]
    fn partition_separates_episodes_from_tracks() {
        let track = format!("spotify:track:{TRACK_ID}");
        let episode = format!("spotify:episode:{EPISODE_ID}");
        let (tracks, episodes) = partition_by_kind(&owned(&[&track, &episode]));
        assert_eq!(tracks, vec![track]);
        assert_eq!(episodes, vec![episode]);
    }

    #[test]
    fn a_local_or_unknown_uri_goes_with_the_tracks() {
        let local = local().to_uri().expect("a local uri");
        let (tracks, episodes) =
            partition_by_kind(&owned(&[&local, "not-a-uri", "spotify:episode:1"]));
        assert_eq!(tracks, vec![local, "not-a-uri".to_owned()]);
        assert_eq!(episodes, vec!["spotify:episode:1".to_owned()]);
    }

    #[test]
    fn partition_keeps_order_within_each_group() {
        let uris = owned(&[
            "spotify:track:a",
            "spotify:episode:b",
            "spotify:track:c",
            "spotify:episode:d",
        ]);
        let (tracks, episodes) = partition_by_kind(&uris);
        assert_eq!(
            tracks,
            vec!["spotify:track:a".to_owned(), "spotify:track:c".to_owned()]
        );
        assert_eq!(
            episodes,
            vec![
                "spotify:episode:b".to_owned(),
                "spotify:episode:d".to_owned()
            ]
        );
    }

    #[test]
    fn an_episode_uri_with_no_id_stays_in_the_episode_group() {
        // Pinned: the split reads only the prefix, so an empty id is still asked
        // for as an episode rather than falling back to the track batch.
        let (tracks, episodes) = partition_by_kind(&owned(&["spotify:episode:"]));
        assert!(tracks.is_empty());
        assert_eq!(episodes, vec!["spotify:episode:".to_owned()]);
    }

    #[test]
    fn track_label_from_reads_an_encoded_track_and_refuses_garbage() {
        let bytes = wire_track(Some("Record"), Some(222_000))
            .write_to_bytes()
            .expect("encoded bytes");
        let label =
            track_label_from(&bytes, &format!("spotify:track:{TRACK_ID}")).expect("a named track");
        assert_eq!(label.title, "One");
        assert_eq!(label.artists, "Band");
        assert_eq!(label.duration_ms, Some(222_000));
        assert_eq!(track_label_from(&[0xff, 0xff], "spotify:track:a"), None);
    }

    #[test]
    fn a_full_episode_message_maps_every_field() {
        let mut wire = wire_episode("An episode");
        wire.cover_image = Some(grouped_cover(Size::DEFAULT, &[7; 20])).into();
        let episode = episode_of(&wire, &episode_uri(9)).expect("a named episode");
        assert_eq!(
            episode.uri,
            episode_uri(9),
            "the echoed URI, not the message gid's"
        );
        assert_eq!(episode.title, "An episode");
        assert_eq!(episode.show, "A Show");
        assert_eq!(episode.show_uri.as_deref(), Some(show_uri(3).as_str()));
        assert_eq!(episode.description.as_deref(), Some("A description"));
        assert_eq!(
            episode.published.map(PublishedAt::as_millis),
            Some(1_792_108_800_000),
            "2026-10-16 at midnight UTC"
        );
        assert_eq!(episode.duration_ms, Some(2_773_461));
        assert_eq!(
            episode
                .cover
                .as_ref()
                .and_then(|cover| cover.variants().first())
                .map(|variant| variant.url.as_str()),
            Some("https://i.scdn.co/image/0707070707070707070707070707070707070707")
        );
        assert!(episode.named);
    }

    #[test]
    fn a_missing_or_zero_publish_date_is_none() {
        let uri = episode_uri(2);
        let mut absent = wire_episode("An episode");
        absent.publish_time = None.into();
        assert_eq!(
            episode_of(&absent, &uri)
                .expect("a named episode")
                .published,
            None
        );

        let mut zero = wire_episode("An episode");
        zero.publish_time = Some(DateMessage::default()).into();
        assert_eq!(
            episode_of(&zero, &uri).expect("a named episode").published,
            None
        );

        let mut before_epoch = wire_episode("An episode");
        let mut date = DateMessage::default();
        date.set_year(1969);
        date.set_month(1);
        date.set_day(1);
        before_epoch.publish_time = Some(date).into();
        assert_eq!(
            episode_of(&before_epoch, &uri)
                .expect("a named episode")
                .published,
            None
        );
    }

    #[test]
    fn a_year_only_publish_date_is_january_first_midnight() {
        let uri = episode_uri(2);
        let mut date = DateMessage::default();
        date.set_year(2026);
        assert_eq!(
            published_at(Some(&date)).map(PublishedAt::as_millis),
            Some(1_767_225_600_000),
            "no month or day means January 1 at 00:00 UTC"
        );

        let mut wire = wire_episode("An episode");
        wire.publish_time = Some(date).into();
        assert_eq!(
            episode_of(&wire, &uri)
                .expect("a named episode")
                .published
                .map(PublishedAt::as_millis),
            Some(1_767_225_600_000)
        );
    }

    #[test]
    fn a_bad_show_gid_gives_no_show_uri() {
        let mut wire = wire_episode("An episode");
        let mut show = ShowMessage::default();
        show.set_gid(vec![1; 15]);
        show.set_name("A Show".to_owned());
        wire.show = Some(show).into();
        let episode = episode_of(&wire, &episode_uri(2)).expect("a named episode");
        assert_eq!(episode.show_uri, None);
        assert_eq!(episode.show, "A Show", "the name is still drawn");
    }

    #[test]
    fn a_named_episode_with_a_bad_gid_keeps_the_requested_uri() {
        let mut wire = wire_episode("An episode");
        wire.set_gid(vec![1; 15]);
        let bytes = wire.write_to_bytes().expect("encoded bytes");
        let episode = episode_from(&bytes, &episode_uri(9)).expect("a named episode");
        assert_eq!(episode.uri, episode_uri(9), "the URI the answer echoed");
        assert_eq!(episode.title, "An episode", "the title is not lost");
        assert!(episode.named);

        let label = label_from_episode(&episode);
        assert_eq!(label.title, "An episode");
        assert_eq!(label.artists, "A Show");
        assert_eq!(
            label.artist_uris,
            vec![show_uri(3)],
            "the show gid is still all the link needs"
        );
    }

    #[test]
    fn a_blank_show_name_gives_neither_a_name_nor_a_link() {
        let uri = episode_uri(2);
        let mut wire = wire_episode("An episode");
        let mut show = ShowMessage::default();
        show.set_gid(gid(3));
        show.set_name(" ".to_owned());
        wire.show = Some(show).into();

        let episode = episode_of(&wire, &uri).expect("a named episode");
        assert_eq!(episode.show, "", "a blank show name normalises to none");
        assert_eq!(
            episode.show_uri.as_deref(),
            Some(show_uri(3).as_str()),
            "the show URI is still resolvable"
        );

        let label = label_from_episode(&episode);
        assert_eq!(label.artists, "");
        assert!(
            label.artist_uris.is_empty(),
            "a row with no visible show name links nowhere"
        );
    }

    #[test]
    fn a_negative_duration_is_none() {
        let uri = episode_uri(2);
        let mut wire = wire_episode("An episode");
        wire.set_duration(-1);
        assert_eq!(
            episode_of(&wire, &uri)
                .expect("a named episode")
                .duration_ms,
            None
        );

        let mut absent = wire_episode("An episode");
        absent.duration = None;
        assert_eq!(
            episode_of(&absent, &uri)
                .expect("a named episode")
                .duration_ms,
            None
        );
    }

    #[test]
    fn an_empty_name_is_no_label() {
        let uri = episode_uri(2);
        let mut empty = wire_episode("");
        assert_eq!(episode_of(&empty, &uri), None);
        empty.name = None;
        assert_eq!(episode_of(&empty, &uri), None);
    }

    #[test]
    fn episode_from_reads_encoded_bytes_and_refuses_garbage() {
        let bytes = wire_episode("An episode")
            .write_to_bytes()
            .expect("encoded bytes");
        let episode = episode_from(&bytes, &episode_uri(9)).expect("a named episode");
        assert_eq!(episode.uri, episode_uri(9));
        assert_eq!(episode.title, "An episode");
        assert_eq!(episode_from(&[0xff, 0xff], &episode_uri(9)), None);
    }

    /// A protocol show, the way `Show::get` answers one.
    fn wire_show(name: &str, publisher: &str, cover: Option<ImageGroup>) -> ShowMessage {
        let mut show = ShowMessage::default();
        show.set_gid(gid(1));
        show.set_name(name.to_owned());
        show.set_publisher(publisher.to_owned());
        if let Some(group) = cover {
            show.cover_image = Some(group).into();
        }
        show
    }

    /// The parsed show the wire's message converts to.
    fn show(name: &str, publisher: &str, cover: Option<ImageGroup>) -> Show {
        Show::try_from(&wire_show(name, publisher, cover)).expect("a valid show message")
    }

    #[test]
    fn a_show_maps_name_publisher_and_cover() {
        let card = show_card(&show(
            "A Show",
            "A Publisher",
            Some(grouped_cover(Size::DEFAULT, &[7; 20])),
        ))
        .expect("a named show");
        assert_eq!(card.name, "A Show");
        assert_eq!(card.publisher.as_deref(), Some("A Publisher"));
        assert_eq!(
            card.cover
                .as_ref()
                .and_then(|cover| cover.variants().first())
                .map(|variant| variant.url.as_str()),
            Some("https://i.scdn.co/image/0707070707070707070707070707070707070707")
        );
    }

    #[test]
    fn a_blank_publisher_is_none() {
        let card = show_card(&show("A Show", "  \n", None)).expect("a named show");
        assert_eq!(card.publisher, None);
    }

    #[test]
    fn an_empty_name_is_no_card() {
        assert_eq!(show_card(&show("", "A Publisher", None)), None);
    }

    #[test]
    fn a_show_without_a_cover_has_none() {
        let card = show_card(&show("A Show", "A Publisher", None)).expect("a named show");
        assert_eq!(card.cover, None);
    }

    #[test]
    fn an_episode_becomes_a_track_label_with_the_show_as_its_credit() {
        let episode =
            episode_of(&wire_episode("An episode"), &episode_uri(2)).expect("a named episode");
        let label = label_from_episode(&episode);
        assert_eq!(label.title, "An episode");
        assert_eq!(label.artists, "A Show");
        assert_eq!(label.artist_uris, vec![show_uri(3)]);
        assert_eq!(label.album, None);
        assert_eq!(label.album_uri, None);
        assert_eq!(label.duration_ms, Some(2_773_461));
    }

    #[test]
    fn expand_labels_over_a_track_and_an_episode_names_both() {
        let episode_uri = episode_uri(2);
        let mut found = HashMap::new();
        found.insert(
            episode_uri.clone(),
            episode_of(&wire_episode("An episode"), &episode_uri).expect("a named episode"),
        );
        let labels = merge_labels(named(&[("a", "One", "Band")]), episode_labels(found));
        let entries = expand_labels(&items(&["a", &episode_uri]), &labels);
        assert_eq!(entries[0].title, "One");
        assert_eq!(entries[0].artists, "Band");
        assert_eq!(entries[1].title, "An episode");
        assert_eq!(entries[1].artists, "A Show");
        assert_eq!(entries[1].artist_uris, vec![show_uri(3)]);
        assert!(entries.iter().all(|entry| entry.named));
    }

    #[test]
    fn a_missing_episode_falls_back_to_its_uri_unnamed() {
        let uris = owned(&[&episode_uri(2), &episode_uri(4)]);
        let mut found = HashMap::new();
        found.insert(
            episode_uri(2),
            episode_of(&wire_episode("An episode"), &episode_uri(2)).expect("a named episode"),
        );
        let episodes = expand_episodes(&uris, &found);
        assert_eq!(episodes.len(), 2);
        assert_eq!(episodes[0].title, "An episode");
        assert!(episodes[0].named);
        assert_eq!(episodes[1].uri, episode_uri(4));
        assert_eq!(episodes[1].title, episode_uri(4));
        assert!(!episodes[1].named);
        assert!(episodes[1].show.is_empty());
        assert_eq!(episodes[1].published, None);
        assert_eq!(episodes[1].duration_ms, None);
    }

    #[test]
    fn expand_episodes_draws_each_position_with_its_own_uri() {
        // The answer is found by the position's URI, so that URI is what the
        // position draws even if the value carried another one.
        let uri = episode_uri(2);
        let mut found = HashMap::new();
        found.insert(
            uri.clone(),
            Episode {
                uri: episode_uri(9),
                title: "An episode".to_owned(),
                show: "A Show".to_owned(),
                show_uri: Some(show_uri(3)),
                description: None,
                published: None,
                duration_ms: None,
                cover: None,
                named: true,
            },
        );
        let episodes = expand_episodes(&owned(&[&uri]), &found);
        assert_eq!(episodes[0].uri, uri);
        assert_eq!(episodes[0].title, "An episode");
    }

    #[test]
    fn expand_episodes_keeps_one_episode_per_repeated_position() {
        let uri = episode_uri(2);
        let uris = vec![uri.clone(), episode_uri(4), uri.clone()];
        let mut found = HashMap::new();
        found.insert(
            uri.clone(),
            episode_of(&wire_episode("An episode"), &uri).expect("a named episode"),
        );
        let episodes = expand_episodes(&uris, &found);
        assert_eq!(episodes.len(), 3, "one episode per input position");
        assert_eq!(episodes[0].uri, uri);
        assert_eq!(episodes[2].uri, uri, "the repeat is cloned, not dropped");
        assert_eq!(episodes[0].title, "An episode");
        assert_eq!(episodes[2].title, "An episode");
        assert_eq!(episodes[1].uri, episode_uri(4));
        assert!(!episodes[1].named);
        assert!(episodes[0].named && episodes[2].named);
    }

    #[test]
    fn a_failed_episode_batch_leaves_track_labels_intact() {
        // A failed batch arrives as no answers at all, like an honestly empty one.
        let merged = merge_labels(named(&[("a", "One", "Band")]), Labels::new());
        assert_eq!(merged.len(), 1);
        assert_eq!(
            merged.get("a").map(|label| label.title.as_str()),
            Some("One")
        );
    }

    #[test]
    fn parse_show_uri_accepts_a_show_and_refuses_an_album_and_garbage() {
        let uri = show_uri(1);
        let id = parse_show_uri(&uri).expect("a show uri");
        assert_eq!(
            id,
            SpotifyUri::Show {
                id: SpotifyId::from_raw(&gid(1)).expect("a 16-byte id"),
            }
        );
        assert!(
            matches!(
                parse_show_uri(&format!("spotify:album:{TRACK_ID}")),
                Err(ShowError::NotAShow)
            ),
            "an album uri names no show"
        );
        assert!(
            matches!(parse_show_uri("spotify:album:x"), Err(ShowError::NotAShow)),
            "spotify:album:x is not a show"
        );
        assert!(
            matches!(parse_show_uri("garbage"), Err(ShowError::NotAShow)),
            "garbage is not a show"
        );
    }

    #[test]
    fn needs_labels_is_false_for_an_empty_order() {
        assert!(!needs_labels(&[]));
    }

    #[test]
    fn needs_labels_is_true_for_any_show() {
        assert!(needs_labels(&owned(&[&show_uri(1)])));
    }

    #[test]
    fn an_update_id_carries_the_millisecond_and_a_counter() {
        assert_eq!(update_id(1_700_000_000_123, 0), "takofy-1700000000123-0");
        assert_ne!(
            update_id(1_700_000_000_123, 0),
            update_id(1_700_000_000_123, 1),
            "batches within one millisecond must still differ"
        );
    }

    #[test]
    fn every_client_update_id_of_a_run_is_different() {
        let first = client_update_id();
        let second = client_update_id();
        assert_ne!(first, second);
        assert!(first.starts_with("takofy-"), "{first}");
    }

    /// One batched answer: an array with `provider` and the `(uri, code)`
    /// entities, where `None` leaves the entity's header absent.
    fn status_response(
        provider: i32,
        entities: &[(&str, Option<i32>)],
    ) -> BatchedExtensionResponse {
        let mut array = EntityExtensionDataArray {
            header: MessageField::some(EntityExtensionDataArrayHeader {
                provider_error_status: provider,
                ..Default::default()
            }),
            ..Default::default()
        };
        for (uri, code) in entities {
            let header = match code {
                Some(code) => MessageField::some(EntityExtensionDataHeader {
                    status_code: *code,
                    ..Default::default()
                }),
                None => MessageField::none(),
            };
            array.extension_data.push(EntityExtensionData {
                header,
                entity_uri: (*uri).to_owned(),
                ..Default::default()
            });
        }
        BatchedExtensionResponse {
            extended_metadata: vec![array],
            ..Default::default()
        }
    }

    #[test]
    fn a_two_hundred_array_records_nothing() {
        let response = status_response(200, &[("spotify:track:a", Some(200))]);
        assert_eq!(tally_of(&response).summary(), None);
    }

    #[test]
    fn a_failing_provider_code_is_recorded() {
        let response = status_response(500, &[("spotify:track:a", Some(200))]);
        assert_eq!(
            tally_of(&response).summary().as_deref(),
            Some("metadata statuses: provider 500 x1")
        );
    }

    #[test]
    fn two_failing_entities_are_two_uris() {
        let response = status_response(
            200,
            &[
                ("spotify:track:a", Some(404)),
                ("spotify:track:b", Some(404)),
            ],
        );
        assert_eq!(
            tally_of(&response).summary().as_deref(),
            Some("metadata statuses: entities 404 x2")
        );
    }

    #[test]
    fn one_uri_twice_is_one_entity() {
        let response = status_response(
            200,
            &[
                ("spotify:track:a", Some(404)),
                ("spotify:track:a", Some(404)),
            ],
        );
        assert_eq!(
            tally_of(&response).summary().as_deref(),
            Some("metadata statuses: entities 404 x1")
        );
    }

    #[test]
    fn an_entity_with_no_header_records_nothing() {
        let response = status_response(200, &[("spotify:track:a", None)]);
        assert_eq!(tally_of(&response).summary(), None);
    }
}
