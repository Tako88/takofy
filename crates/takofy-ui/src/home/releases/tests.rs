use takofy_model::home::{ArtistReleases, Day, HomeAnswer, HomeAsk, NewRelease, ReleaseDate};
use takofy_model::{FollowedArtist, SavedAt};

use super::{DRAWN, SHELF_ARTISTS, State, admits, followed_day, window_start};
use crate::message::Failure;

fn day_at(days: i32) -> Day {
    Day::from_days(days)
}

/// The day the shelf is open on.
fn today() -> Day {
    day_at(739_600)
}

/// A day `back` days before today.
fn back(back: i32) -> Day {
    day_at(today().days() - back)
}

/// The instant a day starts, for a follow date.
fn date(day: Day) -> SavedAt {
    let millis = i64::from(day.days() - 719_163) * 86_400_000;
    SavedAt::from_millis(millis).expect("a real instant")
}

fn followed(uri: &str, at: Option<Day>) -> FollowedArtist {
    FollowedArtist {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        portrait: None,
        releases: None,
        followed_at: at.map(date),
    }
}

fn release(uri: &str, day: Day) -> NewRelease {
    NewRelease {
        artist: "Vela Moreno".to_owned(),
        uri: uri.to_owned(),
        name: uri.to_owned(),
        date: ReleaseDate {
            year: 2025,
            month: Some(10),
            day: Some(14),
        },
        day,
        cover: None,
    }
}

fn releases(artist: &str, list: Vec<NewRelease>) -> ArtistReleases {
    ArtistReleases {
        artist: artist.to_owned(),
        releases: list,
    }
}

fn latched() -> State {
    let mut state = State::default();
    state.latch(today());
    state
}

/// An answer from `found`, as the read lands it.
fn answer(state: &mut State, asked: &[&str], found: Vec<ArtistReleases>) {
    state.land(&HomeAnswer::Releases {
        asked: asked.iter().map(|uri| (*uri).to_owned()).collect(),
        found,
    });
}

/// The URIs the shelf draws, in order.
fn uris(list: &super::List<'_>) -> Vec<String> {
    list.cards
        .iter()
        .map(|card| card.release.uri.clone())
        .collect()
}

#[test]
fn the_window_reaches_thirty_days_back_for_an_old_follow() {
    for at in [Some(date(back(200))), None] {
        let start = window_start(at, today(), 0);
        assert_eq!(start.days(), back(30).days());
        assert!(
            admits(back(30), start, today()),
            "the floor's own day is in"
        );
        assert!(
            !admits(back(31), start, today()),
            "a day past the floor is out"
        );
    }
}

#[test]
fn a_follow_later_than_the_floor_narrows_the_window() {
    let start = window_start(Some(date(back(5))), today(), 0);

    assert_eq!(start.days(), back(5).days());
    assert!(
        !admits(back(10), start, today()),
        "a release older than the follow is not new to the reader"
    );
    assert!(
        admits(back(5), start, today()),
        "the follow's own day is in"
    );
}

#[test]
fn a_follow_older_than_the_floor_gets_the_floor() {
    assert_eq!(
        window_start(Some(date(back(60))), today(), 0).days(),
        back(30).days()
    );
}

#[test]
fn a_follow_on_the_floors_day_is_in() {
    let start = window_start(Some(date(back(30))), today(), 0);
    assert_eq!(start.days(), back(30).days());
    assert!(admits(back(30), start, today()));
}

#[test]
fn a_release_dated_after_today_is_out() {
    let start = window_start(None, today(), 0);
    assert!(!admits(back(-1), start, today()), "a future day");
    assert!(admits(today(), start, today()), "today is in");
}

#[test]
fn the_state_refuses_a_release_it_cannot_place() {
    let mut state = latched();
    answer(
        &mut state,
        &["spotify:artist:a"],
        vec![releases(
            "spotify:artist:a",
            vec![
                release("spotify:album:floor", back(30)),
                release("spotify:album:past", back(31)),
                release("spotify:album:today", today()),
                release("spotify:album:future", back(-1)),
            ],
        )],
    );

    assert_eq!(
        uris(&state.pick(&[followed("spotify:artist:a", None)], 0, DRAWN)),
        vec!["spotify:album:today", "spotify:album:floor"],
        "a release no window can hold was kept"
    );
}

