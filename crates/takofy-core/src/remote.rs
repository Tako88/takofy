//! librespot-backed rootlist source and playlist labelling.
//!
//! The only file in this crate that performs librespot I/O.

use std::collections::{HashMap, HashSet};

use futures_util::StreamExt;
use librespot::core::{Session, SpotifyUri};
use librespot::metadata::{Metadata, Playlist};
use librespot::protocol::metadata::Track as TrackMessage;

use crate::context::{ContextError, Page, PageSource, page_from_proto, parse_page_json};
use crate::rootlist::{RootlistError, RootlistPage, RootlistSource, RootlistWire, parse_rootlist};
use takofy_model::{PlaylistEntry, TrackEntry};

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
            let (name, named) = playlist_label(&uri, name);
            PlaylistEntry { uri, name, named }
        })
        .buffered(8)
        .collect::<Vec<_>>()
        .await
}

/// The display name and whether it is a real name rather than the URI fallback.
fn playlist_label(uri: &str, name: Option<String>) -> (String, bool) {
    match name {
        Some(name) => (name, true),
        None => (uri.to_owned(), false),
    }
}

async fn playlist_name(session: &Session, uri: &str) -> Option<String> {
    let id = SpotifyUri::from_uri(uri).ok()?;
    Playlist::get(session, &id)
        .await
        .ok()
        .map(|playlist| playlist.name().to_owned())
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

/// What the wire says about one track: the name and artists the row already
/// draws, plus the album and length it can now draw too.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TrackLabel {
    title: String,
    artists: String,
    album: Option<String>,
    duration_ms: Option<u32>,
}

/// Labels for tracks, keyed by URI.
type Labels = HashMap<String, TrackLabel>;

/// How many distinct URIs one metadata request asks for.
///
/// The server accepted 500 without complaint, but it deduplicates within a
/// request, so that run could not report where the real ceiling is. 250 keeps
/// clear of it, and a short answer is retried in smaller pieces anyway.
const LABEL_CHUNK: usize = 250;

/// Names each track, falling back to its URI when metadata is missing.
pub(crate) async fn label_tracks(session: &Session, uris: Vec<String>) -> Vec<TrackEntry> {
    let labels = fetch_labels(session, &distinct_uris(&uris)).await;
    let unnamed = uris.iter().filter(|uri| !labels.contains_key(*uri)).count();
    if unnamed > 0 {
        log::warn!("{unnamed} of {} tracks could not be named", uris.len());
    }
    expand_labels(&uris, &labels)
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
fn expand_labels(uris: &[String], labels: &Labels) -> Vec<TrackEntry> {
    uris.iter()
        .map(|uri| match labels.get(uri) {
            Some(label) => TrackEntry {
                uri: uri.clone(),
                title: label.title.clone(),
                artists: label.artists.clone(),
                album: label.album.clone(),
                duration_ms: label.duration_ms,
                named: true,
            },
            None => TrackEntry {
                uri: uri.clone(),
                title: uri.clone(),
                artists: String::new(),
                album: None,
                duration_ms: None,
                named: false,
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
    use librespot::protocol::metadata::{Album, Artist, Track as TrackMessage};

    use super::{
        Labels, TrackLabel, distinct_uris, duration_ms, expand_labels, label_of, playlist_label,
    };

    fn owned(uris: &[&str]) -> Vec<String> {
        uris.iter().map(|uri| (*uri).to_owned()).collect()
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
        let uris = owned(&["a", "b", "a"]);
        let entries = expand_labels(&uris, &named(&[("a", "One", "Band"), ("b", "Two", "Band")]));
        let titles: Vec<&str> = entries.iter().map(|entry| entry.title.as_str()).collect();
        assert_eq!(titles, vec!["One", "Two", "One"]);
    }

    #[test]
    fn a_track_repeated_six_times_is_named_six_times_from_one_entity() {
        let uris = owned(&["a"; 6]);
        let entries = expand_labels(&uris, &named(&[("a", "One", "Band")]));
        assert_eq!(entries.len(), 6);
        assert!(entries.iter().all(|entry| entry.title == "One"));
    }

    #[test]
    fn an_unnamed_track_falls_back_to_its_uri() {
        let uris = owned(&["a", "b"]);
        let entries = expand_labels(&uris, &named(&[("a", "One", "Band")]));
        assert_eq!(entries[1].title, "b");
        assert!(entries[1].artists.is_empty());
        assert_eq!(entries[1].uri, "b");
        assert_eq!(entries[1].album, None);
        assert_eq!(entries[1].duration_ms, None);
        assert!(!entries[1].named);
    }

    #[test]
    fn a_named_track_keeps_its_uri() {
        let uris = owned(&["a"]);
        let entries = expand_labels(&uris, &named(&[("a", "One", "Band")]));
        assert_eq!(entries[0].uri, "a");
        assert_eq!(entries[0].artists, "Band");
        assert!(entries[0].named);
    }

    #[test]
    fn a_labelled_track_carries_its_album_and_length() {
        let uris = owned(&["a"]);
        let labels = labelled("a", "One", "Band", Some("Record"), Some(222_000));
        let entries = expand_labels(&uris, &labels);
        assert_eq!(entries[0].album.as_deref(), Some("Record"));
        assert_eq!(entries[0].duration_ms, Some(222_000));
        assert!(entries[0].named);
    }

    #[test]
    fn a_track_with_no_album_or_length_carries_neither() {
        let uris = owned(&["a"]);
        let entries = expand_labels(&uris, &named(&[("a", "One", "Band")]));
        assert_eq!(entries[0].album, None);
        assert_eq!(entries[0].duration_ms, None);
        assert!(entries[0].named);
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
}
