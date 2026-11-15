use std::time::Instant;

use takofy_model::FollowedArtist;
use takofy_model::home::{ArtistReleases, Day, HomeAnswer, HomeAsk, NewRelease, ReleaseDate};

use super::super::Library;
use crate::home::Section;
use crate::home::releases::{DRAWN, List, SHELF_ARTISTS, WINDOW_DAYS};
use crate::library::Load;
use crate::message::{Command, Failure, Update};

const NOW_MS: u64 = 1_700_000_000_000;
const TODAY: i32 = 20_000;

fn day() -> Day {
    Day::from_days(TODAY)
}

fn artist_uri(at: usize) -> String {
    format!("spotify:artist:{at}")
}

fn followed(count: usize) -> Vec<FollowedArtist> {
    (0..count)
        .map(|at| FollowedArtist {
            uri: artist_uri(at),
            name: format!("Artist {at}"),
            named: true,
            portrait: None,
            releases: None,
            followed_at: None,
        })
        .collect()
}

fn release(uri: &str, at: Day) -> NewRelease {
    NewRelease {
        artist: "Vela Moreno".to_owned(),
        uri: uri.to_owned(),
        name: uri.to_owned(),
        date: ReleaseDate {
            year: 2025,
            month: Some(10),
            day: Some(14),
        },
        day: at,
        cover: None,
    }
}

fn releases(artist: &str, list: Vec<NewRelease>) -> ArtistReleases {
    ArtistReleases {
        artist: artist.to_owned(),
        releases: list,
    }
}

/// A Home library whose followed list has answered with `artists`.
fn library(artists: Vec<FollowedArtist>) -> Library {
    let mut library = Library::starting_home(NOW_MS);
    library.set_local_day(day());
    library.apply(&Update::FollowedArtists(artists));
    library
}

/// The artists and the floor the one release read names.
fn ask(library: &mut Library) -> (Vec<String>, Day) {
    match library.releases_ask() {
        Some(Command::LoadHome(HomeAsk::Releases { artists, since })) => (artists, since),
        other => panic!("the shelf owes one release read, got {other:?}"),
    }
}

fn answer(library: &mut Library, asked: &[String], found: Vec<ArtistReleases>) {
    library.apply(&Update::Home(HomeAnswer::Releases {
        asked: asked.to_vec(),
        found,
    }));
}

fn fail(library: &mut Library, artists: &[String], failure: Failure) {
    library.apply(&Update::HomeFailed {
        ask: HomeAsk::Releases {
            artists: artists.to_vec(),
            since: Day::from_days(TODAY - WINDOW_DAYS),
        },
        failure,
    });
}

/// The URIs the section draws, when it draws any.
fn drawn(section: &Section<List<'_>>) -> Vec<String> {
    match section {
        Section::Ready(list) => list
            .cards
            .iter()
            .map(|card| card.release.uri.clone())
            .collect(),
        _ => Vec::new(),
    }
}

#[test]
fn nothing_is_asked_off_home() {
    let mut library = Library::default();
    library.set_local_day(day());
    library.apply(&Update::FollowedArtists(followed(2)));

    assert!(
        library.home_asks(Instant::now()).is_empty(),
        "a page that is not Home asked for its shelves"
    );
}

#[test]
fn nothing_is_asked_before_the_day_is_latched() {
    let mut library = Library::starting_home(NOW_MS);
    library.apply(&Update::FollowedArtists(followed(2)));

    assert_eq!(library.releases_ask(), None);
    assert!(
        matches!(library.releases_section(), Section::Absent),
        "a shelf with no day has no window to make"
    );
}

#[test]
fn nothing_is_asked_before_the_followed_list_answers() {
    let mut library = Library::starting_home(NOW_MS);
    library.set_local_day(day());

    assert_eq!(library.releases_ask(), None);
    assert!(matches!(library.releases_section(), Section::Absent));

    library.followed_artists = Load::Loading;
    assert!(
        matches!(library.releases_section(), Section::Waiting),
        "a list still loading is a shelf still coming"
    );
}

#[test]
fn one_ask_names_the_first_sixty_from_the_floors_day() {
    let mut library = library(followed(70));

    let (artists, since) = ask(&mut library);

    assert_eq!(artists.len(), SHELF_ARTISTS, "the cap was not applied");
    let expected: Vec<String> = (0..SHELF_ARTISTS).map(artist_uri).collect();
    assert_eq!(artists, expected, "the ask left the followed order");
    assert_eq!(since.days(), TODAY - WINDOW_DAYS, "the ask left the floor");
    assert_eq!(library.releases_ask(), None, "one open sends one read");
}

