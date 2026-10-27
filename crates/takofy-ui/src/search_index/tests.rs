use takofy_model::{CoverSet, CoverUrl, TrackEntry};

use super::{Album, Artist, Band, Caps, Index, Kind, MENU, PAGE, Row, tracks_phrase};

fn entry(uri: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: uri.to_owned(),
        artists: String::new(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

fn cover(name: &str) -> CoverSet {
    CoverSet::single(
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{name}")).expect("a cover url"),
    )
}

fn on_album(mut entry: TrackEntry, name: &str, uri: &str) -> TrackEntry {
    entry.album = Some(name.to_owned());
    entry.album_uri = Some(uri.to_owned());
    entry
}

fn credited(mut entry: TrackEntry, artists: &[(&str, &str)]) -> TrackEntry {
    entry.artists = artists
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ");
    entry.artist_names = artists.iter().map(|(name, _)| (*name).to_owned()).collect();
    entry.artist_uris = artists.iter().map(|(_, uri)| (*uri).to_owned()).collect();
    entry
}

fn covered(mut entry: TrackEntry, name: &str) -> TrackEntry {
    entry.cover = Some(cover(name));
    entry
}

fn album_uris(index: &Index) -> Vec<&str> {
    index
        .albums
        .iter()
        .map(|album| album.uri.as_str())
        .collect()
}

fn artist_uris(index: &Index) -> Vec<&str> {
    index
        .artists
        .iter()
        .map(|artist| artist.uri.as_str())
        .collect()
}

#[test]
fn albums_are_deduplicated_by_uri_in_first_appearance_order() {
    let answer = [
        on_album(entry("t1"), "B", "spotify:album:b"),
        on_album(entry("t2"), "A", "spotify:album:a"),
        on_album(entry("t3"), "B again", "spotify:album:b"),
    ];
    let index = Index::derive(&answer);
    assert_eq!(
        album_uris(&index),
        vec!["spotify:album:b", "spotify:album:a"]
    );
    assert_eq!(index.albums[0].name, "B", "the first answer names it");
}

#[test]
fn an_answer_full_of_one_album_gives_one_album() {
    let answer: Vec<TrackEntry> = (1..=4)
        .map(|n| on_album(entry(&format!("t{n}")), "One", "spotify:album:one"))
        .collect();
    let index = Index::derive(&answer);
    assert_eq!(album_uris(&index), vec!["spotify:album:one"]);
    assert_eq!(index.albums[0].name, "One");
}

#[test]
fn an_unnamed_first_track_leaves_the_album_to_a_named_one() {
    let mut nameless = entry("t1");
    nameless.album_uri = Some("spotify:album:a".to_owned());
    let answer = [nameless, on_album(entry("t2"), "Named", "spotify:album:a")];
    let index = Index::derive(&answer);
    assert_eq!(album_uris(&index), vec!["spotify:album:a"]);
    assert_eq!(index.albums[0].name, "Named");
}

#[test]
fn an_album_artist_is_the_first_credits_first_artist() {
    let answer = [on_album(
        credited(
            entry("t1"),
            &[
                ("First", "spotify:artist:1"),
                ("Second", "spotify:artist:2"),
            ],
        ),
        "Record",
        "spotify:album:a",
    )];
    let index = Index::derive(&answer);
    assert_eq!(index.albums[0].artist, "First");
}

#[test]
fn an_album_artist_falls_back_to_the_joined_credit_when_no_names_are_kept() {
    let mut track = on_album(entry("t1"), "Record", "spotify:album:a");
    track.artists = "A, B".to_owned();
    let index = Index::derive(&[track]);
    assert_eq!(index.albums[0].artist, "A, B");
}

#[test]
fn a_track_without_an_album_uri_or_name_contributes_no_album() {
    let mut named_only = entry("t1");
    named_only.album = Some("Record".to_owned());
    let mut uri_only = entry("t2");
    uri_only.album_uri = Some("spotify:album:a".to_owned());
    let index = Index::derive(&[named_only, uri_only]);
    assert!(index.albums.is_empty());
}

#[test]
fn an_album_cover_is_the_first_one_the_answer_has() {
    let answer = [
        on_album(entry("t1"), "Record", "spotify:album:a"),
        on_album(covered(entry("t2"), "later"), "Record", "spotify:album:a"),
        on_album(covered(entry("t3"), "latest"), "Record", "spotify:album:a"),
    ];
    let index = Index::derive(&answer);
    assert_eq!(index.albums[0].cover, Some(cover("later")));
}

#[test]
fn a_three_credit_track_gives_three_artists() {
    let answer = [credited(
        entry("t1"),
        &[
            ("A", "spotify:artist:a"),
            ("B", "spotify:artist:b"),
            ("C", "spotify:artist:c"),
        ],
    )];
    let index = Index::derive(&answer);
    assert_eq!(
        artist_uris(&index),
        vec!["spotify:artist:a", "spotify:artist:b", "spotify:artist:c"]
    );
}

#[test]
fn artists_are_deduplicated_by_uri_in_first_appearance_order() {
    let answer = [
        credited(entry("t1"), &[("B", "spotify:artist:b")]),
        credited(entry("t2"), &[("A", "spotify:artist:a")]),
        credited(entry("t3"), &[("B", "spotify:artist:b")]),
    ];
    let index = Index::derive(&answer);
    assert_eq!(
        artist_uris(&index),
        vec!["spotify:artist:b", "spotify:artist:a"]
    );
    assert_eq!(index.artists[0].name, "B");
    assert_eq!(index.artists[0].tracks, 2);
    assert_eq!(index.artists[1].tracks, 1);
}

#[test]
fn a_track_crediting_one_uri_twice_counts_once() {
    let answer = [credited(
        entry("t1"),
        &[("A", "spotify:artist:a"), ("A again", "spotify:artist:a")],
    )];
    let index = Index::derive(&answer);
    assert_eq!(artist_uris(&index), vec!["spotify:artist:a"]);
    assert_eq!(index.artists[0].tracks, 1);
    assert_eq!(index.artists[0].name, "A", "the first credit names the row");
}

#[test]
fn the_track_count_is_the_answers_length() {
    for length in [0, 3, 7, 25] {
        let answer: Vec<TrackEntry> = (0..length).map(|n| entry(&format!("t{n}"))).collect();
        assert_eq!(Index::derive(&answer).tracks, length);
    }
}

#[test]
fn an_empty_answer_gives_an_empty_index() {
    let index = Index::derive(&[]);
    assert_eq!(index, Index::default());
}

#[test]
fn the_totals_are_the_derived_lengths_whatever_the_answer_size() {
    for length in [3, 7, 25] {
        let answer: Vec<TrackEntry> = (0..length)
            .map(|n| {
                credited(
                    on_album(
                        entry(&format!("t{n}")),
                        &format!("Record {n}"),
                        &format!("spotify:album:{n}"),
                    ),
                    &[("A", "spotify:artist:a")],
                )
            })
            .collect();
        let index = Index::derive(&answer);
        assert_eq!(index.tracks, length);
        assert_eq!(index.albums.len(), length);
        assert_eq!(index.artists.len(), 1, "one artist, counted per track");
        assert_eq!(index.artists[0].tracks, length);
    }
}

#[test]
fn a_record_cover_is_the_first_crediting_tracks_own_cover() {
    let answer = [
        credited(entry("t1"), &[("A", "spotify:artist:a")]),
        credited(covered(entry("t2"), "first"), &[("A", "spotify:artist:a")]),
        credited(covered(entry("t3"), "second"), &[("A", "spotify:artist:a")]),
    ];
    let index = Index::derive(&answer);
    assert_eq!(index.artists[0].record_cover, Some(cover("first")));
}

#[test]
fn a_track_phrase_is_singular_only_for_one() {
    assert_eq!(tracks_phrase(0), "0 tracks");
    assert_eq!(tracks_phrase(1), "1 track");
    assert_eq!(tracks_phrase(2), "2 tracks");
}

fn index(tracks: usize, albums: usize, artists: usize) -> Index {
    Index {
        tracks,
        albums: (0..albums)
            .map(|n| Album {
                uri: format!("spotify:album:{n}"),
                name: format!("Record {n}"),
                artist: "A".to_owned(),
                cover: None,
            })
            .collect(),
        artists: (0..artists)
            .map(|n| Artist {
                uri: format!("spotify:artist:{n}"),
                name: format!("Artist {n}"),
                tracks: 1,
                record_cover: None,
            })
            .collect(),
    }
}

#[test]
fn the_menu_caps_show_three_tracks_two_albums_and_two_artists_in_order() {
    let shape = index(20, 6, 4).shape(MENU);
    let bands: Vec<(Kind, usize, usize)> = shape
        .bands
        .iter()
        .map(|band| (band.kind, band.shown, band.total))
        .collect();
    assert_eq!(
        bands,
        vec![
            (Kind::Tracks, 3, 20),
            (Kind::Albums, 2, 6),
            (Kind::Artists, 2, 4),
        ]
    );
    assert!(shape.floor);
}

#[test]
fn a_band_with_nothing_in_it_is_omitted() {
    let shape = index(2, 0, 2).shape(MENU);
    let kinds: Vec<Kind> = shape.bands.iter().map(|band| band.kind).collect();
    assert_eq!(kinds, vec![Kind::Tracks, Kind::Artists]);
    assert!(!shape.floor, "nothing was hidden");
}

#[test]
fn the_floor_exists_exactly_when_a_band_hid_something() {
    assert!(!index(3, 2, 2).shape(MENU).floor, "exactly at the caps");
    assert!(!index(1, 1, 1).shape(MENU).floor);
    assert!(index(4, 2, 2).shape(MENU).floor, "one track over");
    assert!(index(0, 3, 0).shape(MENU).floor, "one album over");
    assert!(index(0, 0, 3).shape(MENU).floor, "one artist over");
}

#[test]
fn a_share_says_what_is_hidden_and_what_is_not() {
    let shape = index(20, 2, 0).shape(MENU);
    assert_eq!(shape.bands[0].share(), "3 of 20");
    assert_eq!(shape.bands[1].share(), "2", "a band showing everything");
    let single = index(1, 0, 0).shape(MENU);
    assert_eq!(single.bands[0].share(), "1");
}

#[test]
fn a_band_heading_names_its_kind() {
    let shape = index(3, 2, 2).shape(MENU);
    let headings: Vec<&str> = shape.bands.iter().map(|band| band.heading()).collect();
    assert_eq!(headings, vec!["Tracks", "Albums", "Artists"]);
}

#[test]
fn the_walk_and_the_ordinal_round_trip_for_every_shape() {
    let cases = [
        (index(2, 0, 0).shape(MENU), false),
        (index(20, 0, 0).shape(MENU), true),
        (index(3, 2, 0).shape(MENU), false),
        (index(20, 4, 0).shape(MENU), true),
        (index(3, 2, 2).shape(MENU), false),
        (index(20, 6, 5).shape(MENU), true),
    ];
    for (shape, floor) in cases {
        assert_eq!(shape.floor, floor);
        for ordinal in 0..shape.len() {
            let row = shape.row(ordinal).expect("every ordinal is a row");
            assert_eq!(shape.ordinal(row), Some(ordinal));
        }
        assert_eq!(shape.row(shape.len()), None, "past the end");
        assert_eq!(shape.ordinal(Row::Track(shape.len())), None, "past the end");
    }
}

#[test]
fn the_walk_crosses_tracks_then_albums_then_artists_then_the_floor() {
    let shape = index(20, 6, 5).shape(MENU);
    let rows: Vec<Row> = (0..shape.len()).filter_map(|n| shape.row(n)).collect();
    assert_eq!(
        rows,
        vec![
            Row::Track(0),
            Row::Track(1),
            Row::Track(2),
            Row::Album(0),
            Row::Album(1),
            Row::Artist(0),
            Row::Artist(1),
            Row::Floor,
        ]
    );
}

#[test]
fn without_a_floor_the_walk_ends_on_the_last_shown_row() {
    let shape = index(3, 2, 2).shape(MENU);
    assert_eq!(shape.row(shape.len() - 1), Some(Row::Artist(1)));
    assert_eq!(shape.ordinal(Row::Floor), None);
}

#[test]
fn a_track_row_is_an_index_into_the_whole_answer() {
    let shape = index(20, 0, 0).shape(MENU);
    assert_eq!(shape.row(2), Some(Row::Track(2)));
    assert_eq!(shape.ordinal(Row::Track(2)), Some(2));
}

#[test]
fn the_caps_are_a_parameter_not_a_property_of_the_derivation() {
    let wide = Caps {
        tracks: 10,
        albums: 10,
        artists: 10,
    };
    let shape = index(25, 12, 11).shape(wide);
    let shown: Vec<usize> = shape.rows_per_band();
    assert_eq!(shown, vec![10, 10, 10]);
    assert!(shape.floor);
    assert_eq!(shape.len(), 31);
    assert_eq!(shape.row(30), Some(Row::Floor));

    let all = Caps {
        tracks: 25,
        albums: 12,
        artists: 11,
    };
    assert!(!index(25, 12, 11).shape(all).floor);
}

#[test]
fn the_page_shows_ten_of_each_kind() {
    assert_eq!((PAGE.tracks, PAGE.albums, PAGE.artists), (10, 10, 10));
    let shape = index(25, 12, 11).shape(PAGE);
    let shares: Vec<String> = shape.bands.iter().map(Band::share).collect();
    assert_eq!(shares, ["10 of 25", "10 of 12", "10 of 11"]);
}

fn band(kind: Kind, shown: usize, total: usize) -> Band {
    Band { kind, shown, total }
}

#[test]
fn a_band_offers_the_rest_only_while_something_is_hidden() {
    assert_eq!(band(Kind::Tracks, 10, 10).more(), None);
    assert_eq!(band(Kind::Tracks, 3, 3).more(), None);
    assert_eq!(
        band(Kind::Tracks, 10, 20).more().as_deref(),
        Some("Show 10 more tracks")
    );
    assert_eq!(
        band(Kind::Albums, 10, 13).more().as_deref(),
        Some("Show 3 more albums")
    );
    assert_eq!(
        band(Kind::Artists, 10, 11).more().as_deref(),
        Some("Show 1 more artist")
    );
    assert_eq!(
        band(Kind::Tracks, 10, 11).more().as_deref(),
        Some("Show 1 more track")
    );
    assert_eq!(
        band(Kind::Albums, 10, 11).more().as_deref(),
        Some("Show 1 more album")
    );
}

#[test]
fn a_band_speaks_its_heading_and_share() {
    assert_eq!(band(Kind::Tracks, 10, 20).spoken(), "Tracks, 10 of 20");
    assert_eq!(band(Kind::Albums, 6, 6).spoken(), "Albums, 6");
}
