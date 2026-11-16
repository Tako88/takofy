use takofy_model::TrackEntry;
use takofy_model::home::{DiscoveryArtist, DiscoveryCard};

use super::*;

fn track(index: usize, named: bool) -> TrackEntry {
    if named {
        TrackEntry {
            named: true,
            title: format!("Track {index}"),
            artists: "Vela".to_owned(),
            ..TrackEntry::unnamed(&format!("spotify:track:{index}"))
        }
    } else {
        TrackEntry::unnamed(&format!("spotify:track:{index}"))
    }
}

fn card(index: usize, named: &[bool]) -> DiscoveryCard {
    DiscoveryCard {
        artist: DiscoveryArtist {
            uri: format!("spotify:artist:{index}"),
            name: format!("Artist {index}"),
        },
        outside: None,
        portrait: None,
        tracks: named
            .iter()
            .enumerate()
            .map(|(at, named)| track(index * 10 + at, *named))
            .collect(),
        latest_year: None,
    }
}

fn refs(cards: &[DiscoveryCard]) -> Vec<&DiscoveryCard> {
    cards.iter().collect()
}

#[test]
fn one_card_with_three_two_one_or_no_named_tracks() {
    for (named, expected) in [
        (vec![true, true, true], Body::Split { rows: 3 }),
        (vec![true, true], Body::Split { rows: 2 }),
        (vec![true], Body::One),
        (vec![], Body::One),
    ] {
        let cards = [card(0, &named)];
        assert_eq!(body_of(&refs(&cards), false), expected, "{named:?}");
    }
}

#[test]
fn a_card_with_only_unnamed_tracks_is_a_lone_face() {
    let cards = [card(0, &[false, false, false])];
    assert_eq!(body_of(&refs(&cards), false), Body::One);
}

#[test]
fn two_cards_are_a_pair_wide_and_two_faces_narrow() {
    let cards = [card(0, &[true, true]), card(1, &[true, true])];
    assert_eq!(body_of(&refs(&cards), false), Body::Pair);
    assert_eq!(body_of(&refs(&cards), true), Body::Faces { cards: 2 });
}

#[test]
fn three_or_four_cards_are_faces_and_five_or_six_are_capped_at_four() {
    for count in [3, 4] {
        let cards: Vec<DiscoveryCard> = (0..count).map(|at| card(at, &[true])).collect();
        assert_eq!(
            body_of(&refs(&cards), false),
            Body::Faces { cards: count },
            "{count}"
        );
    }
    for count in [5, 6] {
        let cards: Vec<DiscoveryCard> = (0..count).map(|at| card(at, &[true])).collect();
        assert_eq!(
            body_of(&refs(&cards), false),
            Body::Faces { cards: 4 },
            "{count} is capped at four"
        );
    }
}

#[test]
fn the_columns_are_four_wide_and_two_narrow_including_three_cards() {
    for cards in [3, 4] {
        let body = Body::Faces { cards };
        assert_eq!(columns_for(body, false), 4);
        assert_eq!(columns_for(body, true), 2);
    }
    assert_eq!(columns_for(Body::Faces { cards: 2 }, true), 2);
    assert_eq!(columns_for(Body::Pair, false), 2);
    assert_eq!(columns_for(Body::One, false), 1);
    assert_eq!(columns_for(Body::Split { rows: 2 }, false), 1);
}

#[test]
fn a_row_plays_the_split_tracks_or_each_faces_first() {
    let split = [card(0, &[true, true, false])];
    assert_eq!(
        row_tracks(&refs(&split), Body::Split { rows: 2 })
            .iter()
            .map(|track| track.uri.clone())
            .collect::<Vec<_>>(),
        vec!["spotify:track:0".to_owned(), "spotify:track:1".to_owned()]
    );

    let faces = [card(0, &[true, false]), card(1, &[true])];
    assert_eq!(
        row_tracks(&refs(&faces), Body::Faces { cards: 2 })
            .iter()
            .map(|track| track.uri.clone())
            .collect::<Vec<_>>(),
        vec!["spotify:track:0".to_owned(), "spotify:track:10".to_owned()]
    );
}

#[test]
fn the_ledger_counts_tracks_only_for_a_split() {
    let split = [card(0, &[true, true, true])];
    assert_eq!(
        ledger_counts(&refs(&split), Body::Split { rows: 3 }),
        (1, Some(3))
    );

    let faces: Vec<DiscoveryCard> = (0..4).map(|at| card(at, &[true])).collect();
    assert_eq!(
        ledger_counts(&refs(&faces), Body::Faces { cards: 4 }),
        (4, None)
    );
}
