use super::*;

const NOW_MS: u64 = 1_700_000_000_000;

/// `count` dated entries, a minute apart, oldest first.
fn dates(count: usize) -> Vec<Option<u64>> {
    (0..count)
        .map(|at| Some(NOW_MS + at as u64 * 60_000))
        .collect()
}

/// The seeded draws the shelf would make over the same dates.
fn picks(count: usize) -> Vec<Vec<usize>> {
    (1..=32)
        .map(|opens| pick(&dates(count), Seed::for_open(NOW_MS, opens)))
        .collect()
}

#[test]
fn a_held_seed_pins_the_pick() {
    let dates = dates(24);
    let seed = Seed::for_open(NOW_MS, 7);

    assert_eq!(pick(&dates, seed), pick(&dates, seed));
}

#[test]
fn two_seeds_differ() {
    let dates = dates(24);

    let first = pick(&dates, Seed::for_open(NOW_MS, 1));
    let second = pick(&dates, Seed::for_open(NOW_MS, 2));

    assert_ne!(first, second);
}

#[test]
fn the_pick_draws_no_episode_twice() {
    let every = picks(40);

    for picked in every {
        let mut unique = picked.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), picked.len(), "an episode was drawn twice");
    }
}

#[test]
fn the_pick_never_leaves_the_pool() {
    let all = dates(40);

    for picked in picks(40) {
        assert!(
            picked.len() <= DRAWN,
            "the shelf drew more rows than it has room for"
        );
        assert!(
            picked.iter().all(|at| *at >= all.len() - POOL),
            "the draw reached past the newest pool: {picked:?}"
        );
    }
}

#[test]
fn a_short_pool_draws_every_entry_once() {
    let dates = dates(3);

    let picked = pick(&dates, Seed::for_open(NOW_MS, 1));

    assert_eq!(picked, vec![2, 1, 0]);
}

#[test]
fn undated_episodes_sort_last() {
    let all = vec![Some(NOW_MS), None, Some(NOW_MS + 2), None, Some(NOW_MS + 1)];

    for opens in 1..=32 {
        let picked = pick(&all, Seed::for_open(NOW_MS, opens));
        assert!(
            picked.iter().all(|at| all[*at].is_some()),
            "an undated episode was drawn ahead of a dated one: {picked:?}"
        );
    }
}

#[test]
fn undated_episodes_never_fill_the_rows_a_dated_one_could() {
    let dates = vec![
        Some(NOW_MS),
        None,
        None,
        Some(NOW_MS + 1),
        None,
        None,
        Some(NOW_MS + 2),
    ];

    let picked = pick(&dates, Seed::for_open(NOW_MS, 1));

    assert_eq!(
        picked,
        vec![6, 3, 0],
        "the undated episodes took a row a dated one could have had"
    );
}

const SHOW: &str = "spotify:show:field-notes";
const OTHER: &str = "spotify:show:late-edition";
const SHOW_NAME: &str = "Field Notes";

const DAY_MS: u64 = 86_400_000;

/// One show whose twenty-four episodes are a day apart, newest first.
///
/// The episodes carry neither a show name nor a show address, which is what the
/// wire leaves out of a row it repeats: the answer's own show supplies both.
fn detail_of(uri: &str, name: &str) -> ShowDetail {
    ShowDetail {
        show: takofy_model::Show {
            uri: uri.to_owned(),
            name: name.to_owned(),
            publisher: String::new(),
            description: String::new(),
            cover: None,
        },
        episodes: (0..24)
            .map(|at| Episode {
                uri: format!("{uri}:episode:{at}"),
                title: format!("Episode {at}"),
                show: String::new(),
                show_uri: None,
                description: None,
                published: PublishedAt::from_millis((NOW_MS - at as u64 * DAY_MS) as i64),
                duration_ms: Some(1_800_000),
                cover: None,
                named: true,
            })
            .collect(),
    }
}

/// The one show every landing test opens.
fn detail() -> ShowDetail {
    detail_of(SHOW, SHOW_NAME)
}

/// One open of the shelf: it asks for the shows these details name, and they
/// answer together.
fn read(state: &mut State, details: Vec<ShowDetail>) {
    let asked: Vec<String> = details
        .iter()
        .map(|detail| detail.show.uri.clone())
        .collect();
    state.ask(&asked);
    state.land(&HomeAnswer::Episodes(details));
}

/// One open of the shelf, over the one show.
fn read_one(state: &mut State) {
    read(state, vec![detail()]);
}

/// `detail_of(SHOW, ..)` with every episode as a read that named none of them.
fn unnamed(uri: &str, name: &str) -> ShowDetail {
    let mut detail = detail_of(uri, name);
    for episode in &mut detail.episodes {
        episode.named = false;
        episode.published = None;
    }
    detail
}

/// The URIs the shelf draws, in the order it draws them.
fn drawn(state: &State) -> Vec<String> {
    let list = state.list().expect("the landed shows draw");
    list.rows
        .iter()
        .map(|episode| episode.uri.clone())
        .collect()
}

#[test]
fn a_reopen_draws_other_rows_and_a_retry_redraws_the_same() {
    let mut state = State::default();
    state.open(Seed::for_open(NOW_MS, 1));
    read_one(&mut state);
    let first = drawn(&state);
    assert_eq!(first.len(), DRAWN, "the open drew no rows");

    state.open(Seed::for_open(NOW_MS, 2));
    read_one(&mut state);
    let second = drawn(&state);
    assert_eq!(second.len(), DRAWN, "the reopened shelf drew no rows");
    assert_ne!(first, second, "a reopen drew the same rows");

    state.forget();
    read_one(&mut state);
    assert_eq!(
        drawn(&state),
        second,
        "a retry drew other rows than the open it retried"
    );
}

