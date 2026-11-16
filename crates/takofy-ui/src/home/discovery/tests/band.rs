//! How the three rows gather into the one band.

use super::super::*;
use super::test_support::*;

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