#[test]
fn the_cards_come_newest_first() {
    let mut state = latched();
    answer(
        &mut state,
        &["spotify:artist:a"],
        vec![releases(
            "spotify:artist:a",
            vec![
                release("spotify:album:b", back(9)),
                release("spotify:album:c", today()),
                release("spotify:album:a", back(3)),
            ],
        )],
    );

    assert_eq!(
        uris(&state.pick(&[followed("spotify:artist:a", None)], 0, DRAWN)),
        vec!["spotify:album:c", "spotify:album:a", "spotify:album:b"]
    );
}

#[test]
fn a_tie_goes_to_the_earlier_followed_artist_then_the_uri() {
    let mut state = latched();
    let day = back(2);
    answer(
        &mut state,
        &["spotify:artist:a", "spotify:artist:b"],
        vec![
            releases(
                "spotify:artist:b",
                vec![
                    release("spotify:album:z", day),
                    release("spotify:album:a", day),
                ],
            ),
            releases("spotify:artist:a", vec![release("spotify:album:y", day)]),
        ],
    );

    let followed = [
        followed("spotify:artist:a", None),
        followed("spotify:artist:b", None),
    ];
    assert_eq!(
        uris(&state.pick(&followed, 0, DRAWN)),
        vec!["spotify:album:y", "spotify:album:a", "spotify:album:z"],
        "the tie was not broken by followed order and then by URI"
    );
}

#[test]
fn one_release_uri_draws_one_card() {
    let mut state = latched();
    answer(
        &mut state,
        &["spotify:artist:a", "spotify:artist:b"],
        vec![
            releases(
                "spotify:artist:a",
                vec![release("spotify:album:x", today())],
            ),
            releases(
                "spotify:artist:b",
                vec![release("spotify:album:x", today())],
            ),
        ],
    );

    let followed = [
        followed("spotify:artist:a", None),
        followed("spotify:artist:b", None),
    ];
    assert_eq!(state.pick(&followed, 0, DRAWN).cards.len(), 1);
}

#[test]
fn the_row_is_cut_at_the_limit() {
    let mut state = latched();
    answer(
        &mut state,
        &["spotify:artist:a"],
        vec![releases(
            "spotify:artist:a",
            (0..6)
                .map(|at| release(&format!("spotify:album:{at}"), today()))
                .collect(),
        )],
    );

    assert_eq!(
        state
            .pick(&[followed("spotify:artist:a", None)], 0, DRAWN)
            .cards
            .len(),
        DRAWN
    );
}

#[test]
fn an_unfollowed_artist_drops_out() {
    let mut state = latched();
    answer(
        &mut state,
        &["spotify:artist:a", "spotify:artist:b"],
        vec![
            releases(
                "spotify:artist:a",
                vec![release("spotify:album:a", today())],
            ),
            releases(
                "spotify:artist:b",
                vec![release("spotify:album:b", today())],
            ),
        ],
    );

    assert_eq!(
        uris(&state.pick(&[followed("spotify:artist:a", None)], 0, DRAWN)),
        vec!["spotify:album:a"]
    );
}

#[test]
fn a_follow_day_is_the_local_day_of_the_instant() {
    let day = today();
    let at = date(day);

    assert_eq!(
        followed_day(at, 0),
        day,
        "a midnight instant is its own day"
    );
    assert_eq!(
        followed_day(at, 3_600),
        day,
        "an hour into a day is the same day"
    );
    let before = SavedAt::from_millis(i64::try_from(at.as_millis()).expect("an instant") - 1_000)
        .expect("a real instant");
    assert_eq!(
        followed_day(before, 0),
        back(1),
        "a second before midnight is the day before"
    );
    assert_eq!(
        followed_day(before, 3_600),
        day,
        "a zone ahead crosses that midnight with the instant"
    );
}

