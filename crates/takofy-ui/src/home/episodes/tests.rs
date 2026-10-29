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
const SHOW_NAME: &str = "Field Notes";

/// One show whose twenty-four episodes are a day apart, newest first.
///
/// The episodes carry neither a show name nor a show address, which is what the
/// wire leaves out of a row it repeats: the answer's own show supplies both.
fn detail() -> ShowDetail {
    ShowDetail {
        show: takofy_model::Show {
            uri: SHOW.to_owned(),
            name: SHOW_NAME.to_owned(),
            publisher: String::new(),
            description: String::new(),
            cover: None,
        },
        episodes: (0..24)
            .map(|at| Episode {
                uri: format!("spotify:episode:{at}"),
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

const DAY_MS: u64 = 86_400_000;

/// One open of the shelf: it asks for the show, which answers.
fn read(state: &mut State) {
    state.ask(SHOW);
    state.land(&HomeAnswer::Show(detail()));
}

/// The URIs the shelf draws, in the order it draws them.
fn drawn(state: &State) -> Vec<String> {
    let list = state.list().expect("the landed show draws");
    list.rows
        .iter()
        .map(|episode| episode.uri.clone())
        .collect()
}

#[test]
fn a_reopen_draws_other_rows_and_a_retry_redraws_the_same() {
    let mut state = State::default();
    state.open(Seed::for_open(NOW_MS, 1));
    read(&mut state);
    let first = drawn(&state);
    assert_eq!(first.len(), DRAWN, "the open drew no rows");

    state.open(Seed::for_open(NOW_MS, 2));
    read(&mut state);
    let second = drawn(&state);
    assert_eq!(second.len(), DRAWN, "the reopened shelf drew no rows");
    assert_ne!(first, second, "a reopen drew the same rows");

    state.forget();
    read(&mut state);
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
    read(&mut state);

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
fn an_answer_that_named_its_episodes_is_readable() {
    let mut state = State::default();
    state.open(Seed::for_open(NOW_MS, 1));
    read(&mut state);

    assert!(state.readable(), "a named answer is the shelf's own");
}

#[test]
fn an_answer_that_named_nothing_is_not_readable() {
    let mut state = State::default();
    state.open(Seed::for_open(NOW_MS, 1));
    state.ask(SHOW);
    let mut fallbacks = detail();
    for episode in &mut fallbacks.episodes {
        episode.named = false;
        episode.published = None;
    }
    state.land(&HomeAnswer::Show(fallbacks));

    assert!(
        !state.readable(),
        "episodes that fell back to their URIs named nothing"
    );
    assert!(
        state.list().is_none(),
        "arrange: nothing the answer carried can be drawn"
    );
}
