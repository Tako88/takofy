use takofy_model::TrackEntry;
use takofy_model::home::SeedKind;

use super::*;

/// The four station answers the live probe recorded, one per kind of seed.
const ALBUM: &str = include_str!("../fixtures/radio_stations_album.json");
const ARTIST: &str = include_str!("../fixtures/radio_stations_artist.json");
const PLAYLIST: &str = include_str!("../fixtures/radio_stations_playlist.json");
const TRACK: &str = include_str!("../fixtures/radio_stations_track.json");

fn seed(kind: SeedKind, uri: &str) -> StationSeed {
    StationSeed {
        kind,
        uri: uri.to_owned(),
    }
}

fn read(dump: &str, kind: SeedKind) -> Station {
    station(dump.as_bytes(), &seed(kind, "spotify:playlist:seed"))
        .expect("a probe dump is a station answer")
}

fn entry(uri: &str, title: &str, named: bool) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: title.to_owned(),
        artists: String::new(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named,
        cover: None,
    }
}

#[test]
fn every_probe_dump_is_a_station_with_its_own_uri() {
    for (dump, kind, prefix) in [
        (ALBUM, SeedKind::Album, "spotify:station:album:"),
        (ARTIST, SeedKind::Artist, "spotify:station:artist:"),
        (PLAYLIST, SeedKind::Playlist, "spotify:station:playlist:"),
        (TRACK, SeedKind::Song, "spotify:station:track:"),
    ] {
        let station = read(dump, kind);
        assert!(station.uri.starts_with(prefix), "{}", station.uri);
        assert_eq!(station.seed.kind, kind);
        assert_eq!(station.seed.uri, "spotify:playlist:seed");
    }
}

#[test]
fn the_album_and_track_seeds_name_a_subtitle() {
    assert_eq!(
        read(ALBUM, SeedKind::Album).subtitle.as_deref(),
        Some("Kazzer")
    );
    assert_eq!(
        read(TRACK, SeedKind::Song).subtitle.as_deref(),
        Some("Atrocity Solution")
    );
}

#[test]
fn the_artist_and_playlist_seeds_name_no_subtitle() {
    assert_eq!(read(ARTIST, SeedKind::Artist).subtitle, None);
    assert_eq!(read(PLAYLIST, SeedKind::Playlist).subtitle, None);
}

#[test]
fn a_blank_subtitle_falls_through_to_the_first_named_one() {
    let answer = br#"{"uri":"spotify:station:artist:1","subtitle":"   ",
        "subtitles":[{"name":""},{"name":" The Seed "}]}"#;
    let station = station(answer, &seed(SeedKind::Artist, "spotify:artist:1"))
        .expect("a station with subtitles is readable");
    assert_eq!(station.subtitle.as_deref(), Some("The Seed"));
}

#[test]
fn every_dump_lists_only_track_rows() {
    for dump in [ALBUM, ARTIST, PLAYLIST, TRACK] {
        let station = read(dump, SeedKind::Playlist);
        assert_eq!(station.rows.len(), 20);
        assert!(
            station
                .rows
                .iter()
                .all(|row| row.uri.starts_with("spotify:track:"))
        );
    }
}

#[test]
fn a_row_the_label_read_missed_draws_what_the_station_said() {
    let station = read(ALBUM, SeedKind::Album);
    let first = station.rows.first().expect("the dump lists rows");
    assert_eq!(first.title, "Hypocritical");
    assert_eq!(first.artists, "Methods of Mayhem");
    assert_eq!(first.album.as_deref(), Some("Methods Of Mayhem"));
    assert!(first.named);
    assert_eq!(first.artist_names, vec!["Methods of Mayhem".to_owned()]);
    assert_eq!(first.artist_uris.len(), 1);
    assert!(first.cover.is_some());
    assert!(
        takofy_model::parse(first.album_uri.as_deref().expect("the row names an album")).is_some()
    );
}

#[test]
fn a_row_with_two_credited_artists_names_both() {
    let station = read(PLAYLIST, SeedKind::Playlist);
    let row = station
        .rows
        .iter()
        .find(|row| row.title == "Fuck You")
        .expect("the dump credits two artists to one row");
    assert_eq!(row.artists, "The Bug, Warrior Queen");
    assert_eq!(
        row.artist_names,
        vec!["The Bug".to_owned(), "Warrior Queen".to_owned()]
    );
    assert_eq!(row.artist_uris.len(), 2);
}

#[test]
fn a_station_that_lists_only_non_tracks_has_no_rows() {
    let answer = br#"{"uri":"spotify:station:playlist:1","tracks":[
        {"uri":"spotify:episode:1","name":"An episode"},
        {"uri":"spotify:local:artist:album:title:1","name":"A local file"}]}"#;
    let station = station(answer, &seed(SeedKind::Playlist, "spotify:playlist:1"))
        .expect("a station with no tracks is still a station");
    assert!(station.rows.is_empty());
}

#[test]
fn a_track_listed_twice_is_two_rows() {
    let answer = br#"{"uri":"spotify:station:playlist:1","tracks":[
        {"uri":"spotify:track:1","name":"Once"},
        {"uri":"spotify:track:1","name":"Once"}]}"#;
    let station = station(answer, &seed(SeedKind::Playlist, "spotify:playlist:1"))
        .expect("a station naming one track twice is readable");
    assert_eq!(station.rows.len(), 2);
}

