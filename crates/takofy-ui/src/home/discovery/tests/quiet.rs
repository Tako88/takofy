//! The answers that have nothing to show, and how long they say so.

use super::super::*;
use super::test_support::*;

#[test]
fn an_empty_exhausted_ring_says_nothing_and_then_stops() {
    let mut state = playlists_only(today(), 4);
    let seed = state.next_ask().expect("the row asks");
    land_ring(&mut state, &seed, &[1, 2]);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();
    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
    let ask = state.next_dig(&owned, &hidden).expect("the press mints");

    state.land_dig(&dig(&seed, ask.hop, ring(&[], &[], true)), &owned, &hidden);

    assert_eq!(
        state.more(DiscoveryKind::Playlist, NOW),
        walk::More::Note(walk::Note::Nothing, walk::DIG_NOTE_MS),
        "an unstamped note has its whole window left"
    );
    state.stamp_landed(NOW);
    assert_eq!(
        state.more(DiscoveryKind::Playlist, NOW + walk::DIG_NOTE_MS / 2),
        walk::More::Note(walk::Note::Nothing, walk::DIG_NOTE_MS / 2)
    );
    assert_eq!(
        state.more(DiscoveryKind::Playlist, NOW + walk::DIG_NOTE_MS),
        walk::More::Done,
        "the exhausted union is the end of the line"
    );
    state.stamp_landed(NOW + walk::DIG_NOTE_MS);
    assert_eq!(state.rows[0].dig, Dig::Spent);
}

#[test]
fn an_empty_sparse_ring_goes_back_offered_and_carries_what_it_passed() {
    let mut state = playlists_only(today(), 4);
    let seed = state.next_ask().expect("the row asks");
    land_ring(&mut state, &seed, &[1, 2]);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();
    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
    let ask = state.next_dig(&owned, &hidden).expect("the press mints");

    state.land_dig(
        &dig(&seed, ask.hop, ring(&[], &[9], false)),
        &owned,
        &hidden,
    );

    assert_eq!(state.rows[0].hops, 1, "an empty ring is not a hop");
    assert_eq!(state.rows[0].passed, [id(9)]);
    assert_eq!(
        state.more(DiscoveryKind::Playlist, NOW),
        walk::More::Note(walk::Note::Sparse, walk::DIG_NOTE_MS)
    );
    state.stamp_landed(NOW);
    state.stamp_landed(NOW + walk::DIG_NOTE_MS);

    assert_eq!(
        state.more(DiscoveryKind::Playlist, NOW + walk::DIG_NOTE_MS),
        walk::More::Offered
    );
    assert!(state.press_more(DiscoveryKind::Playlist, NOW + walk::DIG_NOTE_MS));
    let ask = state
        .next_dig(&owned, &hidden)
        .expect("the next press goes deeper");
    assert_eq!(ask.hop, 2, "the same hop is walked again");
    assert_eq!(ask.frontier, [id(1), id(2)]);
    assert_eq!(
        ask.held,
        [id(1), id(2), id(9)],
        "what was passed is not tried twice"
    );
}

#[test]
fn an_unreachable_ring_says_so_and_spends_nothing() {
    let mut state = playlists_only(today(), 4);
    let seed = state.next_ask().expect("the row asks");
    land_ring(&mut state, &seed, &[1, 2]);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();
    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
    let ask = state.next_dig(&owned, &hidden).expect("the press mints");

    state.land_dig(
        &dig(&seed, ask.hop, DigOutcome::Unreachable),
        &owned,
        &hidden,
    );

    assert_eq!(state.rows[0].hops, 1, "an outage is not a hop");
    assert_eq!(state.rows[0].passed, []);
    assert_eq!(
        state.more(DiscoveryKind::Playlist, NOW),
        walk::More::Note(walk::Note::Unreachable, walk::DIG_NOTE_MS)
    );
    state.stamp_landed(NOW);
    state.stamp_landed(NOW + walk::DIG_NOTE_MS);
    assert_eq!(
        state.more(DiscoveryKind::Playlist, NOW + walk::DIG_NOTE_MS),
        walk::More::Offered
    );
    assert!(state.press_more(DiscoveryKind::Playlist, NOW + walk::DIG_NOTE_MS));
}

#[test]
fn an_off_ring_goes_straight_back_to_offered() {
    let mut state = playlists_only(today(), 4);
    let seed = state.next_ask().expect("the row asks");
    land_ring(&mut state, &seed, &[1, 2]);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();
    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
    let ask = state.next_dig(&owned, &hidden).expect("the press mints");

    state.land_dig(&dig(&seed, ask.hop, DigOutcome::Off), &owned, &hidden);

    assert_eq!(state.rows[0].dig, Dig::Offered);
    assert_eq!(
        state.more(DiscoveryKind::Playlist, NOW),
        walk::More::Offered
    );
    assert_eq!(state.rows[0].hops, 1);
}

#[test]
fn a_quiet_note_is_stamped_once_and_expires_at_one_boundary() {
    let mut state = playlists_only(today(), 4);
    let seed = state.next_ask().expect("the row asks");
    land_ring(&mut state, &seed, &[1, 2]);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();
    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
    let ask = state.next_dig(&owned, &hidden).expect("the press mints");
    state.land_dig(
        &dig(&seed, ask.hop, ring(&[], &[9], false)),
        &owned,
        &hidden,
    );

    state.stamp_landed(NOW);
    state.stamp_landed(NOW + 1_000);
    assert_eq!(
        state.rows[0].dig,
        Dig::Quiet {
            note: walk::Note::Sparse,
            at_ms: Some(NOW),
            then: After::Offered,
        },
        "the note's window starts once"
    );

    assert_eq!(
        state.more(DiscoveryKind::Playlist, NOW + walk::DIG_NOTE_MS - 1),
        walk::More::Note(walk::Note::Sparse, 1)
    );
    assert!(
        !state.press_more(DiscoveryKind::Playlist, NOW + walk::DIG_NOTE_MS - 1),
        "a note still showing is not a press"
    );
    assert_eq!(
        state.more(DiscoveryKind::Playlist, NOW + walk::DIG_NOTE_MS),
        walk::More::Offered
    );
    assert!(state.press_more(DiscoveryKind::Playlist, NOW + walk::DIG_NOTE_MS));
    let ask = state.next_dig(&owned, &hidden).expect("the press mints");

    state.land_dig(
        &dig(&seed, ask.hop, ring(&[], &[9], false)),
        &owned,
        &hidden,
    );
    state.stamp_landed(NOW);
    state.stamp_landed(NOW + walk::DIG_NOTE_MS);
    assert_eq!(state.rows[0].dig, Dig::Offered);
}
