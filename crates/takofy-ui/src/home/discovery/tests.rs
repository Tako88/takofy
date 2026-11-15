use takofy_model::home::{
    Day, DiscoveryAnswer, DiscoveryCard, DiscoveryKind, DiscoveryOutcome, DiscoverySeed, HomeAnswer,
};
use takofy_model::{FollowedArtist, PlaylistEntry, SavedAlbum};

use super::*;

fn today() -> Day {
    Day::from_days(19_000)
}

fn tomorrow() -> Day {
    Day::from_days(19_001)
}
const NOW: u64 = 1_700_000_000_000;

fn playlist(index: usize) -> PlaylistEntry {
    PlaylistEntry {
        uri: format!("spotify:playlist:{index}"),
        name: format!("Playlist {index}"),
        named: true,
        cover: None,
        composed: Vec::new(),
        description: None,
        tracks: Some(20),
        owner: None,
        access: None,
        saved_at: None,
    }
}

fn playlists(count: usize) -> Vec<PlaylistEntry> {
    (0..count).map(playlist).collect()
}

fn artist(index: usize) -> FollowedArtist {
    FollowedArtist {
        uri: format!("spotify:artist:{index}"),
        name: format!("Artist {index}"),
        named: true,
        portrait: None,
        releases: None,
        followed_at: None,
    }
}

fn artists(count: usize) -> Vec<FollowedArtist> {
    (0..count).map(artist).collect()
}

fn album(index: usize) -> SavedAlbum {
    SavedAlbum {
        uri: format!("spotify:album:{index}"),
        name: format!("Album {index}"),
        named: true,
        artists: format!("Artist {index}"),
        artist_uris: vec![format!("spotify:artist:{index}")],
        artist_names: vec![format!("Artist {index}")],
        year: None,
        cover: None,
        tracks: Some(10),
        saved_at: None,
    }
}

fn albums(count: usize) -> Vec<SavedAlbum> {
    (0..count).map(album).collect()
}

fn card(index: usize) -> DiscoveryCard {
    DiscoveryCard {
        artist: takofy_model::home::DiscoveryArtist {
            uri: format!("spotify:artist:card{index}"),
            name: format!("Card {index}"),
        },
        portrait: None,
        tracks: Vec::new(),
        latest_year: None,
    }
}

fn cards(indexes: &[usize]) -> Vec<DiscoveryCard> {
    indexes.iter().map(|index| card(*index)).collect()
}

fn answer(seed: DiscoverySeed, outcome: DiscoveryOutcome) -> HomeAnswer {
    HomeAnswer::Discovery(DiscoveryAnswer { seed, outcome })
}

fn neighbours(indexes: &[usize]) -> DiscoveryOutcome {
    DiscoveryOutcome::Neighbours {
        cards: cards(indexes),
        genre: None,
    }
}

/// A state that has opened on `day` with `count` usable items of every kind.
fn chosen(day: Day, count: usize) -> State {
    let mut state = State::default();
    state.latch(day);
    state.open();
    state.choose(
        day,
        &playlists(count),
        &artists(count),
        &albums(count),
        None,
    );
    state
}

/// A state that has opened on `day` with only playlists to seed from, so one
/// row is the only one that can ask.
fn playlists_only(day: Day, count: usize) -> State {
    let mut state = State::default();
    state.latch(day);
    state.open();
    state.choose(day, &playlists(count), &[], &[], None);
    state
}

/// Lands the playlist row's first answer at `now`, which is when its wait starts.
fn land_playlist(state: &mut State, now: u64) {
    let seed = state.next_ask().expect("the playlist row asks");
    state.land(&answer(seed, neighbours(&[0])));
    state.stamp_landed(now);
}

/// The seed the row of `kind` is on.
fn current(state: &State, kind: DiscoveryKind) -> DiscoverySeed {
    let row = &state.rows[kind.index()];
    row.pool[row.at].clone()
}

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
fn two_rows_over_one_neighbourhood_draw_it_once() {
    let mut state = chosen(today(), 4);
    for _ in 0..2 {
        let seed = state.next_ask().expect("each row asks once");
        state.land(&answer(seed, neighbours(&[0, 1])));
    }

    let Band::Rows(rows) = state.band(&fresh::Owned::default(), &hidden::Hidden::default()) else {
        panic!("the row with cards draws");
    };
    assert_eq!(
        rows.iter().filter(|row| row.read == RowRead::Ready).count(),
        1
    );
    assert_eq!(rows[0].seed.kind, DiscoveryKind::Playlist);
    assert_eq!(rows[0].cards.len(), 2);
    assert!(
        !rows
            .iter()
            .any(|row| row.seed.kind == DiscoveryKind::Artist),
        "a ready row with nothing the first one did not show is not drawn"
    );
}

