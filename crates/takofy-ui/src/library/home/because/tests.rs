use takofy_model::home::{HomeAnswer, HomeAsk};
use takofy_model::{FollowedArtist, FollowedShow};

use super::super::Library;
use crate::home::{Budget, Section, Slot};
use crate::library::Load;
use crate::message::{Command, Failure, Update};

const NOW_MS: u64 = 1_700_000_000_000;

fn artist(uri: &str, name: &str) -> FollowedArtist {
    FollowedArtist {
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        portrait: None,
        releases: None,
    }
}

fn followed(count: usize) -> Vec<FollowedArtist> {
    (0..count)
        .map(|at| artist(&format!("spotify:artist:{at}"), &format!("Artist {at}")))
        .collect()
}

/// A Home library whose followed artists have answered with `artists`.
fn library(artists: Vec<FollowedArtist>) -> Library {
    let mut library = Library::starting_home(NOW_MS);
    library.apply(&Update::FollowedArtists(artists));
    library
}

/// The artist the shelf owes a read for, which it must be asking for one.
fn ask(library: &mut Library) -> String {
    let Some(Command::LoadHome(HomeAsk::Related { artist })) = library.because_ask() else {
        panic!("the shelf owes a related read");
    };
    artist
}

fn answer(library: &mut Library, artist: &str, cards: Vec<FollowedArtist>) {
    library.apply(&Update::Home(HomeAnswer::Related {
        artist: artist.to_owned(),
        cards,
    }));
}

fn fail(library: &mut Library, artist: &str, failure: Failure) {
    library.apply(&Update::HomeFailed {
        ask: HomeAsk::Related {
            artist: artist.to_owned(),
        },
        failure,
    });
}

#[test]
fn a_ready_but_empty_answer_advances_to_the_next_artist() {
    let mut library = library(followed(4));

    let first = ask(&mut library);
    assert_eq!(
        library.because_ask(),
        None,
        "a second read went out while one was outstanding"
    );
    answer(&mut library, &first, Vec::new());
    let second = ask(&mut library);

    assert_ne!(first, second, "an empty answer repeated its own artist");
}

#[test]
fn the_ask_order_has_no_repeats_and_stops_at_the_budget() {
    let mut library = library(followed(6));
    let cap = Budget::per_open(Slot::Because);

    let mut asked: Vec<String> = Vec::new();
    for _ in 0..cap {
        let artist = ask(&mut library);
        assert!(!asked.contains(&artist), "one artist was asked twice");
        asked.push(artist.clone());
        answer(&mut library, &artist, Vec::new());
    }

    assert_eq!(asked.len(), cap);
    assert_eq!(
        library.because_ask(),
        None,
        "the shelf asked past its allowance"
    );
    assert_eq!(library.home.budget.left(Slot::Because), 0);
}

#[test]
fn all_empty_answers_settle_absent() {
    let mut library = library(followed(6));

    for _ in 0..Budget::per_open(Slot::Because) {
        let artist = ask(&mut library);
        answer(&mut library, &artist, Vec::new());
    }

    assert!(
        matches!(library.because_section(), Section::Absent),
        "an artist with no neighbours is not a failure"
    );
}

#[test]
fn a_stale_answer_is_ignored() {
    let mut library = library(followed(3));
    let first = ask(&mut library);

    answer(
        &mut library,
        "spotify:artist:elsewhere",
        vec![artist("spotify:artist:z", "Zed")],
    );

    assert!(
        matches!(library.because_section(), Section::Waiting),
        "an answer this open never asked for settled the shelf"
    );
    assert_eq!(library.because_ask(), None);

    answer(
        &mut library,
        &first,
        vec![artist("spotify:artist:z", "Zed")],
    );
    assert!(matches!(library.because_section(), Section::Ready(_)));
}

#[test]
fn a_landed_answer_names_the_followed_artist() {
    let artists = vec![
        artist("spotify:artist:a", "Aria"),
        artist("spotify:artist:b", "Bass"),
    ];
    let mut library = library(artists.clone());
    let first = ask(&mut library);
    let name = artists
        .iter()
        .find(|artist| artist.uri == first)
        .map(|artist| artist.name.clone())
        .expect("the ask names a followed artist");

    answer(
        &mut library,
        &first,
        vec![artist("spotify:artist:z", "Zed")],
    );

    let Section::Ready(row) = library.because_section() else {
        panic!("the answer draws a row");
    };
    assert_eq!(row.seed_name, name, "the head named a related artist");
    assert_eq!(row.cards.len(), 1);
    assert_eq!(row.cards[0].uri, "spotify:artist:z");
}

