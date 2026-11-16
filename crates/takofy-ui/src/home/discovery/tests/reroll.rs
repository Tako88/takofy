//! A row's re-roll: when it may move on, and the wait it draws.

use super::super::*;
use super::test_support::*;

#[test]
fn reroll_moves_forward_within_the_pool_and_waits_out_the_cooldown() {
    let mut state = playlists_only(today(), 3);
    let reseeds = state.rows[0].reseeds_left;
    land_playlist(&mut state, NOW);

    assert!(
        state.reroll(DiscoveryKind::Playlist, NOW + REROLL_COOLDOWN_MS),
        "the wait is over once its two seconds are"
    );
    assert_eq!(state.positions()[0], 1);
    assert!(
        !state.reroll(DiscoveryKind::Playlist, NOW + REROLL_COOLDOWN_MS),
        "the answer the re-roll asked for is still owed, so it cannot be asked again"
    );
    land_playlist(&mut state, NOW + REROLL_COOLDOWN_MS);
    assert!(
        !state.reroll(DiscoveryKind::Playlist, NOW + REROLL_COOLDOWN_MS),
        "the new answer's own wait has only just started"
    );
    assert!(!state.reroll(DiscoveryKind::Playlist, NOW + 2 * REROLL_COOLDOWN_MS - 1));
    assert!(state.reroll(DiscoveryKind::Playlist, NOW + 2 * REROLL_COOLDOWN_MS));
    assert_eq!(state.positions()[0], 2);
    assert!(
        !state.reroll(DiscoveryKind::Playlist, NOW + 4 * REROLL_COOLDOWN_MS),
        "the end of the pool is as far as a re-roll goes"
    );
    assert_eq!(state.positions()[0], state.rows[0].pool.len() - 1);
    assert_eq!(
        state.rows[0].reseeds_left, reseeds,
        "no re-roll spends a re-seed"
    );
}

#[test]
fn a_re_roll_with_no_ask_left_is_refused() {
    let mut state = playlists_only(today(), 3);
    let seed = state.next_ask().expect("the first row asks");
    state.land(&answer(seed, neighbours(&[0])));
    state.asks_left = 0;

    assert!(
        !state.reroll(DiscoveryKind::Playlist, NOW),
        "a re-roll that nothing could ask would leave the row loading"
    );
    assert!(
        matches!(state.rows[0].read, Read::Landed { .. }),
        "the row keeps the cards it has"
    );
    assert_eq!(state.positions()[0], 0, "and stays where it is");
}

#[test]
fn reroll_state_reports_the_remaining_cooldown_and_agrees_with_reroll() {
    let mut state = playlists_only(today(), 3);
    assert_eq!(
        state.reroll_state(DiscoveryKind::Playlist, NOW),
        Reroll::Wait(REROLL_COOLDOWN_MS),
        "a row with no answer yet has no wait to give"
    );
    assert!(!state.can_reroll(DiscoveryKind::Playlist, NOW));

    land_playlist(&mut state, NOW);
    assert_eq!(
        state.reroll_state(DiscoveryKind::Playlist, NOW),
        Reroll::Wait(REROLL_COOLDOWN_MS),
        "the wait starts at the answer, not at the press"
    );
    assert_eq!(
        state.reroll_state(DiscoveryKind::Playlist, NOW + 4),
        Reroll::Wait(REROLL_COOLDOWN_MS - 4)
    );
    assert!(!state.can_reroll(DiscoveryKind::Playlist, NOW + 4));
    assert!(!state.reroll(DiscoveryKind::Playlist, NOW + 4));

    assert_eq!(
        state.reroll_state(DiscoveryKind::Playlist, NOW + REROLL_COOLDOWN_MS),
        Reroll::Ready
    );
    assert!(state.reroll(DiscoveryKind::Playlist, NOW + REROLL_COOLDOWN_MS));
}

#[test]
fn reroll_state_is_never_with_no_ask_left() {
    let mut state = playlists_only(today(), 3);
    state.asks_left = 0;

    assert_eq!(
        state.reroll_state(DiscoveryKind::Playlist, NOW),
        Reroll::Never
    );
    assert!(!state.can_reroll(DiscoveryKind::Playlist, NOW));
    assert!(!state.reroll(DiscoveryKind::Playlist, NOW));
}

#[test]
fn reroll_state_is_never_at_the_pools_end() {
    let mut state = playlists_only(today(), 1);

    assert_eq!(
        state.reroll_state(DiscoveryKind::Playlist, NOW),
        Reroll::Never
    );
    assert!(!state.reroll(DiscoveryKind::Playlist, NOW));
}

#[test]
fn the_wait_draws_from_empty_at_the_answer_to_whole_as_it_ends() {
    let mut state = playlists_only(today(), 3);
    assert_eq!(
        state.reroll_state(DiscoveryKind::Playlist, NOW).waited(),
        Some(0.0),
        "its answer is still owed, so the edge has not started"
    );
    land_playlist(&mut state, NOW);

    let at = |elapsed: u64| {
        state
            .reroll_state(DiscoveryKind::Playlist, NOW + elapsed)
            .waited()
            .expect("the cooldown is running")
    };
    assert!(at(0) <= 0.001, "the answer starts the edge at nothing");
    assert!((at(REROLL_COOLDOWN_MS / 2) - 0.5).abs() < 0.01);
    assert!(at(REROLL_COOLDOWN_MS - 1) > 0.99);
    assert_eq!(
        state
            .reroll_state(DiscoveryKind::Playlist, NOW + REROLL_COOLDOWN_MS)
            .waited(),
        None,
        "a row that is ready has nothing left to fill"
    );
    assert_eq!(Reroll::Never.waited(), None, "a spent row promises nothing");
}

#[test]
fn the_days_draws_end_a_row_before_its_pool_does() {
    let mut state = playlists_only(today(), 30);
    land_playlist(&mut state, NOW);

    let mut now = NOW;
    for expected in 2..=PER_KIND {
        now += REROLL_COOLDOWN_MS;
        assert!(
            state.reroll(DiscoveryKind::Playlist, now),
            "a press inside the day's draws"
        );
        land_playlist(&mut state, now);
        assert_eq!(
            state.rows[0].drawn as usize, expected,
            "a press lands one draw"
        );
    }

    assert!(
        state.positions()[0] < 2 * EDGE,
        "the pool still held seeds when the day ran out: {:?}",
        state.positions()[0]
    );
    assert_eq!(
        state.reroll_state(DiscoveryKind::Playlist, now + 100 * REROLL_COOLDOWN_MS),
        Reroll::Never,
        "a spent row offers no further draw"
    );
    assert!(state.next_ask().is_none(), "and asks for nothing more");
}
