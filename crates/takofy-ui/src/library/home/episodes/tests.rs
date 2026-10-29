use takofy_model::home::{HomeAnswer, HomeAsk};
use takofy_model::{Episode, FollowedShow, PublishedAt, Show, ShowDetail};

use super::super::Library;
use crate::home::episodes::{DRAWN, SHOWS};
use crate::home::{Budget, Section, Slot};
use crate::library::Load;
use crate::message::{Command, Failure, Update};

const NOW_MS: u64 = 1_700_000_000_000;
const DAY_MS: u64 = 86_400_000;

fn show_uri(at: usize) -> String {
    format!("spotify:show:{at}")
}

fn followed(count: usize) -> Vec<FollowedShow> {
    (0..count)
        .map(|at| FollowedShow {
            uri: show_uri(at),
            name: format!("Show {at}"),
            named: true,
            publisher: None,
            episodes: None,
            cover: None,
        })
        .collect()
}

fn episode(show: &str, at: usize, days_ago: u64) -> Episode {
    Episode {
        uri: format!("spotify:episode:{show}:{at}"),
        title: format!("Episode {at}"),
        show: show.to_owned(),
        show_uri: Some(show.to_owned()),
        description: None,
        published: PublishedAt::from_millis((NOW_MS - days_ago * DAY_MS) as i64),
        duration_ms: Some(1_800_000),
        cover: None,
        named: true,
    }
}

fn detail(show: &str, days_ago: u64) -> ShowDetail {
    ShowDetail {
        show: Show {
            uri: show.to_owned(),
            name: show.to_owned(),
            publisher: String::new(),
            description: String::new(),
            cover: None,
        },
        episodes: (0..3)
            .map(|at| episode(show, at, days_ago + at as u64))
            .collect(),
    }
}

/// A Home library whose followed shows have answered with `shows`.
fn library(shows: Vec<FollowedShow>) -> Library {
    let mut library = Library::starting_home(NOW_MS);
    library.apply(&Update::FollowedShows(shows));
    library
}

/// The show the shelf owes a read for, which it must be asking for one.
fn ask(library: &mut Library) -> String {
    let Some(Command::LoadHome(HomeAsk::Show { show })) = library.episodes_ask() else {
        panic!("the shelf owes a show read");
    };
    show
}

fn answer(library: &mut Library, detail: ShowDetail) {
    library.apply(&Update::Home(HomeAnswer::Show(detail)));
}

fn fail(library: &mut Library, show: &str, failure: Failure) {
    library.apply(&Update::HomeFailed {
        ask: HomeAsk::Show {
            show: show.to_owned(),
        },
        failure,
    });
}

#[test]
fn the_ask_names_each_newest_show_once_within_the_allowance() {
    let mut library = library(followed(10));
    let mut asked: Vec<String> = Vec::new();
    while let Some(Command::LoadHome(HomeAsk::Show { show })) = library.episodes_ask() {
        assert!(!asked.contains(&show), "one show was asked twice");
        asked.push(show);
    }

    assert_eq!(asked.len(), SHOWS);
    assert_eq!(asked.len(), Budget::per_open(Slot::Episodes));
    let newest: Vec<String> = (0..SHOWS).map(show_uri).collect();
    assert_eq!(asked, newest, "the asks left the newest-followed order");
    assert_eq!(library.home.budget.left(Slot::Episodes), 0);
}

#[test]
fn waiting_until_every_asked_show_answered() {
    let mut library = library(followed(4));
    let mut shows: Vec<String> = (0..4).map(|_| ask(&mut library)).collect();

    assert!(
        matches!(library.episodes_section(), Section::Waiting),
        "the shelf drew while its reads were out"
    );
    for show in &shows[..3] {
        answer(&mut library, detail(show, 1));
    }
    assert!(
        matches!(library.episodes_section(), Section::Waiting),
        "one show was still out, so the shelf had not settled"
    );

    let last = shows.pop().expect("four shows were asked");
    answer(&mut library, detail(&last, 1));
    assert!(
        matches!(library.episodes_section(), Section::Ready(_)),
        "every asked show answered and nothing was drawn"
    );
}

#[test]
fn one_failed_show_leaves_the_others_drawn() {
    let mut library = library(followed(4));
    let shows: Vec<String> = (0..4).map(|_| ask(&mut library)).collect();

    for show in &shows[..3] {
        answer(&mut library, detail(show, 1));
    }
    fail(&mut library, &shows[3], Failure::Load);

    let Section::Ready(list) = library.episodes_section() else {
        panic!("three shows landed and the shelf is not drawn");
    };
    assert_eq!(list.rows.len(), DRAWN);
    let drawn: Vec<&str> = list
        .rows
        .iter()
        .filter_map(|row| row.show_uri.as_deref())
        .collect();
    assert!(!drawn.is_empty());
    assert!(
        drawn
            .iter()
            .all(|uri| shows[..3].iter().any(|show| show == uri)),
        "a row came from the show that failed"
    );
}

#[test]
fn every_show_failing_settles_failed() {
    let mut library = library(followed(4));
    let shows: Vec<String> = (0..4).map(|_| ask(&mut library)).collect();

    for show in &shows {
        fail(&mut library, show, Failure::Load);
    }

    assert!(
        matches!(library.episodes_section(), Section::Failed(Failure::Load)),
        "an open that drew nothing while every read failed is not a quiet library"
    );
    assert_eq!(
        library.episodes_ask(),
        None,
        "a failed open asked again without a retry"
    );
}

#[test]
fn a_stale_show_answer_is_ignored() {
    let mut library = library(followed(1));
    let first = ask(&mut library);

    answer(&mut library, detail("spotify:show:elsewhere", 1));
    assert!(
        matches!(library.episodes_section(), Section::Waiting),
        "an answer this open never asked for settled the shelf"
    );

    answer(&mut library, detail(&first, 1));
    assert!(
        matches!(library.episodes_section(), Section::Ready(_)),
        "the asked show answered and the shelf did not draw"
    );
}

#[test]
fn nothing_followed_settles_absent() {
    let mut library = library(Vec::new());

    assert!(matches!(library.episodes_section(), Section::Absent));
    assert_eq!(library.episodes_ask(), None);
}

#[test]
fn an_unreadable_followed_list_settles_failed() {
    let mut library = library(followed(2));
    library.followed = Load::Failed(Failure::Followed);

    assert!(matches!(
        library.episodes_section(),
        Section::Failed(Failure::Followed)
    ));
    assert_eq!(library.episodes_ask(), None);
}

#[test]
fn the_rows_run_newest_first() {
    let mut library = library(followed(2));
    let shows: Vec<String> = (0..2).map(|_| ask(&mut library)).collect();

    answer(&mut library, detail(&shows[0], 30));
    answer(&mut library, detail(&shows[1], 1));

    let Section::Ready(list) = library.episodes_section() else {
        panic!("two shows landed and the shelf is not drawn");
    };
    assert!(!list.rows.is_empty());
    let dates: Vec<u64> = list
        .rows
        .iter()
        .filter_map(|row| row.published)
        .map(PublishedAt::as_millis)
        .collect();
    assert_eq!(dates.len(), list.rows.len(), "a drawn row had no date");
    assert!(
        dates.windows(2).all(|pair| pair[0] >= pair[1]),
        "the drawn rows do not run downhill: {dates:?}"
    );
}
