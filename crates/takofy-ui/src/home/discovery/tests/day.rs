//! The pool a day is seeded from, how the rows open, and how they end.

use super::super::*;
use super::test_support::*;

#[test]
fn the_pool_is_built_once_a_day() {
    let mut state = chosen(today(), 4);
    let before = state.rows[0].pool.clone();

    state.choose(
        today(),
        &playlists(9),
        &artists(9),
        &albums(9),
        Some([2, 2, 2]),
    );

    assert_eq!(
        state.rows[0].pool, before,
        "a library change mid-day does not reshuffle the rows under the user"
    );
}

#[test]
fn the_saved_positions_start_the_rows() {
    let mut state = State::default();
    state.latch(today());
    state.open();
    state.choose(
        today(),
        &playlists(4),
        &artists(4),
        &albums(4),
        Some([2, 1, 0]),
    );

    assert_eq!(state.positions(), [2, 1, 0]);
    assert_eq!(
        current(&state, DiscoveryKind::Playlist).uri,
        state.rows[0].pool[2].uri
    );
}

#[test]
fn a_saved_position_at_or_past_the_pools_end_starts_the_row_gone() {
    let mut state = State::default();
    state.latch(today());
    state.open();
    state.choose(
        today(),
        &playlists(2),
        &artists(2),
        &albums(2),
        Some([2, 99, 2]),
    );

    assert!(matches!(state.rows[0].read, Read::Gone));
    assert!(
        matches!(state.rows[1].read, Read::Gone),
        "a position the pool cannot hold is a row that was spent"
    );
    assert_eq!(state.next_ask(), None, "a spent row is not asked");
}

#[test]
fn a_spent_row_stays_spent_across_a_restart_on_the_same_day() {
    let day = today();
    let mut state = playlists_only(day, 2);
    for _ in 0..2 {
        let seed = state.next_ask().expect("the row has two seeds");
        state.land(&answer(seed, DiscoveryOutcome::Rejected));
    }
    assert!(matches!(state.rows[0].read, Read::Gone));
    let saved = state.positions();

    let mut restarted = State::default();
    restarted.latch(day);
    restarted.open();
    restarted.choose(day, &playlists(2), &[], &[], Some(saved));

    assert!(
        matches!(restarted.rows[0].read, Read::Gone),
        "the day's row is spent, not refilled by a clamp"
    );
    assert_eq!(
        restarted.next_ask(),
        None,
        "a restart does not re-ask a spent row"
    );
}

#[test]
fn a_day_latches_across_frames_and_the_next_open_takes_the_new_one() {
    let mut state = chosen(today(), 4);
    let seed = state.next_ask().expect("the first row asks");
    state.land(&answer(seed, neighbours(&[0, 1])));
    assert!(matches!(state.rows[0].read, Read::Landed { .. }));

    state.latch(today());
    state.latch(tomorrow());

    assert!(
        matches!(state.rows[0].read, Read::Landed { .. }),
        "a page left open across midnight does not change under the user"
    );

    state.open();
    state.latch(tomorrow());

    assert!(
        matches!(state.rows[0].read, Read::Idle),
        "the new day starts idle"
    );
    assert_eq!(state.positions(), [0, 0, 0], "and starts from the top");
    assert!(
        state.rows[0].pool.is_empty(),
        "the pool waits for the new day's lists"
    );
}

#[test]
fn a_same_day_reopen_keeps_the_landed_rows() {
    let mut state = chosen(today(), 4);
    let seed = state.next_ask().expect("the first row asks");
    state.land(&answer(seed, neighbours(&[0, 1])));

    state.open();
    state.latch(today());

    assert!(
        matches!(state.rows[0].read, Read::Landed { ref cards, .. } if cards.len() == 2),
        "going home again on the same day does not re-read what already landed"
    );
}

#[test]
fn settle_leaves_a_landed_row_with_fresh_cards_alone() {
    let mut state = chosen(today(), 3);
    let seed = state.next_ask().expect("the first row asks");
    state.land(&answer(seed, neighbours(&[0])));

    state.settle([1, 0, 0]);

    assert!(matches!(state.rows[0].read, Read::Landed { .. }));
    assert_eq!(state.rows[0].reseeds_left, 1);
    assert_eq!(state.positions()[0], 0);
}

#[test]
fn settle_does_not_re_seed_a_row_that_grew() {
    let mut state = playlists_only(today(), 4);
    let seed = state.next_ask().expect("the row asks");
    land_ring(&mut state, &seed, &[1]);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();
    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
    let ask = state.next_dig(&owned, &hidden).expect("the press mints");
    state.land_dig(
        &dig(&seed, ask.hop, ring(&[2], &[], false)),
        &owned,
        &hidden,
    );
    let at = state.rows[0].at;

    state.settle([0, 0, 0]);

    assert!(matches!(state.rows[0].read, Read::Landed { .. }));
    assert_eq!(state.rows[0].at, at);
}
