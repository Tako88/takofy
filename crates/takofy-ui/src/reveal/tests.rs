use super::*;
use crate::search_index::{Index, PAGE};

fn pointed(query: &str) -> Reveal {
    let mut reveal = Reveal::default();
    reveal.point(query);
    reveal
}

#[test]
fn a_fresh_reveal_leaves_the_caps_alone() {
    assert_eq!(pointed("q").caps("q", PAGE), PAGE);
    assert_eq!(Reveal::default().caps("q", PAGE), PAGE);
}

#[test]
fn taking_a_kind_unbounds_only_that_kind() {
    let mut reveal = pointed("q");
    reveal.take(Kind::Tracks);
    let caps = reveal.caps("q", PAGE);
    assert_eq!(caps.tracks, usize::MAX);
    assert_eq!((caps.albums, caps.artists), (10, 10));
}

#[test]
fn taking_twice_is_taking_once() {
    let mut reveal = pointed("q");
    reveal.take(Kind::Albums);
    let once = reveal.clone();
    reveal.take(Kind::Albums);
    assert_eq!(reveal, once);
}

#[test]
fn the_same_query_keeps_the_reveal_and_another_resets_it() {
    let mut reveal = pointed("q");
    reveal.take(Kind::Artists);
    reveal.point("q");
    assert_eq!(reveal.caps("q", PAGE).artists, usize::MAX);
    reveal.point("other");
    assert_eq!(reveal.caps("other", PAGE), PAGE);
    reveal.point("q");
    assert_eq!(reveal.caps("q", PAGE), PAGE);
}

#[test]
fn a_new_query_bumps_the_visit_and_a_reentry_keeps_it() {
    let mut reveal = Reveal::default();
    assert_eq!(reveal.visit(), 0);
    reveal.point("a");
    let first = reveal.visit();
    assert_eq!(first, 1);
    reveal.point("a");
    assert_eq!(reveal.visit(), first, "the same query is not a new visit");
    reveal.point("b");
    let second = reveal.visit();
    assert_ne!(second, first, "another query is a new visit");
    reveal.point("a");
    assert_ne!(
        reveal.visit(),
        first,
        "A→B→A starts over rather than landing on A's old visit"
    );
    assert_ne!(reveal.visit(), second);
}

#[test]
fn a_reveal_never_applies_to_another_query() {
    let mut reveal = pointed("q");
    reveal.take(Kind::Tracks);
    assert_eq!(reveal.caps("other", PAGE), PAGE);
}

#[test]
fn a_taken_block_reads_whole_and_the_others_do_not_move() {
    let index = Index {
        tracks: 25,
        ..Index::default()
    };
    let mut reveal = pointed("q");
    let before = index.shape(reveal.caps("q", PAGE));
    assert_eq!(before.bands[0].share(), "10 of 25");
    reveal.take(Kind::Tracks);
    let after = index.shape(reveal.caps("q", PAGE));
    assert_eq!(after.bands[0].shown, 25);
    assert_eq!(after.bands[0].share(), "25");
}
