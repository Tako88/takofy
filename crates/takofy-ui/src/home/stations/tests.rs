use takofy_model::home::{HomeAnswer, HomeAsk, SeedKind, Station, StationSeed};
use takofy_model::{CoverSet, CoverUrl, TrackEntry};

use crate::message::Failure;

use super::*;

fn seed() -> Seed {
    Seed::for_open(1_700_000_000_000, 1)
}

fn another() -> Seed {
    Seed::for_open(1_700_000_000_000, 2)
}

fn uris(uris: &[&str]) -> Vec<String> {
    uris.iter().map(|uri| (*uri).to_owned()).collect()
}

fn seed_of(kind: SeedKind, uri: &str) -> StationSeed {
    StationSeed {
        kind,
        uri: uri.to_owned(),
    }
}

fn row(uri: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: uri.to_owned(),
        artists: String::new(),
        album: None,
        album_uri: Some("spotify:album:1".to_owned()),
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

fn rows(count: usize) -> Vec<TrackEntry> {
    (0..count)
        .map(|index| row(&format!("spotify:track:{index}")))
        .collect()
}

fn station(seed: &StationSeed, uri: &str, rows: Vec<TrackEntry>) -> Station {
    Station {
        seed: seed.clone(),
        uri: uri.to_owned(),
        title: "Radio".to_owned(),
        subtitle: None,
        art: None,
        rows,
    }
}

fn landed(seed: &StationSeed) -> HomeAnswer {
    HomeAnswer::Station(station(seed, "spotify:station:1", rows(5)))
}

fn four_seeds() -> Vec<StationSeed> {
    vec![
        seed_of(SeedKind::Playlist, "spotify:playlist:1"),
        seed_of(SeedKind::Artist, "spotify:artist:1"),
        seed_of(SeedKind::Album, "spotify:album:1"),
        seed_of(SeedKind::Song, "spotify:track:1"),
    ]
}

/// A state that has chosen `seeds` this open.
fn opened(seeds: Vec<StationSeed>) -> State {
    let mut state = State::default();
    state.open(seed());
    state.choose(seeds);
    state
}

/// A state whose every seed has been asked for.
fn asked(seeds: Vec<StationSeed>) -> State {
    let mut state = opened(seeds);
    while let Some(seed) = state.next().cloned() {
        state.asked(&seed);
    }
    state
}

fn kinds(seeds: &[StationSeed]) -> Vec<SeedKind> {
    seeds.iter().map(|seed| seed.kind).collect()
}

/// The bytes a hex string spells, so a test can name the picture it expects.
fn hex(id: &str) -> Vec<u8> {
    (0..id.len() / 2)
        .map(|at| u8::from_str_radix(&id[at * 2..at * 2 + 2], 16).expect("the test's id is hex"))
        .collect()
}

#[test]
fn one_kind_is_seeded_once_in_the_pages_order() {
    let playlists = uris(&["spotify:playlist:1"]);
    let artists = uris(&["spotify:artist:1"]);
    let albums = uris(&["spotify:album:1"]);
    let songs = uris(&["spotify:track:1"]);
    let seeds = pick_seeds(
        &Candidates {
            playlists: &playlists,
            artists: &artists,
            albums: &albums,
            songs: &songs,
        },
        seed(),
    );
    assert_eq!(
        kinds(&seeds),
        vec![
            SeedKind::Playlist,
            SeedKind::Artist,
            SeedKind::Album,
            SeedKind::Song
        ]
    );
    assert_eq!(seeds[0].uri, "spotify:playlist:1");
    assert_eq!(seeds[1].uri, "spotify:artist:1");
    assert_eq!(seeds[2].uri, "spotify:album:1");
    assert_eq!(seeds[3].uri, "spotify:track:1");
}

#[test]
fn a_second_seed_is_taken_round_robin_from_the_kinds_that_have_one() {
    let playlists = uris(&["spotify:playlist:1", "spotify:playlist:2"]);
    let artists = uris(&["spotify:artist:1", "spotify:artist:2"]);
    let empty: Vec<String> = Vec::new();
    let seeds = pick_seeds(
        &Candidates {
            playlists: &playlists,
            artists: &artists,
            albums: &empty,
            songs: &empty,
        },
        seed(),
    );
    assert_eq!(
        kinds(&seeds),
        vec![
            SeedKind::Playlist,
            SeedKind::Artist,
            SeedKind::Playlist,
            SeedKind::Artist
        ]
    );
    assert_eq!(seeds.len(), STATIONS);
}

#[test]
fn a_kind_with_spares_fills_the_band_when_the_others_run_out() {
    let playlists = uris(&["spotify:playlist:1"]);
    let artists = uris(&["spotify:artist:1", "spotify:artist:2", "spotify:artist:3"]);
    let empty: Vec<String> = Vec::new();
    let seeds = pick_seeds(
        &Candidates {
            playlists: &playlists,
            artists: &artists,
            albums: &empty,
            songs: &empty,
        },
        seed(),
    );
    assert_eq!(seeds.len(), STATIONS);
    assert_eq!(
        kinds(&seeds),
        vec![
            SeedKind::Playlist,
            SeedKind::Artist,
            SeedKind::Artist,
            SeedKind::Artist
        ]
    );
    assert_eq!(seeds[0].uri, "spotify:playlist:1");
    let mut artists_drawn: Vec<&str> = seeds[1..].iter().map(|seed| seed.uri.as_str()).collect();
    artists_drawn.sort_unstable();
    assert_eq!(
        artists_drawn,
        vec!["spotify:artist:1", "spotify:artist:2", "spotify:artist:3"]
    );
}

#[test]
fn seeds_run_out_before_the_band_is_full() {
    let playlists = uris(&["spotify:playlist:1"]);
    let artists = uris(&["spotify:artist:1"]);
    let empty: Vec<String> = Vec::new();
    let seeds = pick_seeds(
        &Candidates {
            playlists: &playlists,
            artists: &artists,
            albums: &empty,
            songs: &empty,
        },
        seed(),
    );
    assert_eq!(seeds.len(), 2);
}

#[test]
fn an_empty_library_seeds_nothing() {
    let empty: Vec<String> = Vec::new();
    assert!(pick_seeds(&Candidates::default(), seed()).is_empty());
    assert!(
        pick_seeds(
            &Candidates {
                playlists: &empty,
                artists: &empty,
                albums: &empty,
                songs: &empty,
            },
            seed()
        )
        .is_empty()
    );
}

#[test]
fn no_uri_is_a_station_twice() {
    let repeated = uris(&[
        "spotify:playlist:1",
        "spotify:playlist:1",
        "spotify:playlist:1",
    ]);
    let songs = uris(&["spotify:playlist:1"]);
    let empty: Vec<String> = Vec::new();
    let seeds = pick_seeds(
        &Candidates {
            playlists: &repeated,
            artists: &empty,
            albums: &empty,
            songs: &songs,
        },
        seed(),
    );
    assert_eq!(seeds.len(), 1);
    assert_eq!(seeds[0].uri, "spotify:playlist:1");
}

#[test]
fn two_playlists_with_one_title_are_two_candidates() {
    // Titles are not part of a candidate: the URI is the identity, so two
    // playlists called the same thing are two stations.
    let playlists = uris(&["spotify:playlist:a", "spotify:playlist:b"]);
    let empty: Vec<String> = Vec::new();
    let seeds = pick_seeds(
        &Candidates {
            playlists: &playlists,
            artists: &empty,
            albums: &empty,
            songs: &empty,
        },
        seed(),
    );
    assert_eq!(seeds.len(), 2);
    assert_ne!(seeds[0].uri, seeds[1].uri);
}

#[test]
fn a_held_seed_pins_the_seeds() {
    let playlists = uris(&[
        "spotify:playlist:1",
        "spotify:playlist:2",
        "spotify:playlist:3",
    ]);
    let albums = uris(&["spotify:album:1", "spotify:album:2"]);
    let empty: Vec<String> = Vec::new();
    let candidates = Candidates {
        playlists: &playlists,
        artists: &empty,
        albums: &albums,
        songs: &empty,
    };
    assert_eq!(
        pick_seeds(&candidates, seed()),
        pick_seeds(&candidates, seed())
    );
    assert_ne!(
        pick_seeds(&candidates, seed()),
        pick_seeds(&candidates, another())
    );
}

#[test]
fn three_of_five_rows_are_shown_ascending_without_repeats() {
    let rows = rows(5);
    let shown = shown_rows(&rows, seed());
    assert_eq!(shown.len(), ROWS);
    let mut sorted = shown.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, shown, "the rows are drawn in order");
    let mut distinct = shown.clone();
    distinct.dedup();
    assert_eq!(distinct.len(), ROWS);
    assert!(shown.iter().all(|at| *at < rows.len()));
}

