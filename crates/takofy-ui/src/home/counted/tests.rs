use super::*;
use takofy_model::TrackEntry;

/// One holding, named after its own artist.
fn held(artist: &str) -> Holding<'_> {
    Holding {
        artist,
        name: artist,
    }
}

/// `count` holdings of `artist`, as a library that names it on that many items.
fn held_many(artist: &'static str, count: usize) -> Vec<Holding<'static>> {
    (0..count).map(|_| held(artist)).collect()
}

/// A track row that names `artists`, as a label read answers one.
fn labelled(uri: &str, artists: &[(&str, &str)]) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: uri.to_owned(),
        artists: String::new(),
        album: None,
        album_uri: None,
        artist_uris: artists
            .iter()
            .map(|(artist, _)| (*artist).to_owned())
            .collect(),
        artist_names: artists.iter().map(|(_, name)| (*name).to_owned()).collect(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

/// A track row that fell back to its own URI.
fn unreadable(uri: &str) -> TrackEntry {
    TrackEntry {
        title: uri.to_owned(),
        named: false,
        ..labelled(uri, &[])
    }
}

/// A seed taken from one open of the page.
fn seed() -> Seed {
    Seed::for_open(1_700_000_000_000, 3)
}

#[test]
fn the_most_held_artist_ranks_first() {
    let holdings = [held("b"), held("a"), held("a"), held("c")];
    let ranked = rank(&holdings);
    assert_eq!(
        ranked.iter().map(|artist| artist.uri).collect::<Vec<_>>(),
        vec!["a", "b", "c"]
    );
}

#[test]
fn a_tie_keeps_the_order_the_library_gave() {
    let holdings = [held("a"), held("b"), held("c"), held("b")];
    let ranked = rank(&holdings);
    assert_eq!(
        ranked.iter().map(|artist| artist.uri).collect::<Vec<_>>(),
        vec!["b", "a", "c"]
    );
}

#[test]
fn an_artist_named_twice_on_one_item_is_one_holding() {
    let holdings = one_each([("spotify:artist:a", "A"), ("spotify:artist:a", "A")]);
    assert_eq!(
        holdings,
        vec![Holding {
            artist: "spotify:artist:a",
            name: "A"
        }]
    );
}

#[test]
fn a_second_artist_on_the_same_item_is_its_own_holding() {
    let holdings = one_each([("spotify:artist:a", "A"), ("spotify:artist:b", "B")]);
    assert_eq!(holdings.len(), 2);
    let ranked = rank(&holdings);
    assert_eq!(ranked.len(), 2, "each artist is ranked once");
}

#[test]
fn an_artist_on_two_items_outranks_an_artist_on_one() {
    let holdings = [held("a"), held("b"), held("b")];
    assert_eq!(rank(&holdings)[0].uri, "b");
}

#[test]
fn the_ranking_reads_every_holding_not_the_newest_five_hundred() {
    // Twenty-five artists hold twenty items each among the newest five hundred;
    // only the oldest hundred name the artist the library holds most of. A
    // ranking built from recent saves alone would not have it at all.
    let mut holdings = Vec::new();
    for name in [
        "a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8", "a9", "a10", "a11", "a12", "a13", "a14",
        "a15", "a16", "a17", "a18", "a19", "a20", "a21", "a22", "a23", "a24", "a25",
    ] {
        holdings.extend(held_many(name, 20));
    }
    holdings.extend(held_many("deep", 100));
    assert_eq!(holdings.len(), 600, "five hundred recent, one hundred old");

    let ranked = rank(&holdings);
    assert_eq!(ranked.len(), 26);
    assert_eq!(ranked[0].uri, "deep");
}

#[test]
fn the_pick_never_leaves_the_top_pool() {
    for ranked_len in [0, 1, 5, POOL, POOL + 1, 40, 1_000] {
        for open in 0..8 {
            let drawn = pick(ranked_len, Seed::for_open(1_700_000_000_000, open));
            let top = ranked_len.min(POOL);
            assert!(
                drawn.iter().all(|at| *at < top),
                "a pick of {ranked_len} left the top {top}: {drawn:?}"
            );
        }
    }
}

#[test]
fn the_pick_is_ascending_and_has_no_repeats() {
    for ranked_len in [0, 1, 5, POOL, 40] {
        for open in 0..8 {
            let drawn = pick(ranked_len, Seed::for_open(1_700_000_000_000, open));
            let mut sorted = drawn.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(drawn, sorted, "the draw is ascending and distinct");
            assert_eq!(
                drawn.len(),
                ranked_len.min(POOL).min(DRAWN),
                "the draw takes as many as it has room for"
            );
        }
    }
}

#[test]
fn a_held_seed_draws_the_same_positions() {
    assert_eq!(pick(40, seed()), pick(40, seed()));
}

#[test]
fn another_open_draws_other_positions() {
    let first = pick(40, Seed::for_open(1_700_000_000_000, 3));
    let second = pick(40, Seed::for_open(1_700_000_000_001, 4));
    assert_ne!(first, second);
}

#[test]
fn a_read_that_names_nothing_is_unreadable() {
    assert!(!readable(100, 100));
    assert!(!readable(100, 2));
    assert!(!readable(1, 2));
}

#[test]
fn a_small_library_survives_one_miss() {
    assert!(
        readable(5, 1),
        "one unnamed like must not fail a small library forever"
    );
    assert!(!readable(5, 2), "two of five is above the tolerance");
}

#[test]
fn a_read_at_the_tolerance_still_ranks() {
    assert!(readable(0, 0), "a library with no likes asks for nothing");
    assert!(readable(100, 1));
    assert!(readable(200, 2));
    assert!(readable(1_000, 10));
    assert!(readable(300, 3), "three of three hundred is the percentage");
    assert!(!readable(300, 4), "four of three hundred is past it");
}

#[test]
fn an_answer_to_another_ask_is_dropped() {
    let mut state = State::default();
    state.open(seed());
    state.mark_asked(vec!["spotify:track:a".to_owned()]);
    state.land(&HomeAnswer::Labelled {
        asked: vec!["spotify:track:b".to_owned()],
        named: vec![labelled("spotify:track:b", &[("spotify:artist:x", "X")])],
    });
    assert!(!state.settled(), "another ask's answer is not adopted");
    assert_eq!(state.artists("spotify:track:b"), None);
}

#[test]
fn the_answer_to_the_sent_ask_is_indexed() {
    let mut state = State::default();
    state.open(seed());
    state.mark_asked(vec!["spotify:track:a".to_owned()]);
    state.land(&HomeAnswer::Labelled {
        asked: vec!["spotify:track:a".to_owned()],
        named: vec![labelled(
            "spotify:track:a",
            &[("spotify:artist:x", "X"), ("spotify:artist:y", "Y")],
        )],
    });
    assert!(state.settled());
    let expected = [
        ("spotify:artist:x".to_owned(), "X".to_owned()),
        ("spotify:artist:y".to_owned(), "Y".to_owned()),
    ];
    assert_eq!(state.artists("spotify:track:a"), Some(expected.as_slice()));
    assert_eq!(state.artists("spotify:track:b"), None);
}

#[test]
fn a_read_that_names_too_little_fails_the_shelf() {
    let mut state = State::default();
    state.open(seed());
    let asked = vec!["spotify:track:a".to_owned(), "spotify:track:b".to_owned()];
    state.mark_asked(asked.clone());
    state.land(&HomeAnswer::Labelled {
        asked,
        named: vec![unreadable("spotify:track:a"), unreadable("spotify:track:b")],
    });
    assert!(!state.settled());
    assert_eq!(state.failed(), Some(Failure::Load));
    assert_eq!(state.artists("spotify:track:a"), None);
}

#[test]
fn a_read_at_the_tolerance_is_ranked_from_what_it_named() {
    let mut state = State::default();
    state.open(seed());
    let asked: Vec<String> = (0..200)
        .map(|index| format!("spotify:track:{index}"))
        .collect();
    state.mark_asked(asked.clone());
    let mut named: Vec<TrackEntry> = (0..198)
        .map(|index| {
            labelled(
                &format!("spotify:track:{index}"),
                &[("spotify:artist:a", "A")],
            )
        })
        .collect();
    named.push(unreadable("spotify:track:198"));
    named.push(unreadable("spotify:track:199"));
    state.land(&HomeAnswer::Labelled { asked, named });
    assert!(state.settled());
    assert_eq!(state.failed(), None);
    assert_eq!(state.artists("spotify:track:198"), None);
    assert!(state.artists("spotify:track:0").is_some());
}

#[test]
fn a_failed_read_is_kept_until_the_shelf_forgets_it() {
    let mut state = State::default();
    state.open(seed());
    let ask = HomeAsk::Labels {
        tracks: vec!["spotify:track:a".to_owned()],
    };
    state.mark_asked(vec!["spotify:track:a".to_owned()]);
    state.fail(&ask, Failure::Load);
    assert_eq!(state.failed(), Some(Failure::Load));
    assert!(!state.owed(), "a failure is not asked again on its own");

    state.forget();
    assert_eq!(state.failed(), None);
    assert!(state.owed(), "a retry asks again");
}

#[test]
fn another_shelf_s_failure_is_not_this_shelf_s() {
    let mut state = State::default();
    state.open(seed());
    state.mark_asked(vec!["spotify:track:a".to_owned()]);
    state.fail(
        &HomeAsk::Related {
            artist: "spotify:artist:a".to_owned(),
        },
        Failure::Related,
    );
    assert_eq!(state.failed(), None);
    assert!(!state.owed(), "the ask is still in flight");
}

#[test]
fn an_open_starts_its_own_draw_and_owes_one_ask() {
    let mut state = State::default();
    state.open(seed());
    assert!(state.owed());
    let held = state.seed();
    state.open(seed());
    assert_eq!(state.seed(), held, "the same open draws the same shelf");

    let mut other = State::default();
    other.open(Seed::for_open(1_700_000_000_001, 4));
    assert_ne!(other.seed(), held, "another open draws another shelf");
}

#[test]
fn an_open_forgets_what_the_last_one_read() {
    let mut state = State::default();
    state.open(seed());
    state.mark_asked(vec!["spotify:track:a".to_owned()]);
    state.land(&HomeAnswer::Labelled {
        asked: vec!["spotify:track:a".to_owned()],
        named: vec![labelled("spotify:track:a", &[("spotify:artist:a", "A")])],
    });
    assert!(state.settled());

    state.open(seed());
    assert!(!state.settled());
    assert_eq!(state.artists("spotify:track:a"), None);
    assert!(state.owed());
}
