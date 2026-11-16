//! What a ring does to the row it lands on.

use super::super::*;
use super::test_support::*;

#[test]
fn a_landed_ring_appends_in_order_and_moves_the_tail() {
    let mut state = playlists_only(today(), 4);
    let seed = state.next_ask().expect("the row asks");
    land_ring(&mut state, &seed, &[1, 2]);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();
    assert_eq!(state.rows[0].hops, 1);
    assert_eq!(state.rows[0].ring_start, 0);

    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
    let ask = state.next_dig(&owned, &hidden).expect("the press mints");
    state.land_dig(
        &dig(&seed, ask.hop, ring(&[3, 4], &[], false)),
        &owned,
        &hidden,
    );

    assert_eq!(
        landed_cards(&state, DiscoveryKind::Playlist),
        ["card1", "card2", "card3", "card4"]
            .map(|uri| format!("spotify:artist:{uri}"))
            .to_vec(),
        "a ring appends after the cards already drawn"
    );
    assert_eq!(state.rows[0].hops, 2);
    assert_eq!(state.rows[0].ring_start, 2);
    assert!(state.rows[0].passed.is_empty());
    assert_eq!(
        state.more(DiscoveryKind::Playlist, NOW),
        walk::More::Offered
    );

    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
    let ask = state.next_dig(&owned, &hidden).expect("the press mints");
    state.land_dig(
        &dig(&seed, ask.hop, ring(&[5], &[], false)),
        &owned,
        &hidden,
    );

    assert_eq!(state.rows[0].hops, walk::HOPS_MAX);
    assert_eq!(state.rows[0].ring_start, 4);
    assert_eq!(state.rows[0].dig, Dig::Spent);
    assert_eq!(state.more(DiscoveryKind::Playlist, NOW), walk::More::Done);
}

#[test]
fn a_ring_the_gather_drops_entirely_is_an_empty_ring() {
    let mut state = playlists_only(today(), 4);
    let seed = state.next_ask().expect("the row asks");
    land_ring(&mut state, &seed, &[1, 2]);
    let owned = owned_cards(&[9]);
    let hidden = hidden::Hidden::default();
    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
    let ask = state.next_dig(&owned, &hidden).expect("the press mints");

    state.land_dig(
        &dig(&seed, ask.hop, ring(&[9], &[], false)),
        &owned,
        &hidden,
    );

    assert_eq!(
        landed_cards(&state, DiscoveryKind::Playlist),
        ["spotify:artist:card1", "spotify:artist:card2"],
        "a card the gather drops is not appended"
    );
    assert_eq!(
        state.rows[0].passed,
        [id(9)],
        "the gather's own drops feed the next press"
    );
    assert_eq!(
        state.more(DiscoveryKind::Playlist, NOW),
        walk::More::Note(walk::Note::Sparse, walk::DIG_NOTE_MS),
        "the ring is treated as empty rather than as a hop"
    );
}

#[test]
fn a_ring_answer_is_dropped_unless_the_row_asked_for_that_hop() {
    let mut state = playlists_only(today(), 4);
    let seed = state.next_ask().expect("the row asks");
    land_ring(&mut state, &seed, &[1, 2]);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();

    state.land_dig(&dig(&seed, 2, ring(&[3], &[], false)), &owned, &hidden);
    assert_eq!(
        landed_cards(&state, DiscoveryKind::Playlist).len(),
        2,
        "a ring nobody asked for does not land"
    );

    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
    let ask = state.next_dig(&owned, &hidden).expect("the press mints");
    state.land_dig(
        &dig(&seed, ask.hop + 1, ring(&[3], &[], false)),
        &owned,
        &hidden,
    );
    assert_eq!(
        state.rows[0].dig,
        Dig::Asked,
        "the wrong hop leaves the press waiting"
    );

    let mut elsewhere = seed.clone();
    elsewhere.uri = "spotify:playlist:elsewhere".to_owned();
    state.land_dig(
        &dig(&elsewhere, ask.hop, ring(&[3], &[], false)),
        &owned,
        &hidden,
    );
    assert_eq!(state.rows[0].dig, Dig::Asked);
}

