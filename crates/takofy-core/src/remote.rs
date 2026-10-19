//! librespot-backed rootlist source and playlist labelling.
//!
//! The only file in this crate that performs librespot I/O.

use std::collections::{HashMap, HashSet};

use futures_util::StreamExt;
use librespot::core::{Session, SpotifyUri};
use librespot::metadata::{Metadata, Playlist};
use librespot::protocol::metadata::Track as TrackMessage;

use crate::context::{
    ContextError, Page, PageSource, PageTrack, Resolved, page_from_proto, parse_page_json,
};
use crate::cover::pick_cover;
use crate::rootlist::{RootlistError, RootlistPage, RootlistSource, RootlistWire, parse_rootlist};
use takofy_model::{CoverUrl, PlaylistEntry, TrackEntry};

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
            } = meta;
            let cover = playlist_cover(picture, first_track.as_deref(), &labels);
            let (name, named) = playlist_label(&uri, name);
            PlaylistEntry {
                uri,
                name,
                named,
                cover,
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
}

impl PlaylistMeta {
    fn empty() -> Self {
        Self {
            name: None,
            first_track: None,
            picture: None,
        }
    }
}

/// Reads one playlist's name, first track and picture in a single request.
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

/// One batched request. `None` when the request itself failed, which asking for
/// less will not fix.
async fn request_labels(session: &Session, chunk: &[String]) -> Option<Labels> {
    use librespot::protocol::extended_metadata::{
        BatchedEntityRequest, EntityRequest, ExtensionQuery,
    };
    use librespot::protocol::extension_kind::ExtensionKind;
    use protobuf::Message as _;

    let request = BatchedEntityRequest {
        entity_request: chunk
            .iter()
            .map(|uri| EntityRequest {
                entity_uri: uri.clone(),
                query: vec![ExtensionQuery {
                    extension_kind: ExtensionKind::TRACK_V4.into(),
                    ..Default::default()
                }],
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };

    let response = match session.spclient().get_extended_metadata(request).await {
        Ok(response) => response,
        Err(error) => {
            log::warn!(
                "metadata request for {} tracks failed: {error}",
                chunk.len()
            );
            return None;
        }
    };

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
    Some(TrackLabel {
        title,
        artists,
        album,
        duration_ms: duration_ms(track.duration),
        cover: track
            .album
            .as_ref()
            .and_then(|album| pick_cover(album.cover_group.as_ref())),
    })
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
        Labels, TrackLabel, context_cover, distinct_uris, duration_ms, expand_labels,
        first_track_uri, label_of, playlist_cover, playlist_label,
    };

    const TRACK_ID: &str = "4uLU6hMCjMI75M1A2tKUQC";
    const EPISODE_ID: &str = "6rqhFgbbKwnb9MLmUQDhG6";

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
}
