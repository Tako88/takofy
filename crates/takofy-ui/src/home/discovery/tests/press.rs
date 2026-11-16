//! A press: when a row may dig, and the ask one press mints.

use super::super::*;
use super::test_support::*;

#[test]
fn a_press_needs_a_landed_offered_row_with_hops_left() {
    let mut state = playlists_only(today(), 4);
    assert!(
        !state.press_more(DiscoveryKind::Playlist, NOW),
        "a row that has not landed has no cards to dig from"
    );
    let seed = state.next_ask().expect("the row asks");
    land_ring(&mut state, &seed, &[1, 2]);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();

    assert!(
        state.press_more(DiscoveryKind::Playlist, NOW),
        "a landed row inside its hops may dig"
    );
    assert!(
        !state.press_more(DiscoveryKind::Playlist, NOW),
        "one dig is in flight per row"
    );
    let _ = state.next_dig(&owned, &hidden).expect("the press mints");
    assert!(
        !state.press_more(DiscoveryKind::Playlist, NOW),
        "a row whose ask is out cannot press again"
    );

    state.land_dig(&dig(&seed, 2, ring(&[3], &[], false)), &owned, &hidden);
    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
    let _ = state
        .next_dig(&owned, &hidden)
        .expect("the second press mints");
    state.land_dig(&dig(&seed, 3, ring(&[4], &[], false)), &owned, &hidden);

    assert_eq!(state.rows[0].hops, walk::HOPS_MAX);
    assert!(
        !state.press_more(DiscoveryKind::Playlist, NOW),
        "three hops from the seed is the end of the dig"
    );
    assert_eq!(state.more(DiscoveryKind::Playlist, NOW), walk::More::Done);
}

#[test]
fn next_dig_mints_one_ask_and_spends_no_day_ask() {
    let mut state = playlists_only(today(), 4);
    let seed = state.next_ask().expect("the row asks");
    land_ring(&mut state, &seed, &[1, 2]);
    let owned = owned(&[7]);
    let mut hidden = hidden::Hidden::default();
    assert!(hidden.hide("spotify:artist:8"));
    let asks = state.asks_left;
    assert!(state.press_more(DiscoveryKind::Playlist, NOW));

    let ask = state
        .next_dig(&owned, &hidden)
        .expect("the press owes one ask");

    assert_eq!(ask.hop, 2, "the first lookup is hop one");
    assert_eq!(ask.seed, seed);
    assert_eq!(ask.frontier, [id(1), id(2)]);
    assert_eq!(ask.held, [id(1), id(2)], "every card a row holds");
    for uri in [
        "spotify:artist:card1",
        "spotify:artist:card2",
        "spotify:artist:7",
        "spotify:artist:8",
    ] {
        assert!(ask.skip.contains(&uri.to_owned()), "{uri} must be skipped");
    }
    assert_eq!(
        state.asks_left, asks,
        "a dig is minted by a press, not by the day's asks"
    );
    assert_eq!(
        state.rows[0].dig,
        Dig::Asked,
        "one ask per press, however many frames read it"
    );
    assert!(
        state.next_dig(&owned, &hidden).is_none(),
        "an asked row is not asked again"
    );
}

#[test]
fn an_empty_frontier_ends_the_row_without_an_ask() {
    let mut state = playlists_only(today(), 4);
    let seed = state.next_ask().expect("the row asks");
    state.land(&answer(seed, neighbours(&[0, 1])));
    assert!(state.press_more(DiscoveryKind::Playlist, NOW));

    assert_eq!(
        state.next_dig(&fresh::Owned::default(), &hidden::Hidden::default()),
        None,
        "a card with no outside id is nothing to walk from"
    );
    assert_eq!(state.rows[0].dig, Dig::Spent);
    assert_eq!(state.more(DiscoveryKind::Playlist, NOW), walk::More::Done);
}