#[test]
fn the_shelf_waits_then_draws_what_landed() {
    let mut library = library(followed(2));
    let (artists, _) = ask(&mut library);

    assert!(
        matches!(library.releases_section(), Section::Waiting),
        "the shelf drew before its read answered"
    );

    answer(
        &mut library,
        &artists,
        vec![releases(
            &artists[0],
            vec![release("spotify:album:a", day())],
        )],
    );

    assert_eq!(drawn(&library.releases_section()), vec!["spotify:album:a"]);
}

#[test]
fn a_window_with_nothing_in_it_is_absent() {
    let mut library = library(followed(2));
    let (artists, _) = ask(&mut library);
    answer(&mut library, &artists, Vec::new());

    assert!(
        matches!(library.releases_section(), Section::Absent),
        "an answered read with nothing new is not a shelf still coming"
    );
}

#[test]
fn a_failed_read_draws_nothing_and_asks_no_more() {
    let mut library = library(followed(2));
    let (artists, _) = ask(&mut library);

    fail(&mut library, &artists, Failure::Load);

    assert!(
        matches!(library.releases_section(), Section::Absent),
        "a failure draws nothing"
    );
    assert_eq!(library.releases_ask(), None, "the failed open asked again");
}

#[test]
fn the_reload_interlude_keeps_the_same_cards_drawn() {
    let mut library = library(followed(2));
    let (artists, _) = ask(&mut library);
    answer(
        &mut library,
        &artists,
        vec![
            releases(&artists[0], vec![release("spotify:album:a", day())]),
            releases(&artists[1], vec![release("spotify:album:b", day())]),
        ],
    );
    let before = drawn(&library.releases_section());
    assert_eq!(before.len(), 2, "arrange: the shelf has both cards");

    // The reload a follow causes: the list is asked again, so it is unready
    // while the shelf is still on screen.
    for unready in [Load::Idle, Load::Loading] {
        library.followed_artists = unready;
        let section = library.releases_section();
        assert!(
            !matches!(section, Section::Absent),
            "the shelf blinked through the reload"
        );
        assert_eq!(
            drawn(&section),
            before,
            "the shelf lost a card through the reload"
        );
    }
}

#[test]
fn an_unready_list_with_nothing_held_is_absent() {
    let mut library = Library::starting_home(NOW_MS);
    library.set_local_day(day());
    library.followed_artists = Load::Idle;

    assert!(matches!(library.releases_section(), Section::Absent));
}

#[test]
fn a_second_open_the_same_day_asks_for_nothing() {
    let mut library = library(followed(2));
    let (artists, _) = ask(&mut library);
    answer(
        &mut library,
        &artists,
        vec![
            releases(&artists[0], vec![release("spotify:album:a", day())]),
            releases(&artists[1], Vec::new()),
        ],
    );

    library.home.open(NOW_MS);

    assert_eq!(
        library.releases_ask(),
        None,
        "the same day's cache was lost"
    );
    assert_eq!(
        drawn(&library.releases_section()),
        vec!["spotify:album:a"],
        "the second open dropped what it held"
    );
}

#[test]
fn following_one_more_artist_asks_only_for_that_one() {
    let mut library = library(followed(2));
    let (artists, _) = ask(&mut library);
    answer(
        &mut library,
        &artists,
        artists
            .iter()
            .map(|uri| releases(uri, Vec::new()))
            .collect(),
    );
    library.home.open(NOW_MS);

    library.apply(&Update::FollowedArtists(followed(3)));
    let (again, _) = ask(&mut library);

    assert_eq!(again, vec![artist_uri(2)], "the whole shelf was re-read");
}

#[test]
fn a_new_day_clears_the_shelf_and_asks_again() {
    let mut library = library(followed(2));
    let (artists, _) = ask(&mut library);
    answer(
        &mut library,
        &artists,
        vec![releases(
            &artists[0],
            vec![release("spotify:album:a", day())],
        )],
    );

    library.set_local_day(Day::from_days(TODAY + 1));

    assert!(
        matches!(library.releases_section(), Section::Waiting),
        "the new day kept drawing the old day's releases"
    );
    let (again, since) = ask(&mut library);
    assert_eq!(again.len(), 2);
    assert_eq!(since.days(), TODAY + 1 - WINDOW_DAYS);
}

#[test]
fn turning_discovery_off_changes_nothing() {
    let mut library = library(followed(2));
    library.set_discovery_enabled(false);

    let (artists, _) = ask(&mut library);
    answer(
        &mut library,
        &artists,
        vec![releases(
            &artists[0],
            vec![release("spotify:album:a", day())],
        )],
    );

    assert_eq!(
        drawn(&library.releases_section()).len(),
        1,
        "the shelf is the library's own, so the setting does not gate it"
    );
}

#[test]
fn the_row_is_one_page_row() {
    assert_eq!(DRAWN, 4, "the shelf draws one row, not a second one");
}