#[test]
fn a_station_shorter_than_the_band_shows_every_row() {
    assert_eq!(shown_rows(&rows(2), seed()), vec![0, 1]);
    assert_eq!(shown_rows(&rows(ROWS), seed()), vec![0, 1, 2]);
}

#[test]
fn no_rows_show_nothing() {
    assert!(shown_rows(&[], seed()).is_empty());
}

#[test]
fn a_held_seed_pins_the_shown_rows() {
    let rows = rows(12);
    assert_eq!(shown_rows(&rows, seed()), shown_rows(&rows, seed()));
    assert_ne!(shown_rows(&rows, seed()), shown_rows(&rows, another()));
}

#[test]
fn every_kind_names_the_word_its_head_draws() {
    assert_eq!(kind_label(SeedKind::Playlist), "Playlist");
    assert_eq!(kind_label(SeedKind::Artist), "Artist");
    assert_eq!(kind_label(SeedKind::Album), "Album");
    assert_eq!(kind_label(SeedKind::Song), "Song");
}

#[test]
fn a_station_with_no_rows_is_not_drawn() {
    let seeds = four_seeds();
    let mut state = asked(seeds.clone());
    state.land(&HomeAnswer::Station(station(
        &seeds[0],
        "spotify:station:empty",
        Vec::new(),
    )));
    assert!(state.band().is_none());
    assert!(!state.answered(), "the other three reads are still out");

    state.land(&landed(&seeds[1]));
    let band = state.band().expect("one station landed with rows");
    assert_eq!(band.stations.len(), 1);
    assert_eq!(band.stations[0].uri, "spotify:station:1");
}

