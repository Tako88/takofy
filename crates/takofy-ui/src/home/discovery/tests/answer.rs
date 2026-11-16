//! What a row does with the answer to its seed.

use super::super::*;
use super::test_support::*;

#[test]
fn a_stale_answer_is_dropped() {
    let mut state = chosen(today(), 4);
    let seed = state.next_ask().expect("the first row asks");
    let mut stale = seed.clone();
    stale.uri = "spotify:playlist:not-the-one".to_owned();

    state.land(&answer(stale, neighbours(&[0])));

    assert!(
        matches!(state.rows[0].read, Read::Asked),
        "the row keeps waiting for the answer to the seed it asked"
    );
    assert!(
        matches!(
            state.band(&fresh::Owned::default(), &hidden::Hidden::default()),
            Band::Rows(_)
        ),
        "and the band keeps loading"
    );
}

#[test]
fn an_answer_to_a_row_that_is_not_asking_is_dropped() {
    let mut state = chosen(today(), 4);
    let seed = current(&state, DiscoveryKind::Playlist);

    state.land(&answer(seed.clone(), neighbours(&[0])));
    assert!(
        matches!(state.rows[0].read, Read::Idle),
        "an answer before the ask changes nothing"
    );

    let asked = state.next_ask().expect("the first row asks");
    state.land(&answer(asked.clone(), DiscoveryOutcome::Off));
    assert!(matches!(state.rows[0].read, Read::Idle));

    state.land(&answer(asked, neighbours(&[0])));
    assert!(
        matches!(state.rows[0].read, Read::Idle),
        "an answer after the row has gone back to idle is stale too"
    );
}

#[test]
fn an_answer_lands_on_its_own_row_and_no_other() {
    let mut state = chosen(today(), 4);
    let playlist = state.next_ask().expect("the first row asks");
    let mut mismatched = playlist.clone();
    mismatched.kind = DiscoveryKind::Artist;

    state.land(&answer(mismatched, neighbours(&[0])));

    assert!(
        matches!(state.rows[0].read, Read::Asked),
        "the answer names another kind's row, so the asking row is still waiting"
    );
    assert!(matches!(state.rows[1].read, Read::Idle));
    assert_eq!(current(&state, DiscoveryKind::Playlist), playlist);
}

#[test]
fn off_never_lands_and_never_advances() {
    let mut state = chosen(today(), 4);
    let seed = state.next_ask().expect("the first row asks");
    let before = state.positions();

    state.land(&answer(seed.clone(), DiscoveryOutcome::Off));

    assert!(matches!(state.rows[0].read, Read::Idle));
    assert_eq!(state.positions(), before, "an off setting moves no row");
    assert_eq!(
        state.next_ask(),
        Some(seed),
        "the row asks the same seed again"
    );
}

#[test]
fn unreachable_retries_after_open_without_spending_the_reseed() {
    let mut state = playlists_only(today(), 4);
    let seed = state.next_ask().expect("the first row asks");
    let before = state.positions();
    let reseeds = state.rows[0].reseeds_left;

    state.land(&answer(seed.clone(), DiscoveryOutcome::Unreachable));

    assert!(matches!(state.rows[0].read, Read::Unreachable));
    assert_eq!(state.positions(), before, "an outage moves no row");
    assert_eq!(state.rows[0].reseeds_left, reseeds, "and spends no re-seed");
    assert_eq!(
        state.next_ask(),
        None,
        "an unreachable row waits for the next open instead of retrying every frame"
    );

    state.open();

    assert_eq!(state.next_ask(), Some(seed), "the next open tries it again");
}

#[test]
fn a_rejected_seed_costs_one_step_and_the_pool_bounds_it() {
    let mut state = playlists_only(today(), 2);
    let first = state.next_ask().expect("the first row asks");

    state.land(&answer(first.clone(), DiscoveryOutcome::Rejected));

    assert_eq!(state.positions()[0], 1, "a rejected seed moves the row on");
    assert_eq!(state.rows[0].reseeds_left, 1, "and spends no re-seed");
    let second = state.next_ask().expect("the next seed is asked");
    assert_ne!(second, first);

    state.land(&answer(second, DiscoveryOutcome::Rejected));

    assert!(matches!(state.rows[0].read, Read::Gone));
    assert_eq!(
        state.positions()[0],
        state.rows[0].pool.len(),
        "the pool is what bounds the row, so there is no third seed"
    );
    assert_eq!(state.next_ask(), None, "a gone row is never asked");
}

#[test]
fn an_empty_row_spends_one_reseed_and_then_goes() {
    let mut state = playlists_only(today(), 3);

    let seed = state.next_ask().expect("the first row asks");
    state.land(&answer(seed, neighbours(&[0, 1])));
    state.settle([0, 0, 0]);

    assert_eq!(
        state.rows[0].reseeds_left, 0,
        "the empty row spends its re-seed"
    );
    assert_eq!(state.positions()[0], 1, "on the next seed");
    assert!(matches!(state.rows[0].read, Read::Idle));

    let seed = state.next_ask().expect("the re-seed is asked");
    state.land(&answer(seed, neighbours(&[0, 1])));
    state.settle([0, 0, 0]);

    assert!(
        matches!(state.rows[0].read, Read::Gone),
        "the second empty answer ends the row rather than looping"
    );
    assert_eq!(state.next_ask(), None);
}

#[test]
fn an_ask_goes_out_once() {
    let mut state = chosen(today(), 4);
    let mut asked = Vec::new();

    for _ in 0..1_000 {
        if let Some(seed) = state.next_ask() {
            asked.push(seed);
        }
    }

    assert_eq!(
        asked.len(),
        DiscoveryKind::ALL.len(),
        "one ask per row, however many frames read the band without an answer"
    );
    assert_eq!(
        asked.iter().map(|seed| seed.kind).collect::<Vec<_>>(),
        DiscoveryKind::ALL,
        "the rows are asked in kind order"
    );
    assert_eq!(state.asks_left, ASKS_PER_OPEN - DiscoveryKind::ALL.len());
}

#[test]
fn a_rejected_seed_costs_a_walk_but_not_a_draw() {
    // Thirty playlists give a pool twice the edge, which is more walks than a
    // row's draws: that is what leaves room for the rejections.
    let mut state = playlists_only(today(), 30);
    for _ in 0..3 {
        let seed = state.next_ask().expect("the row asks");
        state.land(&answer(seed, DiscoveryOutcome::Rejected));
    }
    assert_eq!(state.rows[0].drawn, 0, "a rejection draws nothing");
    assert_eq!(state.positions()[0], 3, "but it does walk the row on");
    assert!(!state.rows[0].spent());

    let seed = state.next_ask().expect("the row asks after the rejections");
    state.land(&answer(seed, neighbours(&[0])));
    assert_eq!(state.rows[0].drawn, 1, "the landing is the draw");
}