#[test]
fn nothing_followed_settles_absent() {
    let mut library = library(Vec::new());

    assert!(matches!(library.because_section(), Section::Absent));
    assert_eq!(library.because_ask(), None);
}

#[test]
fn an_unreadable_followed_list_settles_failed() {
    let mut library = library(Vec::new());
    library.followed_artists = Load::Failed(Failure::FollowedArtists);

    assert!(matches!(
        library.because_section(),
        Section::Failed(Failure::FollowedArtists)
    ));
}

#[test]
fn one_failed_read_does_not_end_the_section() {
    let artists = followed(6);
    let mut library = library(artists.clone());

    let first = ask(&mut library);
    fail(&mut library, &first, Failure::Related);

    assert!(
        matches!(library.because_section(), Section::Waiting),
        "one failed read ended a section the allowance could still fill"
    );
    let second = ask(&mut library);
    assert_ne!(second, first, "a failed read asked the same artist again");

    answer(
        &mut library,
        &second,
        vec![artist("spotify:artist:z", "Zed")],
    );

    let Section::Ready(row) = library.because_section() else {
        panic!("an answer after a failed read draws a row");
    };
    let name = artists
        .iter()
        .find(|artist| artist.uri == second)
        .map(|artist| artist.name.clone())
        .expect("the ask names a followed artist");
    assert_eq!(
        row.seed_name, name,
        "the head named an artist that did not answer"
    );
    assert_eq!(row.cards.len(), 1);
    assert_eq!(row.cards[0].uri, "spotify:artist:z");
}

#[test]
fn every_failed_read_settles_failed() {
    let mut library = library(followed(6));

    for _ in 0..Budget::per_open(Slot::Because) {
        let artist = ask(&mut library);
        fail(&mut library, &artist, Failure::Related);
    }

    assert!(
        matches!(library.because_section(), Section::Failed(Failure::Related)),
        "an open that drew nothing while its reads failed is not a quiet library"
    );
    assert_eq!(
        library.because_ask(),
        None,
        "a failed open asked again without a retry"
    );
}

#[test]
fn a_held_seed_asks_the_same_artist() {
    let artists = followed(6);

    let first = ask(&mut library(artists.clone()));
    let second = ask(&mut library(artists));

    assert_eq!(first, second);
}

#[test]
fn a_retry_asks_the_same_first_artist() {
    let mut library = library(followed(4));
    let first = ask(&mut library);
    fail(&mut library, &first, Failure::Related);

    library.retry_home(Slot::Because);

    assert_eq!(ask(&mut library), first);
}

/// The page asks for the followed artists on its first frame, so an `Idle` list
/// is the one frame before that ask — not a read this shelf waits on. Waiting on
/// it would hide the day-one invite behind a section that draws nothing.
#[test]
fn an_unasked_followed_list_is_not_waited_on() {
    let mut library = Library::starting_home(NOW_MS);
    library.apply(&Update::Playlists(Vec::new()));
    library.apply(&Update::LikedTracks(Vec::new()));
    library.apply(&Update::SavedAlbums(Vec::new()));
    assert!(matches!(library.followed_artists, Load::Idle));

    assert!(
        !matches!(library.because_section(), Section::Waiting),
        "a list nobody asked for yet is not something to wait for"
    );
}

#[test]
fn because_and_episodes_ask_without_the_playlists() {
    let mut library = Library::starting_home(NOW_MS);
    library.apply(&Update::SignedIn);
    library.apply(&Update::LibraryReady);
    library.apply(&Update::FollowedArtists(followed(2)));
    library.apply(&Update::FollowedShows(vec![FollowedShow {
        uri: "spotify:show:a".to_owned(),
        name: "Show".to_owned(),
        named: true,
        publisher: None,
        cover: None,
        episodes: None,
    }]));
    assert_eq!(
        library.playlists,
        Load::Loading,
        "arrange: the labels are still out"
    );

    assert!(
        matches!(
            library.because_ask(),
            Some(Command::LoadHome(HomeAsk::Related { .. }))
        ),
        "the related shelf reads only the followed artists"
    );
    assert!(
        matches!(
            library.episodes_ask(),
            Some(Command::LoadHome(HomeAsk::Episodes { .. }))
        ),
        "the episodes shelf reads only the followed shows"
    );
}
