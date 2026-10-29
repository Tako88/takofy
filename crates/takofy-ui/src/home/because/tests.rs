use super::*;

const NOW_MS: u64 = 1_700_000_000_000;

fn seed() -> Seed {
    Seed::for_open(NOW_MS, 1)
}

fn artist(uri: &str) -> FollowedArtist {
    FollowedArtist {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        portrait: None,
        releases: None,
    }
}

fn related(artist: &str, cards: Vec<FollowedArtist>) -> HomeAnswer {
    HomeAnswer::Related {
        artist: artist.to_owned(),
        cards,
    }
}

fn asked_artist(uri: &str) -> HomeAsk {
    HomeAsk::Related {
        artist: uri.to_owned(),
    }
}

#[test]
fn a_held_seed_pins_the_pick() {
    let held = seed();

    assert_eq!(pick(9, held), pick(9, held));
    assert_eq!(order(7, held), order(7, held));
}

#[test]
fn two_seeds_differ() {
    let first = Seed::for_open(NOW_MS, 1);
    let second = Seed::for_open(NOW_MS + 1, 1);

    assert_ne!(order(8, first), order(8, second));
}

#[test]
fn the_pick_draws_no_card_twice_and_never_leaves_the_first_neighbours() {
    for opens in 0..32 {
        let picked = pick(20, Seed::for_open(NOW_MS + opens, opens + 1));

        assert_eq!(picked.len(), DRAWN);
        let mut unique = picked.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), DRAWN, "a pick drew one card twice");
        assert!(
            picked.iter().all(|at| *at < NEIGHBOURS),
            "a pick reached past the artists the row may use"
        );
    }
}

#[test]
fn a_small_answer_picks_only_what_it_has() {
    let held = seed();

    assert_eq!(pick(2, held).len(), 2);
    assert!(pick(0, held).is_empty());
}

#[test]
fn the_tried_order_has_no_repeats() {
    let order = order(9, seed());

    assert_eq!(order.len(), 9);
    let mut sorted = order;
    sorted.sort_unstable();
    assert_eq!(sorted, (0..9).collect::<Vec<_>>());
    assert!(super::order(0, seed()).is_empty());
}

#[test]
fn every_artist_is_tried_once() {
    let mut state = State::default();
    state.open(seed());
    let artists: Vec<FollowedArtist> = (0..5)
        .map(|at| artist(&format!("spotify:artist:{at}")))
        .collect();

    let mut tried = Vec::new();
    while let Some(at) = state.next_artist(artists.len()) {
        assert!(!tried.contains(&at), "one artist was tried twice");
        tried.push(at);
        state.ask(&artists[at]);
    }

    assert_eq!(tried.len(), artists.len());
    assert_eq!(state.tried(), artists.len());
}

#[test]
fn a_stale_answer_is_ignored() {
    let mut state = State::default();
    state.open(seed());
    state.ask(&artist("spotify:artist:a"));

    state.land(&related(
        "spotify:artist:b",
        vec![artist("spotify:artist:z")],
    ));

    assert_eq!(
        state.asked(),
        Some("spotify:artist:a"),
        "a stale answer settled the outstanding read"
    );
    assert!(state.row().is_none(), "a stale answer drew cards");
}

#[test]
fn an_empty_answer_leaves_nothing_to_draw() {
    let mut state = State::default();
    state.open(seed());
    state.ask(&artist("spotify:artist:a"));

    state.land(&related("spotify:artist:a", Vec::new()));

    assert!(state.row().is_none());
    assert_eq!(
        state.asked(),
        None,
        "an empty answer left a read outstanding"
    );
    assert_eq!(state.tried(), 1);
}

#[test]
fn a_ready_answer_picks_from_the_first_neighbours() {
    let mut state = State::default();
    state.open(seed());
    state.ask(&artist("spotify:artist:a"));
    let cards: Vec<FollowedArtist> = (0..20)
        .map(|at| artist(&format!("spotify:artist:{at}")))
        .collect();

    state.land(&related("spotify:artist:a", cards.clone()));

    let row = state.row().expect("the answer draws a row");
    assert_eq!(row.cards.len(), DRAWN);
    let mut unique: Vec<&str> = row.cards.iter().map(|card| card.uri.as_str()).collect();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), DRAWN, "one card was picked twice");
    assert!(
        row.cards.iter().all(|card| cards[..NEIGHBOURS]
            .iter()
            .any(|first| first.uri == card.uri)),
        "the row reached past the first neighbours"
    );
}

#[test]
fn a_neighbour_list_is_filtered_before_the_pick() {
    let cards = vec![
        artist("spotify:artist:a"),
        artist("spotify:artist:b"),
        artist("spotify:artist:b"),
        artist("spotify:artist:c"),
    ];

    let kept: Vec<&str> = distinct("spotify:artist:a", &cards)
        .iter()
        .map(|card| card.uri.as_str())
        .collect();

    assert_eq!(kept, ["spotify:artist:b", "spotify:artist:c"]);
}

#[test]
fn a_row_draws_neither_the_seed_artist_nor_a_repeated_neighbour() {
    let mut state = State::default();
    state.open(seed());
    state.ask(&artist("spotify:artist:a"));
    let mut cards = vec![artist("spotify:artist:a")];
    cards.extend((0..8).map(|_| artist("spotify:artist:b")));

    state.land(&related("spotify:artist:a", cards));

    let row = state.row().expect("the one real neighbour draws a card");
    assert_eq!(row.cards.len(), 1, "a repeated neighbour drew twice");
    assert_eq!(row.cards[0].uri, "spotify:artist:b");
}

#[test]
fn a_later_answer_draws_after_one_read_failed() {
    let mut state = State::default();
    state.open(seed());
    state.ask(&artist("spotify:artist:a"));
    state.fail(&asked_artist("spotify:artist:a"), Failure::Related);
    assert!(state.row().is_none());

    state.ask(&artist("spotify:artist:b"));
    state.land(&related(
        "spotify:artist:b",
        vec![artist("spotify:artist:z")],
    ));

    let row = state.row().expect("a later answer still draws a row");
    assert_eq!(row.seed_name, "spotify:artist:b");
    assert_eq!(
        state.failure(),
        None,
        "a drawn row kept a failure it made good"
    );
}

#[test]
fn a_failed_related_read_is_kept() {
    let mut state = State::default();
    state.open(seed());
    state.ask(&artist("spotify:artist:a"));

    state.fail(&asked_artist("spotify:artist:a"), Failure::Related);

    assert_eq!(state.failure(), Some(Failure::Related));
    assert_eq!(state.asked(), None, "a failed read left an answer expected");
}

#[test]
fn another_shelfs_read_is_not_this_shelfs_failure() {
    let mut state = State::default();
    state.open(seed());

    state.fail(
        &HomeAsk::Show {
            show: "spotify:show:s".to_owned(),
        },
        Failure::Followed,
    );

    assert_eq!(state.failure(), None);
}

#[test]
fn forget_drops_what_the_open_read() {
    let mut state = State::default();
    state.open(seed());
    state.ask(&artist("spotify:artist:a"));
    state.land(&related(
        "spotify:artist:a",
        vec![artist("spotify:artist:z")],
    ));
    assert!(state.row().is_some());

    state.forget();

    assert!(state.row().is_none());
    assert_eq!(state.tried(), 0);
    assert_eq!(state.asked(), None);
    assert_eq!(state.failure(), None);
}
