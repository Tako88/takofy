use std::collections::HashSet;

use takofy_model::home::{DiscoveryArtist, DiscoveryCard, OutsideId};
use takofy_model::{FollowedArtist, SavedAlbum};

use super::*;

fn artist(index: usize) -> DiscoveryArtist {
    DiscoveryArtist {
        uri: format!("spotify:artist:{index}"),
        name: format!("Artist {index}"),
    }
}

fn id(index: usize) -> OutsideId {
    OutsideId::from_id(index as i64)
}

fn card(index: usize) -> DiscoveryCard {
    DiscoveryCard {
        artist: artist(index),
        outside: Some(id(index)),
        portrait: None,
        tracks: Vec::new(),
        latest_year: None,
    }
}

fn unstamped(index: usize) -> DiscoveryCard {
    DiscoveryCard {
        outside: None,
        ..card(index)
    }
}

fn named(index: usize, name: &str) -> DiscoveryCard {
    DiscoveryCard {
        artist: DiscoveryArtist {
            uri: format!("spotify:artist:{index}"),
            name: name.to_owned(),
        },
        ..card(index)
    }
}

fn followed(index: usize) -> FollowedArtist {
    FollowedArtist {
        uri: format!("spotify:artist:{index}"),
        name: format!("Artist {index}"),
        named: true,
        portrait: None,
        releases: None,
        followed_at: None,
    }
}

fn owned(artists: &[usize]) -> Owned {
    let followed: Vec<FollowedArtist> = artists.iter().copied().map(followed).collect();
    Owned::new(&followed, &[] as &[SavedAlbum])
}

fn hidden(artists: &[usize]) -> Hidden {
    let mut hidden = Hidden::default();
    for index in artists {
        assert!(
            hidden.hide(&artist(*index).uri),
            "an artist address is hidable"
        );
    }
    hidden
}

fn held(ids: &[usize]) -> HashSet<OutsideId> {
    ids.iter().copied().map(id).collect()
}

fn uris(cards: &[DiscoveryCard]) -> Vec<&str> {
    cards.iter().map(|card| card.artist.uri.as_str()).collect()
}

fn kept(cards: &[DiscoveryCard]) -> Vec<&DiscoveryCard> {
    cards.iter().collect()
}

#[test]
fn a_card_another_row_holds_is_not_appended() {
    let ring = [card(1), card(2)];

    let (cards, dropped) = gather(&ring, &held(&[1]), None, &owned(&[]), &hidden(&[]));

    assert_eq!(
        uris(&cards),
        ["spotify:artist:2"],
        "the held artist is skipped"
    );
    assert_eq!(
        dropped,
        [id(1)],
        "and reported, so the next press goes deeper"
    );
}

#[test]
fn a_card_the_library_already_holds_is_not_appended() {
    let ring = [card(1), card(2)];

    let (cards, dropped) = gather(&ring, &held(&[]), None, &owned(&[1]), &hidden(&[]));

    assert_eq!(uris(&cards), ["spotify:artist:2"]);
    assert_eq!(dropped, [id(1)]);
}

#[test]
fn a_hidden_artist_is_not_appended() {
    let ring = [card(1), card(2)];

    let (cards, dropped) = gather(&ring, &held(&[]), None, &owned(&[]), &hidden(&[1]));

    assert_eq!(uris(&cards), ["spotify:artist:2"]);
    assert_eq!(dropped, [id(1)]);
}

#[test]
fn the_seeds_own_artist_is_not_appended() {
    let ring = [card(1), card(2)];
    let anchor = artist(1);

    let (cards, dropped) = gather(&ring, &held(&[]), Some(&anchor), &owned(&[]), &hidden(&[]));

    assert_eq!(uris(&cards), ["spotify:artist:2"]);
    assert_eq!(dropped, [id(1)]);
}

