//! librespot-backed rootlist source and playlist labelling.
//!
//! The only file in this crate that performs librespot I/O.

use std::collections::{HashMap, HashSet};

use futures_util::StreamExt;
use http::header::{ACCEPT, CONTENT_TYPE};
use http::{HeaderMap, Method};
use librespot::core::{Session, SpotifyId, SpotifyUri};
use librespot::metadata::{Album, Artist, Metadata, Playlist};
use librespot::protocol::extended_metadata::{
    BatchedEntityRequest, BatchedExtensionResponse, EntityRequest, ExtensionQuery,
};
use librespot::protocol::extension_kind::ExtensionKind;
use librespot::protocol::metadata::Track as TrackMessage;
use protobuf::Message as _;
use takofy_model::{AlbumDetail, ArtistDetail, CoverUrl, PlaylistEntry, TrackEntry};

use crate::album::{AlbumError, detail, positions};
use crate::artist::{
    ArtistError, detail as artist_detail, release_chunks, release_uris, releases_of,
    top_track_positions,
};
use crate::collection;
use crate::context::{
    ContextError, Page, PageSource, PageTrack, Resolved, page_from_proto, parse_page_json,
};
use crate::cover::pick_cover;
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
///
/// A playlist's cover is, in order: the picture the user set, or the cover of
/// the first Track-kind item. The first-track cover needs one batched metadata
/// request, only for the playlists that have no picture.
pub(crate) async fn label_playlists(session: &Session, uris: Vec<String>) -> Vec<PlaylistEntry> {
    let metas = futures_util::stream::iter(uris.iter().cloned())
        .map(|uri| async move { playlist_meta(session, &uri).await })
        .buffered(8)
        .collect::<Vec<_>>()
        .await;

    let mut candidates: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for meta in &metas {
        if meta.picture.is_none()
            && let Some(track) = &meta.first_track
            && seen.insert(track.clone())
        {
            candidates.push(track.clone());
        }
    }
    let labels = fetch_labels(session, &candidates).await;

    uris.into_iter()
        .zip(metas)
        .map(|(uri, meta)| {
            let PlaylistMeta {
                name,
                first_track,
                picture,
                description,
                owner,
            } = meta;
            let cover = playlist_cover(picture, first_track.as_deref(), &labels);
            let (name, named) = playlist_label(&uri, name);
            PlaylistEntry {
                uri,
                name,
                named,
                cover,
                description,
                owner,
            }
        })
        .collect()
}

/// What one playlist's metadata tells the listing.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PlaylistMeta {
    name: Option<String>,
    /// The URI of the first Track-kind item, skipping episodes and local files.
    first_track: Option<String>,
    /// The picture the user set, when the playlist carries a usable one.
    picture: Option<CoverUrl>,
    /// The playlist's own description, collapsed to a line, when it has one.
    description: Option<String>,
    /// The playlist's owner as the wire names it, when it names one.
    owner: Option<String>,
}

