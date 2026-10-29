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