#[test]
fn the_ask_names_the_first_sixty_neither_held_nor_asked() {
    let mut state = latched();
    let followed: Vec<FollowedArtist> = (0..70)
        .map(|at| followed(&format!("spotify:artist:{at}"), None))
        .collect();

    let owed = state.owed(&followed);
    assert_eq!(owed.len(), SHELF_ARTISTS, "the cap was not applied");
    assert_eq!(
        owed[0], "spotify:artist:0",
        "the ask left the followed order"
    );

    let asked: Vec<&str> = owed.iter().map(String::as_str).collect();
    state.ask(&owed);
    answer(
        &mut state,
        &asked,
        vec![releases("spotify:artist:0", Vec::new())],
    );

    assert!(
        state.owed(&followed).is_empty(),
        "an artist this open asked for was asked for again"
    );
}

#[test]
fn an_artist_that_landed_empty_is_held_and_still_answers_once() {
    let mut state = latched();
    answer(
        &mut state,
        &["spotify:artist:a"],
        vec![releases("spotify:artist:a", Vec::new())],
    );

    assert!(
        state.owed(&[followed("spotify:artist:a", None)]).is_empty(),
        "an empty answer was asked for again"
    );
    assert!(!state.pending(), "an empty answer still settled the read");
    assert!(state.held_list(DRAWN).cards.is_empty());
}

#[test]
fn an_artist_the_read_left_out_is_not_asked_again_this_open() {
    let mut state = latched();
    state.ask(&["spotify:artist:a".to_owned()]);
    answer(&mut state, &["spotify:artist:a"], Vec::new());

    assert!(!state.pending(), "a whole answer settles the ask");
    assert!(state.owed(&[followed("spotify:artist:a", None)]).is_empty());
}

#[test]
fn opening_again_keeps_what_is_held_and_forgets_the_asks() {
    let mut state = latched();
    answer(
        &mut state,
        &["spotify:artist:a"],
        vec![releases(
            "spotify:artist:a",
            vec![release("spotify:album:a", today())],
        )],
    );

    state.open();

    assert_eq!(
        state
            .pick(&[followed("spotify:artist:a", None)], 0, DRAWN)
            .cards
            .len(),
        1,
        "a second open on the same day dropped what it held"
    );
    assert!(
        state.owed(&[followed("spotify:artist:a", None)]).is_empty(),
        "what is held is not re-read"
    );
    assert!(!state.pending(), "the asks were kept");
}

#[test]
fn a_new_day_drops_what_the_last_day_held() {
    let mut state = latched();
    answer(
        &mut state,
        &["spotify:artist:a"],
        vec![releases(
            "spotify:artist:a",
            vec![release("spotify:album:a", today())],
        )],
    );

    state.latch(back(-1));

    assert_eq!(state.day(), Some(back(-1)));
    assert_eq!(
        state.owed(&[followed("spotify:artist:a", None)]),
        vec!["spotify:artist:a".to_owned()],
        "the new day did not ask again"
    );
    assert!(state.held_list(DRAWN).cards.is_empty());
}

#[test]
fn a_failure_settles_the_open_and_asks_nothing_more() {
    let mut state = latched();
    let ask = HomeAsk::Releases {
        artists: vec!["spotify:artist:a".to_owned()],
        since: back(30),
    };

    state.fail(&ask, Failure::Load);

    assert_eq!(state.failure(), Some(Failure::Load));
    assert!(!state.pending(), "the failed read is still owed");
    assert!(state.owed(&[followed("spotify:artist:a", None)]).is_empty());
}

#[test]
fn a_release_landed_before_the_day_is_latched_is_dropped() {
    let mut state = State::default();
    answer(
        &mut state,
        &["spotify:artist:a"],
        vec![releases(
            "spotify:artist:a",
            vec![release("spotify:album:a", today())],
        )],
    );

    assert!(state.held_list(DRAWN).cards.is_empty());
}
