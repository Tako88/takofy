use super::super::*;
use super::test_support::*;

use librespot::core::{SpotifyId, SpotifyUri};
use std::collections::HashMap;
use takofy_model::{CoverOrigin, CoverSet, CoverUrl, PlaylistCover};

fn cover_url() -> CoverUrl {
    CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url")
}

fn cover_set() -> CoverSet {
    CoverSet::single(cover_url())
}

fn single(url: &str) -> CoverSet {
    CoverSet::single(CoverUrl::from_wire(url).expect("a cover url"))
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
        Some(PlaylistCover::chosen(cover_set()))
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
    assert_eq!(cover, PlaylistCover::derived(cover_set()));
    assert_eq!(cover.origin, CoverOrigin::Derived);
}

#[test]
fn playlist_cover_is_none_without_a_picture_or_a_known_first_track() {
    let labels = Labels::new();
    assert_eq!(playlist_cover(None, None, &labels), None);
    assert_eq!(playlist_cover(None, Some("t"), &labels), None);
}
