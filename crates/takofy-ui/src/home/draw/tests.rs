use super::*;

/// A date `days` before a fixed clock, in milliseconds.
fn date(days: u64) -> Option<SavedAt> {
    SavedAt::from_millis(1_000_000_000_000 - (days as i64) * 86_400_000)
}

/// `count` dated entries, newest first.
fn newest_first(count: usize) -> Vec<Option<SavedAt>> {
    (0..count).map(|i| date(i as u64)).collect()
}

fn seed() -> Seed {
    Seed::for_open(1_700_000_000_000, 1)
}

#[test]
fn a_held_seed_pins_the_pick() {
    let dates = newest_first(POOL);
    let drawn = pick(&dates, &pool(&dates), seed(), 5);
    assert_eq!(drawn, vec![2, 3, 8, 15, 20]);
    assert_eq!(pick(&dates, &pool(&dates), seed(), 5), drawn);
}

#[test]
fn another_seed_picks_another_set() {
    let dates = newest_first(POOL);
    let other = Seed::for_open(1_700_000_000_000, 2);
    assert_ne!(
        pick(&dates, &pool(&dates), seed(), 5),
        pick(&dates, &pool(&dates), other, 5)
    );
}

#[test]
fn a_card_is_never_drawn_twice() {
    let dates = newest_first(POOL);
    let drawn = pick(&dates, &pool(&dates), seed(), 10);
    let mut seen = drawn.clone();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), drawn.len());
    assert_eq!(drawn.len(), 10);
}

#[test]
fn an_undated_entry_never_enters_the_pool() {
    let mut dates = vec![None, date(3), None, date(1), date(2), None];
    assert_eq!(pool(&dates), vec![3, 4, 1]);
    dates.push(date(0));
    assert_eq!(pool(&dates), vec![6, 3, 4, 1]);
}

#[test]
fn the_drawn_dates_do_not_increase() {
    let dates = newest_first(POOL);
    let drawn = pick(&dates, &pool(&dates), seed(), 8);
    let millis: Vec<u64> = drawn
        .iter()
        .filter_map(|index| dates.get(*index).copied().flatten())
        .map(SavedAt::as_millis)
        .collect();
    assert!(millis.windows(2).all(|pair| pair[0] >= pair[1]));
}

#[test]
fn a_shelf_as_wide_as_the_pool_draws_it_whole_newest_first() {
    let dates = newest_first(POOL);
    let drawn = pick(&dates, &pool(&dates), seed(), POOL);
    assert_eq!(drawn, (0..POOL).collect::<Vec<_>>());
    assert_eq!(pick(&dates, &pool(&dates), seed(), POOL + 10), drawn);
}

#[test]
fn no_cards_and_no_dates_draw_nothing() {
    let dates = newest_first(POOL);
    assert!(pick(&dates, &pool(&dates), seed(), 0).is_empty());
    assert!(pool(&[]).is_empty());
    assert!(pick(&[], &[], seed(), 4).is_empty());
}

#[test]
fn a_hundred_dated_entries_pool_the_newest_twenty_four() {
    let dates = newest_first(100);
    assert_eq!(pool(&dates), (0..POOL).collect::<Vec<_>>());
}

#[test]
fn a_wider_shelf_keeps_every_card_it_already_drew() {
    let dates = newest_first(POOL);
    let pooled = pool(&dates);
    for cards in 0..pooled.len() {
        let narrower = pick(&dates, &pooled, seed(), cards);
        let wider = pick(&dates, &pooled, seed(), cards + 1);
        assert!(
            narrower.iter().all(|index| wider.contains(index)),
            "k = {cards}"
        );
        assert_eq!(wider.len(), cards + 1);
    }
}

#[test]
fn a_tie_keeps_the_input_order() {
    let dates = vec![None, date(2), date(1), date(0), date(1)];
    assert_eq!(pool(&dates), vec![3, 2, 4, 1]);
}

#[test]
fn a_pool_index_past_the_dates_is_dropped() {
    let dates = newest_first(3);
    assert_eq!(pick(&dates, &[0, 5], seed(), 2), vec![0]);
}

#[test]
fn a_pool_index_that_names_an_undated_entry_is_dropped() {
    let dates = vec![date(1), None, date(2)];
    assert_eq!(
        pick(&dates, &[0, 1], seed(), 2),
        vec![0],
        "an in-range but undated index is not a card"
    );
}

#[test]
fn sample_has_no_repeats() {
    for want in 0..=12 {
        let drawn = sample(12, seed(), want);
        let mut seen = drawn.clone();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), drawn.len(), "want = {want}");
        assert_eq!(drawn.len(), want);
        assert!(drawn.windows(2).all(|pair| pair[0] < pair[1]), "ascending");
    }
    assert_eq!(sample(4, seed(), 9).len(), 4, "a sample past its pool");
    assert!(sample(0, seed(), 3).is_empty());
}

#[test]
fn a_held_seed_samples_the_same_positions() {
    assert_eq!(sample(20, seed(), 6), sample(20, seed(), 6));
    assert_ne!(
        sample(20, seed(), 6),
        sample(20, Seed::for_open(1_700_000_000_000, 2), 6)
    );
}

#[test]
fn newest_is_the_pool_of_a_narrower_width() {
    let dates = newest_first(100);
    assert_eq!(newest(&dates, 3), vec![0, 1, 2]);
    assert_eq!(newest(&dates, POOL), pool(&dates));
    assert!(newest(&dates, 0).is_empty());
}
