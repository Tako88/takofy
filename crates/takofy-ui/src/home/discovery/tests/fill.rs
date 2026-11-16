//! A row that lands too thin to draw fills itself, without the reader's press.

use super::super::*;
use super::test_support::*;

/// Lands the playlist row's first answer with `indexes` and settles it, which
/// is what the frame does after a landing.
fn land_thin(state: &mut State, indexes: &[usize]) -> DiscoverySeed {
    let seed = state.next_ask().expect("the row asks");
    state.land(&answer(
        seed.clone(),
        DiscoveryOutcome::Neighbours {
            cards: stamped_cards(indexes),
            genre: None,
        },
    ));
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();
    let counts = state.fresh_counts(&owned, &hidden);
    state.settle(counts);
    seed
}

/// The one ring the row's fill owes, minted and marked as out.
fn fill_ring(state: &mut State, owned: &fresh::Owned, hidden: &hidden::Hidden) -> DigAsk {
    let ask = state.next_dig(owned, hidden).expect("the fill mints one");
    state.land_dig(
        &dig(&ask.seed, ask.hop, ring(&[], &[], false)),
        owned,
        hidden,
    );
    ask
}

#[test]
fn a_row_too_thin_to_draw_fills_itself_before_it_is_offered() {
    let mut state = playlists_only(today(), 4);
    let seed = land_thin(&mut state, &[1, 2]);

    assert_eq!(
        state.rows[0].dig,
        Dig::Wanted,
        "two cards is not a row to offer a press on"
    );
    assert_eq!(state.rows[0].fills, 1, "the fill is the row's own");
    assert_eq!(
        state.more(DiscoveryKind::Playlist, NOW),
        walk::More::Fetching,
        "the line stays inert for the whole fill"
    );
    assert_eq!(state.rows[0].hops, 1, "a fill is not a press from the seed");

    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();
    let ask = state
        .next_dig(&owned, &hidden)
        .expect("the fill owes one ring");
    assert_eq!(ask.seed, seed);
    assert_eq!(ask.hop, 2, "the fill walks the ring the press would have");
    assert_eq!(ask.frontier, [id(1), id(2)]);
    let Band::Rows(rows) = state.band(&owned, &hidden) else {
        panic!("a filling row keeps drawing");
    };
    assert_eq!(rows[0].read, RowRead::Ready);
    assert_eq!(
        rows[0].cards.len(),
        2,
        "the row is drawable throughout the fill"
    );
}

#[test]
fn a_fill_ring_that_finds_nothing_says_nothing_and_fills_again() {
    let mut state = playlists_only(today(), 4);
    land_thin(&mut state, &[1, 2]);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();

    fill_ring(&mut state, &owned, &hidden);

    assert_eq!(
        landed_cards(&state, DiscoveryKind::Playlist).len(),
        2,
        "an empty ring appends nothing"
    );
    assert_eq!(state.rows[0].passed, []);
    assert_eq!(state.rows[0].fills, 2, "the empty ring is the second fill");
    assert_eq!(
        state.more(DiscoveryKind::Playlist, NOW),
        walk::More::Fetching,
        "a fill never shows a note"
    );
}

#[test]
fn a_row_that_has_spent_both_fill_rings_is_offered_where_it_lands() {
    let mut state = playlists_only(today(), 4);
    land_thin(&mut state, &[1, 2]);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();
    fill_ring(&mut state, &owned, &hidden);

    let ask = state.next_dig(&owned, &hidden).expect("the second fill");
    state.land_dig(
        &dig(&ask.seed, ask.hop, ring(&[], &[], false)),
        &owned,
        &hidden,
    );

    assert_eq!(state.rows[0].fills, FILL_RINGS);
    assert_eq!(
        state.rows[0].dig,
        Dig::Offered,
        "two cards is the honest answer once the fills are spent"
    );
    assert_eq!(
        state.more(DiscoveryKind::Playlist, NOW),
        walk::More::Offered,
        "a spent fill is not a note"
    );
    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
}

#[test]
fn a_filled_row_still_has_its_two_presses() {
    let mut state = playlists_only(today(), 4);
    land_thin(&mut state, &[1, 2]);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();

    for index in [3, 4] {
        let ask = state.next_dig(&owned, &hidden).expect("the fill mints");
        state.land_dig(
            &dig(&ask.seed, ask.hop, ring(&[index], &[], false)),
            &owned,
            &hidden,
        );
    }

    assert_eq!(state.rows[0].fills, FILL_RINGS);
    assert_eq!(state.rows[0].hops, 1, "the fills spent no hop");
    assert_eq!(
        landed_cards(&state, DiscoveryKind::Playlist).len(),
        RING_CARDS,
        "the row filled itself to the cards it draws"
    );

    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
    let ask = state.next_dig(&owned, &hidden).expect("the first press");
    assert_eq!(ask.hop, 2);
    state.land_dig(
        &dig(&ask.seed, ask.hop, ring(&[5], &[], false)),
        &owned,
        &hidden,
    );
    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
    let ask = state.next_dig(&owned, &hidden).expect("the second press");
    assert_eq!(ask.hop, walk::HOPS_MAX);
    state.land_dig(
        &dig(&ask.seed, ask.hop, ring(&[6], &[], false)),
        &owned,
        &hidden,
    );

    assert_eq!(state.rows[0].hops, walk::HOPS_MAX);
    assert_eq!(state.rows[0].dig, Dig::Spent, "two presses, then the end");
}

