use super::super::*;
use super::test_support::*;

use librespot::core::{Session, SessionConfig, SpotifyId, SpotifyUri};
use librespot::protocol::metadata::image::Size;
use librespot::protocol::metadata::{Album, Artist, Image, ImageGroup, Track as TrackMessage};
use takofy_model::AddedAt;

fn dated(uri: &str, millis: u64) -> PageTrack {
    PageTrack {
        uri: uri.to_owned(),
        added_at: AddedAt::from_wire(&millis.to_string()),
    }
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