impl PlaylistMeta {
    fn empty() -> Self {
        Self {
            name: None,
            first_track: None,
            picture: None,
            description: None,
            owner: None,
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

/// Reads one playlist's name, description, owner, first track and picture in a
/// single request.
async fn playlist_meta(session: &Session, uri: &str) -> PlaylistMeta {
    let Ok(id) = SpotifyUri::from_uri(uri) else {
        return PlaylistMeta::empty();
    };
    let Ok(playlist) = Playlist::get(session, &id).await else {
        return PlaylistMeta::empty();
    };
    PlaylistMeta {
        name: Some(playlist.name().to_owned()),
        first_track: first_track_uri(playlist.tracks()),
        picture: CoverUrl::from_file_id(&playlist.attributes.picture),
        description: non_blank(&playlist.attributes.description),
        owner: playlist_owner(&playlist.id),
    }
}

/// The URI of the first Track-kind item, skipping episodes and local files.
///
/// Reads the head only: a truncated list still begins with an intact item, and
/// the derivation never pages.
fn first_track_uri<'a>(mut uris: impl Iterator<Item = &'a SpotifyUri>) -> Option<String> {
    uris.find_map(|uri| match uri {
        SpotifyUri::Track { .. } => uri.to_uri().ok(),
        _ => None,
    })
}

/// The picture if present, else the first track's album cover.
fn playlist_cover(
    picture: Option<CoverUrl>,
    first_track: Option<&str>,
    labels: &Labels,
) -> Option<CoverUrl> {
    picture.or_else(|| {
        first_track
            .and_then(|track| labels.get(track))
            .and_then(|label| label.cover.clone())
    })
}

/// The display name and whether it is a real name rather than the URI fallback.
fn playlist_label(uri: &str, name: Option<String>) -> (String, bool) {
    match name {
        Some(name) => (name, true),
        None => (uri.to_owned(), false),
    }
}

/// Reads one album: its metadata, then the labels for its tracks.
///
/// The album is fetched by its own route rather than by walking a context: its
/// order and its numbering are the album's, not a playlist's. A URI that does not
/// name an album never reaches the network, and a request that fails hands the
/// caller the source's message so the page can show the failure with a retry.
pub(crate) async fn read_album(session: &Session, uri: &str) -> Result<AlbumDetail, AlbumError> {
    let id = SpotifyUri::from_uri(uri).map_err(|_| AlbumError::NotAnAlbum)?;
    if !matches!(id, SpotifyUri::Album { .. }) {
        return Err(AlbumError::NotAnAlbum);
    }
    let album = Album::get(session, &id)
        .await
        .map_err(|error| AlbumError::Source(error.to_string()))?;
    let tracks = label_tracks(session, positions(&album)).await;
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
    let top_tracks = label_tracks(session, top_track_positions(&artist, &country)).await;
    let followed = read_follow(session, uri).await;
    Ok((artist_detail(uri, &artist, top_tracks, releases), followed))
}

/// One release's track names, in the release's own order.
///
/// Unlike a position list this carries no added date: a release is a record, not
/// a membership. A URI that cannot be labelled falls back to itself, exactly as
/// a playlist row does.
pub(crate) async fn read_release_tracks(session: &Session, uris: &[String]) -> Vec<TrackEntry> {
    let positions = uris
        .iter()
        .map(|uri| PageTrack {
            uri: uri.clone(),
            added_at: None,
        })
        .collect();
    label_tracks(session, positions).await
}

/// Whether the artist is in the user's followed set, or `None` when the probe
/// did not answer.
async fn read_follow(session: &Session, uri: &str) -> Option<bool> {
    let headers = collection_headers()?;
    let username = session.username();
    let body = collection::contains_body(&username, &[uri]);
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
fn context_cover(metadata: &HashMap<String, String>) -> Option<CoverUrl> {
    ["image_url", "header_image_url_desktop"]
        .iter()
        .filter_map(|key| metadata.get(*key))
        .find_map(|value| CoverUrl::from_wire(value))
}

/// What the wire says about one track: the name and artists the row already
/// draws, plus the album and length it can now draw too.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TrackLabel {
    title: String,
    artists: String,
    album: Option<String>,
    album_uri: Option<String>,
    artist_uris: Vec<String>,
    duration_ms: Option<u32>,
    cover: Option<CoverUrl>,
}

/// Labels for tracks, keyed by URI.
type Labels = HashMap<String, TrackLabel>;

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
pub(crate) async fn label_tracks(session: &Session, items: Vec<PageTrack>) -> Vec<TrackEntry> {
    let uris: Vec<String> = items.iter().map(|item| item.uri.clone()).collect();
    let labels = fetch_labels(session, &distinct_uris(&uris)).await;
    let unnamed = uris.iter().filter(|uri| !labels.contains_key(*uri)).count();
    if unnamed > 0 {
        log::warn!("{unnamed} of {} tracks could not be named", uris.len());
    }
    expand_labels(&items, &labels)
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
                added_at: item.added_at,
                duration_ms: None,
                named: false,
                cover: None,
            },
        })
        .collect()
}