#[test]
fn a_union_that_runs_out_is_not_filled_further() {
    let mut state = playlists_only(today(), 4);
    land_thin(&mut state, &[1, 2]);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();
    let ask = state.next_dig(&owned, &hidden).expect("the fill mints");

    state.land_dig(
        &dig(&ask.seed, ask.hop, ring(&[3], &[], true)),
        &owned,
        &hidden,
    );

    assert_eq!(
        landed_cards(&state, DiscoveryKind::Playlist).len(),
        3,
        "what the ring found is kept"
    );
    let counts = state.fresh_counts(&owned, &hidden);
    state.settle(counts);

    assert_eq!(
        state.rows[0].dig,
        Dig::Offered,
        "there is nothing left to walk"
    );
    assert_eq!(state.rows[0].fills, 1, "the exhausted union fills no more");
}

#[test]
fn an_exhausted_fill_ring_ends_the_row_without_a_note() {
    let mut state = playlists_only(today(), 4);
    land_thin(&mut state, &[1, 2]);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();
    let ask = state.next_dig(&owned, &hidden).expect("the fill mints");

    state.land_dig(
        &dig(&ask.seed, ask.hop, ring(&[], &[], true)),
        &owned,
        &hidden,
    );

    assert_eq!(state.rows[0].dig, Dig::Spent);
    assert_eq!(
        state.more(DiscoveryKind::Playlist, NOW),
        walk::More::Done,
        "an exhausted fill says nothing and stops"
    );
}

#[test]
fn a_row_fills_on_the_cards_it_keeps_not_on_the_ones_it_found() {
    let mut state = playlists_only(today(), 4);
    let seed = state.next_ask().expect("the row asks");
    state.land(&answer(
        seed,
        DiscoveryOutcome::Neighbours {
            cards: stamped_cards(&[1, 2, 3, 4, 5]),
            genre: None,
        },
    ));
    let owned = owned_cards(&[4, 5]);
    let hidden = hidden::Hidden::default();

    let counts = state.fresh_counts(&owned, &hidden);
    assert_eq!(counts, [3, 0, 0], "five found, three the reader can see");
    state.settle(counts);

    assert_eq!(
        state.rows[0].dig,
        Dig::Wanted,
        "the row is drawn with three cards, so it fills"
    );
}

#[test]
fn a_row_that_lands_full_is_offered_at_once() {
    let mut state = playlists_only(today(), 4);
    let seed = state.next_ask().expect("the row asks");
    state.land(&answer(
        seed,
        DiscoveryOutcome::Neighbours {
            cards: stamped_cards(&[1, 2, 3, 4]),
            genre: None,
        },
    ));

    state.settle([RING_CARDS, 0, 0]);

    assert_eq!(state.rows[0].dig, Dig::Offered);
    assert_eq!(state.rows[0].fills, 0, "a full row needs no fill");
    assert_eq!(
        state.more(DiscoveryKind::Playlist, NOW),
        walk::More::Offered
    );
}

#[test]
fn a_press_that_finds_nothing_is_not_filled_behind_its_note() {
    let mut state = playlists_only(today(), 4);
    let seed = state.next_ask().expect("the row asks");
    land_ring(&mut state, &seed, &[1, 2]);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();
    assert!(state.press_more(DiscoveryKind::Playlist, NOW));
    let ask = state.next_dig(&owned, &hidden).expect("the press mints");
    state.land_dig(
        &dig(&ask.seed, ask.hop, ring(&[], &[9], false)),
        &owned,
        &hidden,
    );

    let counts = state.fresh_counts(&owned, &hidden);
    state.settle(counts);

    assert_eq!(
        state.rows[0].dig,
        Dig::Quiet {
            note: walk::Note::Sparse,
            at_ms: None,
            then: After::Offered
        },
        "the note says what the press found"
    );
    state.stamp_landed(NOW);
    state.stamp_landed(NOW + walk::DIG_NOTE_MS);
    let counts = state.fresh_counts(&owned, &hidden);
    state.settle(counts);

    assert_eq!(state.rows[0].fills, 0, "a press is not a landing to fill");
    assert_eq!(
        state.rows[0].dig,
        Dig::Offered,
        "a frame never fills a press"
    );
    assert!(
        state.next_dig(&owned, &hidden).is_none(),
        "the row asks nothing of its own after the note"
    );
}

#[test]
fn a_fill_the_wire_cannot_answer_is_not_asked_again() {
    let mut state = playlists_only(today(), 4);
    land_thin(&mut state, &[1, 2]);
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();
    let ask = state.next_dig(&owned, &hidden).expect("the fill mints");

    state.land_dig(&dig(&ask.seed, ask.hop, DigOutcome::Off), &owned, &hidden);
    let counts = state.fresh_counts(&owned, &hidden);
    state.settle(counts);

    assert_eq!(state.rows[0].dig, Dig::Offered);
    assert_eq!(state.rows[0].fills, 1, "the fill still spent itself");
    assert!(
        state.next_dig(&owned, &hidden).is_none(),
        "a disabled feature is not re-asked by a frame"
    );
}

#[test]
fn a_fill_spends_no_day_ask() {
    let mut state = playlists_only(today(), 4);
    land_thin(&mut state, &[1, 2]);
    let asks = state.asks_left;
    let owned = fresh::Owned::default();
    let hidden = hidden::Hidden::default();

    fill_ring(&mut state, &owned, &hidden);
    fill_ring(&mut state, &owned, &hidden);

    assert_eq!(
        state.asks_left, asks,
        "a fill is minted by the row, not by the day's asks"
    );
}

#[test]
fn a_new_seed_starts_with_no_fills() {
    let mut state = playlists_only(today(), 3);
    land_thin(&mut state, &[1, 2]);
    state.stamp_landed(NOW);
    assert_eq!(state.rows[0].fills, 1);

    assert!(state.reroll(DiscoveryKind::Playlist, NOW + REROLL_COOLDOWN_MS));

    assert_eq!(state.rows[0].fills, 0);
    assert!(!state.rows[0].filling, "no fill is in hand on a new seed");
}
