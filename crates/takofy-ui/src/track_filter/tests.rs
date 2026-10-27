use super::*;

fn track(title: &str, artists: &str, album: Option<&str>) -> TrackEntry {
    TrackEntry {
        uri: format!("spotify:track:{title}"),
        title: title.to_owned(),
        artists: artists.to_owned(),
        album: album.map(str::to_owned),
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

#[test]
fn title_artist_or_album_match_case_insensitively() {
    let entries = vec![
        track("Midnight City", "M83", Some("Hurry Up, We're Dreaming")),
        track("Outro", "M83", Some("Hurry Up, We're Dreaming")),
    ];

    assert_eq!(hits(&entries, "midnight"), vec![0]);
    assert_eq!(hits(&entries, "m83"), vec![0, 1]);
    assert_eq!(hits(&entries, "dreaming"), vec![0, 1]);
}

#[test]
fn non_ascii_case_folds() {
    let entries = vec![track("Été", "Hana Sō", None)];

    assert_eq!(hits(&entries, "é"), vec![0]);
    assert_eq!(hits(&entries, "ō"), vec![0]);
}

#[test]
fn the_query_is_trimmed() {
    let mut filter = TrackFilter::default();
    filter.edit("spotify:playlist:a").push_str("  midnight  ");

    assert_eq!(
        filter.needle("spotify:playlist:a").as_deref(),
        Some("midnight")
    );
}

#[test]
fn blank_or_whitespace_is_no_filter() {
    let mut filter = TrackFilter::default();
    assert_eq!(filter.needle("spotify:playlist:a"), None);

    filter.edit("spotify:playlist:a").push_str("   ");
    assert_eq!(filter.needle("spotify:playlist:a"), None);
}

#[test]
fn a_missing_album_is_not_a_match_and_not_a_panic() {
    let entries = vec![track("Outro", "M83", None)];

    assert_eq!(hits(&entries, "dreaming"), Vec::<usize>::new());
    assert_eq!(hits(&entries, "outro"), vec![0]);
}

#[test]
fn hits_are_ascending_true_indices() {
    let matching = [1, 4, 9, 12, 20, 23, 31, 38, 44, 51, 57, 60];
    let entries: Vec<TrackEntry> = (0..62)
        .map(|index| {
            if matching.contains(&index) {
                track(&format!("midnight {index}"), "M83", None)
            } else {
                track(&format!("other {index}"), "Vela Moreno", None)
            }
        })
        .collect();

    let found = hits(&entries, "midnight");

    assert_eq!(found, matching.to_vec());
    assert!(found.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn another_playlist_resets_the_text() {
    let mut filter = TrackFilter::default();
    filter.edit("spotify:playlist:a").push_str("midnight");

    let text = filter.edit("spotify:playlist:b");
    assert!(text.is_empty(), "another playlist starts unfiltered");
}

#[test]
fn take_hands_back_the_text_it_dropped() {
    let mut filter = TrackFilter::default();
    filter.edit("spotify:playlist:a").push_str("  midnight  ");

    assert_eq!(filter.take(), "  midnight  ");
    assert_eq!(filter.needle("spotify:playlist:a"), None);
    assert_eq!(filter.take(), "", "nothing left to hand back");
}

#[test]
fn needle_for_another_playlist_is_none() {
    let mut filter = TrackFilter::default();
    filter.edit("spotify:playlist:a").push_str("midnight");

    assert_eq!(filter.needle("spotify:playlist:b"), None);
}