#[test]
fn a_row_whose_cards_all_appeared_in_an_earlier_row_spends_its_re_seed() {
    let mut state = chosen(today(), 4);
    for _ in 0..2 {
        let seed = state.next_ask().expect("each row asks once");
        state.land(&answer(seed, neighbours(&[0, 1])));
    }

    let counts = state.fresh_counts(&fresh::Owned::default(), &hidden::Hidden::default());
    assert_eq!(
        counts,
        [2, 0, 0],
        "the second row offers nothing the first did not"
    );

    state.settle(counts);
    assert!(
        matches!(state.rows[1].read, Read::Idle),
        "the empty row moves on to its next seed"
    );
    assert_eq!(state.positions()[1], 1);
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
fn the_band_loads_every_row_that_has_a_seed() {
    let state = chosen(today(), 4);

    let Band::Rows(rows) = state.band(&fresh::Owned::default(), &hidden::Hidden::default()) else {
        panic!("three seeded rows draw");
    };

    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row.read == RowRead::Loading));
    assert_eq!(rows[0].seed.kind, DiscoveryKind::Playlist);
    assert_eq!(rows[1].seed.kind, DiscoveryKind::Artist);
    assert_eq!(rows[2].seed.kind, DiscoveryKind::Album);
}

#[test]
fn the_band_keeps_one_card_per_artist_across_rows() {
    let mut state = chosen(today(), 4);
    for (row, indexes) in [[0usize, 1], [1, 2], [2, 3]].iter().enumerate() {
        let seed = state.next_ask().expect("each row asks once");
        let genre = (row == 0).then(|| "dubstep".to_owned());
        state.land(&answer(
            seed,
            DiscoveryOutcome::Neighbours {
                cards: cards(indexes),
                genre,
            },
        ));
    }

    let Band::Rows(rows) = state.band(&fresh::Owned::default(), &hidden::Hidden::default()) else {
        panic!("three landed rows draw");
    };

    let uris: Vec<Vec<&str>> = rows
        .iter()
        .map(|row| {
            row.cards
                .iter()
                .map(|card| card.artist.uri.as_str())
                .collect()
        })
        .collect();
    assert_eq!(
        uris,
        [
            vec!["spotify:artist:card0", "spotify:artist:card1"],
            vec!["spotify:artist:card2"],
            vec!["spotify:artist:card3"],
        ],
        "the first row that showed an artist keeps it"
    );
    assert!(rows.iter().all(|row| row.read == RowRead::Ready));
    assert_eq!(rows[0].genre, Some("dubstep"));
    assert_eq!(rows[1].genre, None);
}

#[test]
fn an_unreachable_row_draws_as_unreachable() {
    let mut state = chosen(today(), 1);
    let mut seeds = Vec::new();
    while let Some(seed) = state.next_ask() {
        seeds.push(seed);
    }
    for seed in seeds {
        state.land(&answer(seed, DiscoveryOutcome::Unreachable));
    }

    assert_eq!(
        state.band(&fresh::Owned::default(), &hidden::Hidden::default()),
        Band::Unreachable
    );
}

#[test]
fn no_row_that_is_here_draws_as_absent() {
    let mut state = State::default();

    assert_eq!(
        state.band(&fresh::Owned::default(), &hidden::Hidden::default()),
        Band::Absent,
        "no day, no pool, no band"
    );

    state = chosen(today(), 0);
    assert_eq!(
        state.band(&fresh::Owned::default(), &hidden::Hidden::default()),
        Band::Absent,
        "a library with nothing to seed from"
    );
}

#[test]
fn the_four_answers_are_distinct() {
    assert_ne!(Band::Off, Band::Waiting);
    assert_ne!(Band::Waiting, Band::Absent);
    assert_ne!(Band::Absent, Band::Unreachable);
    assert_ne!(Band::Off, Band::Unreachable);
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
