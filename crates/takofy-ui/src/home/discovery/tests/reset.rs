//! What starts a row's dig over.

use super::super::*;
use super::test_support::*;

#[test]
fn a_ring_one_landing_resets_the_dig_and_a_re_roll_starts_it_over() {
    let mut state = playlists_only(today(), 4);
    let seed = state.next_ask().expect("the row asks");
    land_ring(&mut state, &seed, &[1, 2]);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();
    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
    let ask = state.next_dig(&owned, &hidden).expect("the press mints");
    state.land_dig(
        &dig(&seed, ask.hop, ring(&[3], &[], false)),
        &owned,
        &hidden,
    );
    assert_eq!(state.rows[0].hops, 2);

    state.stamp_landed(NOW);
    assert!(state.reroll(DiscoveryKind::Playlist, NOW + REROLL_COOLDOWN_MS));
    assert_eq!(state.rows[0].hops, 0);
    assert_eq!(state.rows[0].ring_start, 0);
    assert!(state.rows[0].passed.is_empty());
    assert_eq!(state.rows[0].dig, Dig::Offered);

    let seed = state.next_ask().expect("the re-rolled row asks");
    land_ring(&mut state, &seed, &[4, 5]);
    assert_eq!(state.rows[0].hops, 1);
    assert_eq!(state.rows[0].ring_start, 0);
    assert_eq!(state.rows[0].dig, Dig::Offered);
    assert_eq!(
        landed_cards(&state, DiscoveryKind::Playlist).len(),
        2,
        "a new seed replaces the row's cards"
    );
}

#[test]
fn a_rejected_seed_starts_the_dig_over() {
    let mut state = playlists_only(today(), 4);
    let seed = state.next_ask().expect("the row asks");
    land_ring(&mut state, &seed, &[1, 2]);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();
    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
    let ask = state.next_dig(&owned, &hidden).expect("the press mints");
    state.land_dig(
        &dig(&seed, ask.hop, ring(&[3], &[], false)),
        &owned,
        &hidden,
    );

    state.stamp_landed(NOW);
    state.reroll(DiscoveryKind::Playlist, NOW + REROLL_COOLDOWN_MS);
    let seed = state.next_ask().expect("the next seed asks");
    state.land(&answer(seed, DiscoveryOutcome::Rejected));

    assert_eq!(state.rows[0].hops, 0);
    assert_eq!(state.rows[0].ring_start, 0);
    assert!(state.rows[0].passed.is_empty());
    assert_eq!(state.rows[0].dig, Dig::Offered);
}

#[test]
fn opening_turns_a_dig_in_flight_back_offered_and_keeps_the_cards() {
    let mut state = playlists_only(today(), 4);
    let seed = state.next_ask().expect("the row asks");
    land_ring(&mut state, &seed, &[1, 2]);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();
    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
    let ask = state.next_dig(&owned, &hidden).expect("the press mints");
    state.land_dig(
        &dig(&seed, ask.hop, ring(&[3], &[], false)),
        &owned,
        &hidden,
    );
    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
    assert_eq!(state.rows[0].dig, Dig::Wanted);

    state.open();

    assert_eq!(
        state.rows[0].dig,
        Dig::Offered,
        "a press whose answer can no longer come is not left fetching"
    );
    assert_eq!(state.rows[0].hops, 2, "the cards it grew keep their place");
}