#[test]
fn a_ring_answer_after_a_re_roll_is_dropped() {
    let mut state = playlists_only(today(), 3);
    let seed = state.next_ask().expect("the row asks");
    land_ring(&mut state, &seed, &[1, 2]);
    state.stamp_landed(NOW);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();
    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
    let ask = state.next_dig(&owned, &hidden).expect("the press mints");
    let at = state.rows[0].at;

    assert!(state.reroll(DiscoveryKind::Playlist, NOW + REROLL_COOLDOWN_MS));
    assert_ne!(state.rows[0].at, at, "a re-roll only moves forward");
    let next = current(&state, DiscoveryKind::Playlist);
    assert_ne!(next.uri, seed.uri);

    state.land_dig(
        &dig(&seed, ask.hop, ring(&[3], &[], false)),
        &owned,
        &hidden,
    );

    assert!(
        matches!(state.rows[0].read, Read::Idle),
        "the stale ring did not land on the re-rolled row"
    );
    assert_eq!(state.rows[0].hops, 0, "the new seed has no ring yet");
}

#[test]
fn a_dig_never_blanks_the_row_and_the_band_grows_after_a_ring() {
    let mut state = playlists_only(today(), 4);
    let seed = state.next_ask().expect("the row asks");
    land_ring(&mut state, &seed, &[1, 2, 3, 4, 5, 6]);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();

    let Band::Rows(rows) = state.band(&owned, &hidden) else {
        panic!("the row draws");
    };
    assert_eq!(
        rows[0].cards.len(),
        RING_CARDS,
        "the first ring draws at most four"
    );

    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
    let ask = state.next_dig(&owned, &hidden).expect("the press mints");
    let Band::Rows(rows) = state.band(&owned, &hidden) else {
        panic!("a fetching dig keeps the row's cards");
    };
    assert_eq!(rows[0].read, RowRead::Ready);
    assert_eq!(rows[0].cards.len(), RING_CARDS);

    state.land_dig(
        &dig(&seed, ask.hop, ring(&[7], &[], false)),
        &owned,
        &hidden,
    );
    let Band::Rows(rows) = state.band(&owned, &hidden) else {
        panic!("the grown row draws");
    };
    assert_eq!(
        rows[0].cards.len(),
        7,
        "after a ring the row draws every card it kept"
    );
}

#[test]
fn a_one_card_row_that_grows_relays_out_as_faces() {
    let mut state = playlists_only(today(), 4);
    let seed = state.next_ask().expect("the row asks");
    land_ring(&mut state, &seed, &[1]);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();
    assert_eq!(body::body_of(&[&stamped(1)], false), body::Body::One);

    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
    let ask = state.next_dig(&owned, &hidden).expect("the press mints");
    state.land_dig(
        &dig(&seed, ask.hop, ring(&[2, 3], &[], false)),
        &owned,
        &hidden,
    );

    let Band::Rows(rows) = state.band(&owned, &hidden) else {
        panic!("the grown row draws");
    };
    assert_eq!(
        body::body_of(&rows[0].cards, false),
        body::Body::Faces { cards: 3 },
        "three cards is a face row, however it was reached"
    );
}

#[test]
fn a_ring_never_takes_a_card_another_row_holds() {
    let mut state = chosen(today(), 4);
    let mut seeds = Vec::new();
    for indexes in [&[1usize, 2][..], &[3, 4], &[5, 6]] {
        let seed = state.next_ask().expect("each row asks once");
        state.land(&answer(
            seed.clone(),
            DiscoveryOutcome::Neighbours {
                cards: stamped_cards(indexes),
                genre: None,
            },
        ));
        seeds.push(seed);
    }
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();

    assert!(state.press_more(DiscoveryKind::Artist, NOW));
    let ask = state
        .next_dig(&owned, &hidden)
        .expect("the artist row mints");
    for held in [id(1), id(2), id(3), id(4), id(5), id(6)] {
        assert!(
            ask.held.contains(&held),
            "every row's held cards are named: {held:?}"
        );
    }

    state.land_dig(
        &dig(&seeds[1], ask.hop, ring(&[1], &[], false)),
        &owned,
        &hidden,
    );

    assert_eq!(
        landed_cards(&state, DiscoveryKind::Artist),
        ["spotify:artist:card3", "spotify:artist:card4"],
        "a card the first row holds is not appended to the second"
    );
    assert_eq!(state.rows[1].passed, [id(1)]);
}