#[test]
fn the_same_artist_appears_once_in_a_ring() {
    let ring = [card(1), card(2), card(1)];

    let (cards, dropped) = gather(&ring, &held(&[]), None, &owned(&[]), &hidden(&[]));

    assert_eq!(uris(&cards), ["spotify:artist:1", "spotify:artist:2"]);
    assert_eq!(dropped, [id(1)], "the repeat produced no card of its own");
}

#[test]
fn the_ring_keeps_the_order_the_walk_found_it_in() {
    let ring = [named(3, "Zed"), named(1, "Ada"), named(2, "Moe")];

    let (cards, dropped) = gather(&ring, &held(&[]), None, &owned(&[]), &hidden(&[]));

    assert_eq!(
        cards
            .iter()
            .map(|card| card.artist.name.as_str())
            .collect::<Vec<_>>(),
        ["Zed", "Ada", "Moe"],
        "a ring is not re-sorted on the way in"
    );
    assert!(dropped.is_empty());
}

#[test]
fn a_ring_is_cut_at_ring_cards() {
    let ring: Vec<DiscoveryCard> = (1..=RING_CARDS + 2).map(card).collect();

    let (cards, dropped) = gather(&ring, &held(&[]), None, &owned(&[]), &hidden(&[]));

    assert_eq!(cards.len(), RING_CARDS, "one ring adds at most RING_CARDS");
    assert_eq!(
        uris(&cards),
        [
            "spotify:artist:1",
            "spotify:artist:2",
            "spotify:artist:3",
            "spotify:artist:4"
        ],
        "the cut takes the first of the ring, in order"
    );
    assert!(
        dropped.is_empty(),
        "a card past the cut produced a card, so it is not passed"
    );
}

#[test]
fn an_empty_ring_gives_nothing() {
    let (cards, dropped) = gather(&[], &held(&[1]), None, &owned(&[]), &hidden(&[]));

    assert!(cards.is_empty(), "there is nothing to append");
    assert!(dropped.is_empty(), "and nothing was passed");
}

#[test]
fn every_card_the_ring_drops_is_reported() {
    let ring = [card(1), card(2), card(3), card(4), card(5), card(5)];
    let anchor = artist(3);

    let (cards, dropped) = gather(
        &ring,
        &held(&[1]),
        Some(&anchor),
        &owned(&[2]),
        &hidden(&[4]),
    );

    assert_eq!(uris(&cards), ["spotify:artist:5"], "only one card survives");
    assert_eq!(
        dropped,
        [id(1), id(2), id(3), id(4), id(5)],
        "every id that produced no card feeds the next press"
    );
}

#[test]
fn the_frontier_is_at_most_ring_cards() {
    let cards: Vec<DiscoveryCard> = (1..=RING_CARDS + 2).map(card).collect();
    let kept = kept(&cards);

    let frontier = frontier(&kept, &cards);

    assert_eq!(frontier, (1..=RING_CARDS).map(id).collect::<Vec<_>>());
}

#[test]
fn the_frontier_is_the_cards_the_row_draws_that_its_tail_holds() {
    let cards: Vec<DiscoveryCard> = (1..=5).map(card).collect();
    let kept = kept(&cards);

    let frontier = frontier(&kept, &cards[2..]);

    assert_eq!(
        frontier,
        [id(3), id(4), id(5)],
        "only the ring the row grew in the last hop is walked from"
    );
}

#[test]
fn a_card_with_no_outside_id_is_not_a_frontier_node() {
    let cards = [card(1), unstamped(2)];
    let kept = kept(&cards);

    let frontier = frontier(&kept, &cards);

    assert_eq!(
        frontier,
        [id(1)],
        "there is nothing to walk from without an id"
    );
}

#[test]
fn a_card_an_earlier_row_claims_is_never_a_frontier_node() {
    let cards = [card(1), card(2), card(3)];
    let drawn = [cards[1].clone(), cards[2].clone()];
    let kept = kept(&drawn);

    let frontier = frontier(&kept, &cards);

    assert_eq!(
        frontier,
        [id(2), id(3)],
        "the frontier is what this row draws, not every card it holds"
    );
}