/// Fetches names for distinct URIs, splitting a chunk whenever the server
/// answers only part of it.
async fn fetch_labels(session: &Session, distinct: &[String]) -> Labels {
    let mut labels = Labels::new();
    let mut queue: Vec<Vec<String>> = distinct
        .chunks(LABEL_CHUNK)
        .map(<[String]>::to_vec)
        .collect();
    queue.reverse();
    while let Some(chunk) = queue.pop() {
        let Some(found) = request_labels(session, &chunk).await else {
            continue;
        };
        let answered = !found.is_empty();
        let mut missing: Vec<String> = chunk
            .iter()
            .filter(|uri| !found.contains_key(*uri))
            .cloned()
            .collect();
        labels.extend(found);
        if missing.is_empty() || !answered || missing.len() == 1 {
            // Nothing arrived at all means the provider refused the chunk rather
            // than truncated it, and retrying one URI at a time is the request
            // storm this exists to avoid. A single unanswered URI is simply
            // unknown. Either way the caller falls back to the URI.
            continue;
        }
        log::debug!(
            "{} of {} tracks came back; retrying the rest in smaller requests",
            chunk.len() - missing.len(),
            chunk.len()
        );
        let right = missing.split_off(missing.len() / 2);
        queue.push(right);
        queue.push(missing);
    }
    labels
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

/// One batched request for track labels. `None` when the request itself failed,
/// which asking for less will not fix.
async fn request_labels(session: &Session, chunk: &[String]) -> Option<Labels> {
    let response = batched_request(session, chunk, ExtensionKind::TRACK_V4).await?;

    let mut labels = Labels::new();
    for array in &response.extended_metadata {
        if array.header.provider_error_status != 200 {
            log::warn!(
                "metadata provider reported status {}",
                array.header.provider_error_status
            );
        }
        for data in &array.extension_data {
            let Some(any) = data.extension_data.as_ref() else {
                continue;
            };
            let Ok(track) = TrackMessage::parse_from_bytes(&any.value) else {
                continue;
            };
            if let Some(label) = label_of(&track) {
                labels.insert(data.entity_uri.clone(), label);
            }
        }
    }
    Some(labels)
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
    let artist_uris = track
        .artist
        .iter()
        .map_while(|artist| {
            artist
                .name
                .as_deref()
                .filter(|name| !name.is_empty())
                .and_then(|_| uri_of(artist.gid.as_deref(), |id| SpotifyUri::Artist { id }))
        })
        .collect();
    Some(TrackLabel {
        title,
        artists,
        album,
        album_uri,
        artist_uris,
        duration_ms: duration_ms(track.duration),
        cover: track
            .album
            .as_ref()
            .and_then(|album| pick_cover(album.cover_group.as_ref())),
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

    use librespot::core::{SpotifyId, SpotifyUri};
    use librespot::protocol::metadata::image::Size;
    use librespot::protocol::metadata::{Album, Artist, Image, ImageGroup, Track as TrackMessage};
    use takofy_model::{AddedAt, CoverUrl};

    use crate::context::PageTrack;

    use super::{
        Labels, PlaylistMeta, TrackLabel, context_cover, distinct_uris, duration_ms, expand_labels,
        first_track_uri, label_of, non_blank, playlist_cover, playlist_label, playlist_owner,
    };

    const TRACK_ID: &str = "4uLU6hMCjMI75M1A2tKUQC";
    const EPISODE_ID: &str = "6rqhFgbbKwnb9MLmUQDhG6";
    const PLAYLIST_ID: &str = "37i9dQZF1DXcBWIGoYBM5M";

    fn cover_url() -> CoverUrl {
        CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url")
    }

    fn first_track() -> SpotifyUri {
        SpotifyUri::Track {
            id: SpotifyId::from_base62(TRACK_ID).expect("a track id"),
        }
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
                duration_ms: None,
                cover: None,
            },
        );
        let entries = expand_labels(&items(&["a", "b"]), &labels);
        assert_eq!(entries[0].album_uri.as_deref(), Some("spotify:album:x"));
        assert_eq!(entries[0].artist_uris, vec!["spotify:artist:y".to_owned()]);
        assert_eq!(entries[1].album_uri, None);
        assert!(entries[1].artist_uris.is_empty());
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
            cover
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
            context_cover(&metadata).map(|cover| cover.as_str().to_owned()),
            Some("https://i.scdn.co/image/a".to_owned())
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
            context_cover(&metadata).map(|cover| cover.as_str().to_owned()),
            Some("https://i.scdn.co/image/b".to_owned())
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
            context_cover(&metadata).map(|cover| cover.as_str().to_owned()),
            Some("https://pickasso.spotifycdn.com/image/b".to_owned())
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
    fn first_track_uri_skips_episodes_and_locals() {
        let uris = [episode(), local(), first_track()];
        assert_eq!(
            first_track_uri(uris.iter()),
            Some(format!("spotify:track:{TRACK_ID}"))
        );
    }

    #[test]
    fn first_track_uri_reads_the_head_only() {
        let second = SpotifyUri::Track {
            id: SpotifyId::from_base62("6rqhFgbbKwnb9MLmUQDhG7").expect("a track id"),
        };
        let uris = [first_track(), second];
        assert_eq!(
            first_track_uri(uris.iter()),
            Some(format!("spotify:track:{TRACK_ID}"))
        );
    }

    #[test]
    fn first_track_uri_is_none_for_an_empty_or_episode_only_list() {
        let empty: Vec<SpotifyUri> = Vec::new();
        assert_eq!(first_track_uri(empty.iter()), None);
        let episodes = [episode()];
        assert_eq!(first_track_uri(episodes.iter()), None);
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
                duration_ms: None,
                cover: Some(cover_url()),
            },
        );
        assert_eq!(
            playlist_cover(Some(cover_url()), Some("t"), &labels),
            Some(cover_url())
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
                duration_ms: None,
                cover: Some(cover_url()),
            },
        );
        assert_eq!(playlist_cover(None, Some("t"), &labels), Some(cover_url()));
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
    fn empty_playlist_meta_has_no_description_or_owner() {
        let meta = PlaylistMeta::empty();
        assert_eq!(meta.description, None);
        assert_eq!(meta.owner, None);
    }
}
