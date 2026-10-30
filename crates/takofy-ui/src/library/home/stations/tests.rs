use takofy_model::home::{HomeAnswer, HomeAsk, SeedKind, Station, StationSeed};
use takofy_model::{FollowedArtist, LikedTrack, PlaylistEntry, SavedAlbum, TrackEntry};

use super::*;
use crate::home::Presence;
use crate::message::{Failure, Update};

const NOW_MS: u64 = 1_700_000_000_000;

fn playlist(uri: &str) -> PlaylistEntry {
    PlaylistEntry {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        cover: None,
        composed: Vec::new(),
        description: None,
        tracks: None,
        owner: None,
        access: None,
        saved_at: None,
    }
}

fn artist(uri: &str) -> FollowedArtist {
    FollowedArtist {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        portrait: None,
        releases: None,
    }
}

fn album(uri: &str) -> SavedAlbum {
    SavedAlbum {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        artists: String::new(),
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        year: None,
        cover: None,
        tracks: None,
        saved_at: None,
    }
}

fn liked(uri: &str) -> LikedTrack {
    LikedTrack {
        uri: uri.to_owned(),
        saved_at: None,
    }
}

fn track(uri: &str) -> TrackEntry {
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

/// A library Home was opened on, with every feed behind the seeds answered.
fn settled(
    playlists: Vec<PlaylistEntry>,
    artists: Vec<FollowedArtist>,
    albums: Vec<SavedAlbum>,
    tracks: Vec<LikedTrack>,
) -> Library {
    let mut library = Library::default();
    library.go_home(NOW_MS);
    library.apply(&Update::LibraryReady);
    library.apply(&Update::Playlists(playlists));
    library.apply(&Update::FollowedArtists(artists));
    library.apply(&Update::SavedAlbums(albums));
    assert_eq!(
        library.likes_request(),
        Some(Command::LoadLikedTracks),
        "arrange: the liked read must be in flight"
    );
    library.apply(&Update::LikedTracks(tracks));
    library
}

/// A Home library whose lists answered while the label pass is still out.
fn lists_before_the_labels(tracks: Vec<LikedTrack>) -> Library {
    let mut library = Library::default();
    library.go_home(NOW_MS);
    library.apply(&Update::SignedIn);
    library.apply(&Update::LibraryReady);
    library.apply(&Update::FollowedArtists(vec![artist("spotify:artist:1")]));
    library.apply(&Update::SavedAlbums(vec![album("spotify:album:1")]));
    assert_eq!(
        library.likes_request(),
        Some(Command::LoadLikedTracks),
        "arrange: the liked read must be in flight"
    );
    library.apply(&Update::LikedTracks(tracks));
    assert_eq!(
        library.playlists,
        Load::Loading,
        "arrange: the playlists are still unlabelled"
    );
    library
}

/// The kinds the seeds a shelf asks for were drawn from.
fn asked_kinds(library: &mut Library) -> Vec<SeedKind> {
    asked_seeds(library, 4)
        .into_iter()
        .map(|seed| seed.kind)
        .collect()
}

/// The seeds a shelf asks for, in ask order.
fn asked_seeds(library: &mut Library, count: usize) -> Vec<StationSeed> {
    let mut seeds = Vec::new();
    for _ in 0..count {
        match library.stations_ask() {
            Some(Command::LoadHome(HomeAsk::Station { seed })) => seeds.push(seed),
            Some(other) => panic!("the stations shelf asked for {other:?}"),
            None => break,
        }
    }
    seeds
}

fn station(seed: &StationSeed, rows: usize) -> Station {
    Station {
        seed: seed.clone(),
        uri: format!("spotify:station:{}", seed.uri),
        title: seed.uri.clone(),
        subtitle: None,
        art: None,
        rows: (0..rows)
            .map(|at| track(&format!("spotify:track:{at}")))
            .collect(),
    }
}

fn land(library: &mut Library, seed: &StationSeed, rows: usize) {
    library.apply(&Update::Home(HomeAnswer::Station(station(seed, rows))));
}

fn four_feeds() -> Library {
    settled(
        vec![playlist("spotify:playlist:1")],
        vec![artist("spotify:artist:1")],
        vec![album("spotify:album:1")],
        vec![liked("spotify:track:1")],
    )
}

#[test]
fn nothing_is_asked_before_the_feeds_settle() {
    let mut library = Library::default();
    library.go_home(NOW_MS);
    assert_eq!(library.stations_ask(), None);

    // The rootlist alone is not enough: the seeds are drawn from four feeds.
    library.apply(&Update::Playlists(vec![playlist("spotify:playlist:1")]));
    assert_eq!(library.stations_ask(), None);
}

/// Whether any of `asks` builds a station.
fn no_station(asks: &[Command]) -> bool {
    !asks
        .iter()
        .any(|ask| matches!(ask, Command::LoadHome(HomeAsk::Station { .. })))
}

#[test]
fn seeds_wait_for_playlists_when_the_lists_land_first() {
    let mut library = lists_before_the_labels(vec![liked("spotify:track:1")]);
    assert_eq!(
        library.stations_ask(),
        None,
        "the playlist seeds have not been read yet"
    );
    assert!(!library.home.stations.chosen());
    assert_eq!(library.stations_section().presence(), Presence::Waiting);

    library.apply(&Update::Playlists(vec![playlist("spotify:playlist:1")]));
    let kinds = asked_kinds(&mut library);
    assert!(
        kinds.contains(&SeedKind::Playlist),
        "the playlists landed, so their seed is in the band: {kinds:?}"
    );
}

#[test]
fn seeds_wait_for_playlists_after_the_hold_is_spent() {
    let mut library = lists_before_the_labels(vec![liked("spotify:track:1")]);
    let start = std::time::Instant::now();

    let first = library.home_asks(start);
    assert!(no_station(&first));
    assert!(library.home_held(), "arrange: the dated feeds hold it");

    let spent = library.home_asks(start + crate::library::HOME_HOLD);
    assert!(no_station(&spent));
    assert!(!library.home.stations.chosen(), "the seeds waited");

    library.apply(&Update::Playlists(vec![playlist("spotify:playlist:1")]));
    let kinds = asked_kinds(&mut library);
    assert!(
        kinds.contains(&SeedKind::Playlist),
        "the playlists landed, so their seed is in the band: {kinds:?}"
    );
}

#[test]
fn seeds_wait_for_playlists_on_a_library_with_no_likes() {
    let mut library = lists_before_the_labels(Vec::new());

    let asks = library.home_asks(std::time::Instant::now());
    assert!(no_station(&asks));
    assert!(!library.home_held(), "nothing dated is owed");
    assert!(!library.home.stations.chosen(), "the seeds waited");

    library.apply(&Update::Playlists(vec![playlist("spotify:playlist:1")]));
    let kinds = asked_kinds(&mut library);
    assert!(
        kinds.contains(&SeedKind::Playlist),
        "the playlists landed, so their seed is in the band: {kinds:?}"
    );
}

#[test]
fn a_failed_rootlist_still_seeds_nothing() {
    let mut library = four_feeds();
    library.apply(&Update::PlaylistFailed(Failure::Load));

    assert_eq!(library.stations_ask(), None);
    assert!(!library.home.stations.chosen());
}

#[test]
fn a_settled_library_is_asked_for_one_station_of_every_kind() {
    let mut library = four_feeds();
    let seeds = asked_seeds(&mut library, 4);
    assert_eq!(seeds.len(), 4);
    assert_eq!(
        seeds.iter().map(|seed| seed.kind).collect::<Vec<_>>(),
        vec![
            SeedKind::Playlist,
            SeedKind::Artist,
            SeedKind::Album,
            SeedKind::Song
        ]
    );
    assert_eq!(library.stations_ask(), None);
}

#[test]
fn the_band_asks_for_four_stations_and_no_more() {
    let playlists: Vec<PlaylistEntry> = (0..6)
        .map(|at| playlist(&format!("spotify:playlist:{at}")))
        .collect();
    let mut library = settled(playlists, Vec::new(), Vec::new(), Vec::new());
    assert_eq!(asked_seeds(&mut library, 8).len(), 4);
}

#[test]
fn a_seed_is_never_asked_twice_in_one_open() {
    let mut library = four_feeds();
    let seeds = asked_seeds(&mut library, 4);
    let mut uris: Vec<&str> = seeds.iter().map(|seed| seed.uri.as_str()).collect();
    uris.sort_unstable();
    uris.dedup();
    assert_eq!(uris.len(), 4);
}

#[test]
fn a_retry_asks_for_the_seeds_again() {
    let mut library = four_feeds();
    assert_eq!(asked_seeds(&mut library, 4).len(), 4);
    let _ = library.retry_home(Slot::Stations);
    assert_eq!(asked_seeds(&mut library, 4).len(), 4);
}

#[test]
fn the_shelf_waits_for_the_reads_it_sent() {
    let mut library = four_feeds();
    assert_eq!(
        library.stations_section().presence(),
        Presence::Absent,
        "nothing drew and no read is out"
    );

    let seeds = asked_seeds(&mut library, 4);
    assert_eq!(library.stations_section().presence(), Presence::Waiting);

    land(&mut library, &seeds[0], 5);
    let section = library.stations_section();
    let Section::Ready(band) = section else {
        panic!("one station landed, so the band draws");
    };
    assert_eq!(band.stations.len(), 1);
}

#[test]
fn one_failed_station_leaves_the_others() {
    let mut library = four_feeds();
    let seeds = asked_seeds(&mut library, 4);
    for (at, seed) in seeds.iter().enumerate() {
        if at == 1 {
            library.apply(&Update::HomeFailed {
                ask: HomeAsk::Station { seed: seed.clone() },
                failure: Failure::Load,
            });
        } else {
            land(&mut library, seed, 4);
        }
    }
    let Section::Ready(band) = library.stations_section() else {
        panic!("three stations landed, so the band draws");
    };
    assert_eq!(band.stations.len(), 3);
}

#[test]
fn a_shelf_where_every_station_failed_names_the_failure() {
    let mut library = four_feeds();
    let seeds = asked_seeds(&mut library, 4);
    for seed in &seeds {
        library.apply(&Update::HomeFailed {
            ask: HomeAsk::Station { seed: seed.clone() },
            failure: Failure::Load,
        });
    }
    assert!(matches!(
        library.stations_section(),
        Section::Failed(Failure::Load)
    ));
}

#[test]
fn a_library_with_seeds_to_choose_from_but_none_read_is_absent() {
    let mut library = settled(Vec::new(), Vec::new(), Vec::new(), Vec::new());
    assert_eq!(library.stations_ask(), None);
    assert_eq!(library.stations_section().presence(), Presence::Absent);
}

#[test]
fn a_station_whose_rows_are_all_non_tracks_is_not_drawn() {
    let mut library = four_feeds();
    let seeds = asked_seeds(&mut library, 4);
    land(&mut library, &seeds[0], 0);
    assert!(!matches!(library.stations_section(), Section::Ready(_)));

    for seed in &seeds[1..] {
        land(&mut library, seed, 4);
    }
    let Section::Ready(band) = library.stations_section() else {
        panic!("three stations landed with rows");
    };
    assert_eq!(band.stations.len(), 3);
}

#[test]
fn a_failed_rootlist_is_the_pages_failure() {
    let mut library = four_feeds();
    library.apply(&Update::PlaylistFailed(Failure::Load));
    assert!(matches!(
        library.stations_section(),
        Section::Failed(Failure::Load)
    ));
}

#[test]
fn a_failed_rootlist_asks_for_no_station() {
    let mut library = four_feeds();
    library.apply(&Update::PlaylistFailed(Failure::Load));
    assert_eq!(
        library.stations_ask(),
        None,
        "a gated page draws no station read"
    );
}

/// A band of four stations, each with four rows, landed and waiting to be drawn.
fn a_landed_band() -> Library {
    let mut library = four_feeds();
    let seeds = asked_seeds(&mut library, 4);
    for seed in &seeds {
        land(&mut library, seed, 4);
    }
    library
}

#[test]
fn the_shown_rows_lengths_are_one_batched_read() {
    let mut library = a_landed_band();
    let Some(Command::LoadHome(HomeAsk::Labels { tracks })) = library.stations_lengths_ask() else {
        panic!("the band asks for the lengths of the rows it shows");
    };
    let band = library.home.stations.band().expect("the band landed");
    let mut shown: Vec<&str> = band
        .stations
        .iter()
        .flat_map(|station| {
            station
                .shown
                .iter()
                .filter_map(move |at| station.whole.get(*at).map(|row| row.uri.as_str()))
        })
        .collect();
    shown.sort_unstable();
    shown.dedup();
    assert_eq!(
        tracks.len(),
        shown.len(),
        "one read names every distinct row the band draws"
    );
    assert_eq!(
        library.stations_lengths_ask(),
        None,
        "the same rows are not asked for twice"
    );
}

#[test]
fn a_row_a_label_read_already_named_is_not_asked_for_again() {
    let mut library = a_landed_band();
    let band = library.home.stations.band().expect("the band landed");
    let named = band.stations[0].whole[band.stations[0].shown[0]]
        .uri
        .clone();
    let mut answer = track(&named);
    answer.duration_ms = Some(252_000);
    library.apply(&Update::SavedLabelled {
        asked: vec![named.clone()],
        named: vec![answer],
    });
    let Some(Command::LoadHome(HomeAsk::Labels { tracks })) = library.stations_lengths_ask() else {
        panic!("the rest of the rows are still owed");
    };
    assert!(!tracks.contains(&named), "a row already named is answered");
}

#[test]
fn the_lengths_read_is_spent_from_the_shelfs_own_budget() {
    let mut library = a_landed_band();
    assert!(library.stations_lengths_ask().is_some());
    assert_eq!(
        library.home.budget.left(Slot::Stations),
        0,
        "four stations and one lengths read are the shelf's whole open"
    );
}

#[test]
fn the_lengths_wait_until_every_station_has_answered() {
    let mut library = four_feeds();
    let seeds = asked_seeds(&mut library, 4);
    for seed in seeds.iter().take(2) {
        land(&mut library, seed, 4);
    }
    assert_eq!(
        library.stations_lengths_ask(),
        None,
        "one read covers the whole band, so it waits for the whole band"
    );
    for seed in seeds.iter().skip(2) {
        land(&mut library, seed, 4);
    }
    assert!(
        library.stations_lengths_ask().is_some(),
        "every station answered, so the one lengths read is due"
    );
}