#[test]
fn an_episode_takes_the_show_it_came_from() {
    let mut state = State::default();
    state.open(Seed::for_open(NOW_MS, 1));
    read_one(&mut state);

    let list = state.list().expect("the landed show draws");

    assert!(!list.rows.is_empty());
    for episode in &list.rows {
        assert_eq!(episode.show, SHOW_NAME, "the row cannot name its show");
        assert_eq!(
            episode.show_uri.as_deref(),
            Some(SHOW),
            "the row cannot link its show"
        );
    }
}

#[test]
fn two_shows_draw_each_episode_with_its_own_show() {
    let mut state = State::default();
    state.open(Seed::for_open(NOW_MS, 1));
    read(&mut state, vec![detail(), detail_of(OTHER, "Late Edition")]);

    let list = state.list().expect("both shows draw");
    assert!(!list.rows.is_empty());
    let names: Vec<&str> = list
        .rows
        .iter()
        .map(|episode| episode.show.as_str())
        .collect();
    assert!(
        names.contains(&SHOW_NAME) && names.contains(&"Late Edition"),
        "an episode drew the other show's name: {names:?}"
    );
}

#[test]
fn the_same_details_draw_the_same_rows_whatever_order_they_arrive_in() {
    let one = detail();
    let two = detail_of(OTHER, "Late Edition");

    let mut first = State::default();
    first.open(Seed::for_open(NOW_MS, 1));
    read(&mut first, vec![one.clone(), two.clone()]);

    let mut second = State::default();
    second.open(Seed::for_open(NOW_MS, 1));
    read(&mut second, vec![two, one]);

    assert_eq!(
        drawn(&first),
        drawn(&second),
        "the rows followed the order the answers arrived in"
    );
}

#[test]
fn an_answer_that_named_its_episodes_is_readable() {
    let mut state = State::default();
    state.open(Seed::for_open(NOW_MS, 1));
    read_one(&mut state);

    assert!(state.readable(), "a named answer is the shelf's own");
}

#[test]
fn one_unnamed_show_leaves_the_others_drawn() {
    let mut state = State::default();
    state.open(Seed::for_open(NOW_MS, 1));
    read(
        &mut state,
        vec![unnamed(SHOW, SHOW_NAME), detail_of(OTHER, "Late Edition")],
    );

    assert!(state.readable(), "the other show named its episodes");
    let list = state.list().expect("the named show still draws");
    assert!(
        list.rows
            .iter()
            .all(|episode| episode.show_uri.as_deref() == Some(OTHER)),
        "a row was drawn from the show that named nothing"
    );
}

#[test]
fn an_answer_that_named_nothing_is_not_readable() {
    let mut state = State::default();
    state.open(Seed::for_open(NOW_MS, 1));
    read(
        &mut state,
        vec![unnamed(SHOW, SHOW_NAME), unnamed(OTHER, "Late Edition")],
    );

    assert!(
        !state.readable(),
        "episodes that fell back to their URIs named nothing"
    );
    assert!(
        state.list().is_none(),
        "arrange: nothing the answer carried can be drawn"
    );
}

#[test]
fn a_show_asked_but_absent_from_the_answer_fails() {
    let mut state = State::default();
    state.open(Seed::for_open(NOW_MS, 1));
    state.ask(&[SHOW.to_owned(), OTHER.to_owned()]);

    state.land(&HomeAnswer::Episodes(vec![detail()]));

    assert_eq!(state.landed().len(), 1, "arrange: only one show came back");
    assert_eq!(
        state.failure(),
        Some(Failure::Load),
        "a show the answer left out is a read that did not happen"
    );
    assert!(
        !state.pending(),
        "the open waited for a show it will never see"
    );
}

#[test]
fn fail_fails_every_asked_show_that_has_not_landed() {
    let mut state = State::default();
    state.open(Seed::for_open(NOW_MS, 1));
    state.ask(&[SHOW.to_owned()]);

    state.fail(
        &HomeAsk::Episodes {
            shows: vec![SHOW.to_owned(), OTHER.to_owned()],
        },
        Failure::Load,
    );

    assert_eq!(state.failure(), Some(Failure::Load));
    assert!(
        !state.pending(),
        "the open waited for a show the failure already covered"
    );
}

#[test]
fn a_failure_never_costs_a_show_that_landed() {
    let mut state = State::default();
    state.open(Seed::for_open(NOW_MS, 1));
    state.ask(&[SHOW.to_owned()]);
    state.land(&HomeAnswer::Episodes(vec![detail()]));

    state.fail(
        &HomeAsk::Episodes {
            shows: vec![SHOW.to_owned()],
        },
        Failure::Load,
    );

    assert_eq!(state.failure(), None, "a landed show kept a failure");
    assert_eq!(state.landed().len(), 1);
}

#[test]
fn an_answer_for_a_show_the_open_never_asked_is_dropped() {
    let mut state = State::default();
    state.open(Seed::for_open(NOW_MS, 1));
    state.ask(&[SHOW.to_owned()]);

    state.land(&HomeAnswer::Episodes(vec![detail_of(
        OTHER,
        "Late Edition",
    )]));

    assert!(
        state.landed().is_empty(),
        "an answer this open never asked for was drawn"
    );
    assert_eq!(
        state.failure(),
        None,
        "another open's answer is not this open's failure"
    );
    assert!(state.pending(), "the asked show is still owed an answer");
}

#[test]
fn forget_leaves_the_open_nothing_to_draw() {
    let mut state = State::default();
    state.open(Seed::for_open(NOW_MS, 1));
    read_one(&mut state);
    assert!(state.list().is_some());

    state.forget();

    assert!(state.landed().is_empty());
    assert!(state.asked().is_empty());
    assert_eq!(state.failure(), None);
    assert!(state.list().is_none());
}