#[test]
fn a_stale_answer_is_ignored() {
    let seeds = four_seeds();
    let mut state = asked(seeds.clone());
    state.land(&landed(&seed_of(
        SeedKind::Playlist,
        "spotify:playlist:elsewhere",
    )));
    assert!(state.band().is_none());

    // A new open has asked for nothing, so the last open's answer is not drawn.
    let mut state = opened(seeds.clone());
    state.land(&landed(&seeds[0]));
    assert!(state.band().is_none());
}

#[test]
fn one_failed_station_leaves_the_others() {
    let seeds = four_seeds();
    let mut state = asked(seeds.clone());
    state.fail(
        &HomeAsk::Station {
            seed: seeds[2].clone(),
        },
        Failure::Load,
    );
    for seed in [&seeds[0], &seeds[1], &seeds[3]] {
        state.land(&landed(seed));
    }
    let band = state.band().expect("three of four stations landed");
    assert_eq!(band.stations.len(), 3);
    assert_eq!(state.failure(), Some(Failure::Load));
    assert!(state.answered());
}

#[test]
fn a_failure_for_a_seed_this_open_did_not_ask_is_ignored() {
    let seeds = four_seeds();
    let mut state = opened(seeds.clone());
    state.fail(
        &HomeAsk::Station {
            seed: seeds[0].clone(),
        },
        Failure::Load,
    );
    assert_eq!(state.failure(), None);
}

#[test]
fn a_band_that_drew_nothing_names_the_failure() {
    let seeds = four_seeds();
    let mut state = asked(seeds.clone());
    for seed in &seeds {
        state.fail(&HomeAsk::Station { seed: seed.clone() }, Failure::Load);
    }
    assert!(state.band().is_none());
    assert_eq!(state.failure(), Some(Failure::Load));
}

#[test]
fn each_station_is_asked_once_and_the_next_one_follows() {
    let seeds = four_seeds();
    let mut state = opened(seeds.clone());
    for seed in &seeds {
        assert_eq!(state.next(), Some(seed));
        state.asked(seed);
    }
    assert_eq!(state.next(), None);
    assert!(!state.answered(), "asked for is not answered");
}

#[test]
fn a_forgotten_shelf_asks_again() {
    let seeds = four_seeds();
    let mut state = asked(seeds.clone());
    state.land(&landed(&seeds[0]));
    state.forget();
    assert!(!state.chosen());
    assert!(!state.answered());
    assert_eq!(state.failure(), None);
    assert!(state.band().is_none());
}

#[test]
fn the_band_names_each_stations_kind_title_and_picture() {
    let seeds = four_seeds();
    let mut state = asked(seeds.clone());
    let id = "ab67616d0000b273810dd84a63f327f269b14b42";
    let art = CoverUrl::from_file_id(&hex(id)).map(|url| StationArt::Single(CoverSet::single(url)));
    let mut drawn = station(&seeds[1], "spotify:station:art", rows(4));
    drawn.title = "Seeded by Die Antwoord".to_owned();
    drawn.art = art.clone();
    state.land(&HomeAnswer::Station(drawn));

    let band = state.band().expect("one station landed");
    let shown = band.stations.first().expect("the band holds it");
    assert_eq!(shown.kind, "Artist");
    assert_eq!(shown.head, "Seeded by Die Antwoord");
    assert_eq!(shown.art, art.as_ref());
    assert_eq!(shown.uri, "spotify:station:art");
    assert_eq!(shown.shown.len(), ROWS);
    assert_eq!(shown.whole.len(), 4);
}

#[test]
fn the_band_draws_the_stations_in_seed_order() {
    let seeds = four_seeds();
    let mut state = asked(seeds.clone());
    for (at, seed) in seeds.iter().enumerate() {
        state.land(&HomeAnswer::Station(station(
            seed,
            &format!("spotify:station:{at}"),
            rows(3),
        )));
    }
    let band = state.band().expect("four stations landed");
    let uris: Vec<&str> = band.stations.iter().map(|shown| shown.uri).collect();
    assert_eq!(
        uris,
        vec![
            "spotify:station:0",
            "spotify:station:1",
            "spotify:station:2",
            "spotify:station:3"
        ]
    );
}