#[test]
fn an_answer_without_a_station_uri_is_not_a_station() {
    assert!(matches!(
        station(
            br#"{"title":"Radio","tracks":[]}"#,
            &seed(SeedKind::Song, "x")
        ),
        Err(StationError::Parse)
    ));
}

#[test]
fn an_answer_that_is_not_json_is_not_a_station() {
    for answer in [
        &b""[..],
        &b"not json"[..],
        &b"{\"uri\":\"spotify:station:1\""[..],
        &br#"{"uri": }"#[..],
    ] {
        assert!(matches!(
            station(answer, &seed(SeedKind::Song, "x")),
            Err(StationError::Parse)
        ));
    }
}

#[test]
fn an_answer_that_is_not_utf8_is_not_a_station() {
    assert!(matches!(
        station(&[0x7b, 0xff, 0x7d], &seed(SeedKind::Song, "x")),
        Err(StationError::Parse)
    ));
}

#[test]
fn a_station_without_a_track_list_has_no_rows() {
    let station = station(
        br#"{"uri":"spotify:station:artist:1","title":"Radio"}"#,
        &seed(SeedKind::Artist, "spotify:artist:1"),
    )
    .expect("a station with no list is still a station");
    assert!(station.rows.is_empty());
    assert_eq!(station.title, "Radio");
}

#[test]
fn the_json_reader_unescapes_what_the_wire_escapes() {
    let answer = br#"{"uri":"spotify:station:track:1","title":"Quote \" and \u00e9 and \\ and \ud83c\udfb5"}"#;
    let station = station(answer, &seed(SeedKind::Song, "spotify:track:1"))
        .expect("escaped text is readable");
    assert_eq!(station.title, "Quote \" and \u{e9} and \\ and \u{1f3b5}");
}

#[test]
fn a_body_with_a_broken_escape_is_not_a_station() {
    for answer in [
        &br#"{"uri":"spotify:station:1","title":"a\u00zz"}"#[..],
        &br#"{"uri":"spotify:station:1","title":"a\q"}"#[..],
        &br#"{"uri":"spotify:station:1","title":"a\ud83c"}"#[..],
    ] {
        assert!(matches!(
            station(answer, &seed(SeedKind::Song, "x")),
            Err(StationError::Parse)
        ));
    }
}

#[test]
fn the_label_read_names_the_rows_it_answers_with() {
    let fallback = vec![
        entry("spotify:track:1", "Spotify said", true),
        entry("spotify:track:2", "Spotify said", true),
    ];
    let named = vec![
        entry("spotify:track:1", "The label said", true),
        entry("spotify:track:2", "spotify:track:2", false),
    ];
    let merged = merge(fallback, named);
    assert_eq!(merged[0].title, "The label said");
    assert_eq!(merged[1].title, "Spotify said");
}

#[test]
fn a_label_read_that_answers_less_than_asked_keeps_the_rows_it_missed() {
    let fallback = vec![
        entry("spotify:track:1", "Spotify said", true),
        entry("spotify:track:2", "Spotify said", true),
    ];
    let merged = merge(
        fallback,
        vec![entry("spotify:track:1", "The label said", true)],
    );
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[1].title, "Spotify said");
}

#[test]
fn a_dump_with_a_picture_reads_it_as_art() {
    let station = read(PLAYLIST, SeedKind::Playlist);
    assert!(matches!(
        station.art,
        Some(takofy_model::home::StationArt::Mosaic(ref parts)) if parts.len() == 4
    ));
    assert!(matches!(
        read(ALBUM, SeedKind::Album).art,
        Some(takofy_model::home::StationArt::Single(_))
    ));
}

#[test]
fn an_artist_stations_neighbours_are_the_cards_it_names() {
    let cards = related(ARTIST.as_bytes()).expect("a probe dump is a station answer");

    assert_eq!(cards.len(), 5, "the probe's artist station names five");
    assert_eq!(cards[0].uri, "spotify:artist:6klB70v9fPkNc3IKDORAW7");
    assert_eq!(cards[0].name, "Maxnormal.tv");
    assert!(cards[0].named, "the answer carries the name with the URI");
    assert!(
        cards
            .iter()
            .all(|card| card.uri.starts_with("spotify:artist:")),
        "only an artist URI can be drawn as a card"
    );
}

#[test]
fn a_neighbour_with_no_artist_uri_is_dropped() {
    let answer = br#"{"uri":"spotify:station:x","related_artists":[
        {"artistName":"Kept","artistUri":"spotify:artist:1"},
        {"artistName":"Dropped","artistUri":""},
        {"artistName":"Dropped","artistUri":"spotify:show:1"}
    ]}"#;

    let cards = related(answer).expect("an answer with related_artists is read");

    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].name, "Kept");
}

#[test]
fn an_answer_with_no_neighbours_names_none() {
    let answer = br#"{"uri":"spotify:station:x"}"#;

    assert!(related(answer).expect("a station answer").is_empty());
}

#[test]
fn a_body_that_is_not_json_is_not_a_station_answer() {
    assert!(related(b"not json").is_err());
}
