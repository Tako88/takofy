use super::*;
use crate::theme::space;

const EXTRA: f32 = 40.0;

fn area(width: f32) -> Rect {
    Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 0.0))
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.left() < b.right() - 0.01
        && b.left() < a.right() - 0.01
        && a.top() < b.bottom() - 0.01
        && b.top() < a.bottom() - 0.01
}

#[test]
fn the_narrow_grid_wraps_three_or_four_faces_and_keeps_two_on_one_row() {
    let four = faces(area(600.0), 2, space::S3, EXTRA);
    assert_eq!(four.rows(4), 2);
    let three = faces(area(600.0), 2, space::S3, EXTRA);
    assert_eq!(three.rows(3), 2);
    let two = faces(area(600.0), 2, space::S3, EXTRA);
    assert_eq!(two.rows(2), 1);
    let wide_three = faces(area(1000.0), 4, space::S3, EXTRA);
    assert_eq!(wide_three.rows(3), 1);
}

#[test]
fn a_lone_face_keeps_its_size_and_never_stretches() {
    let wide = lone(area(1200.0), 192.0, EXTRA);
    assert!(
        (wide.width() - 192.0).abs() < 0.01,
        "a lone face stays a card"
    );
    assert!((wide.height() - (192.0 + EXTRA)).abs() < 0.01);

    let narrow = lone(area(120.0), 192.0, EXTRA);
    assert!(
        (narrow.width() - 120.0).abs() < 0.01,
        "it gives way to a narrower page"
    );
}

#[test]
fn a_pair_turns_two_cards_on_their_side_without_overlapping() {
    let pair = pair(area(1000.0), space::S3, 0.47, EXTRA);
    assert!(!overlaps(pair.cards[0], pair.cards[1]));
    assert!((pair.cards[0].width() - pair.cards[1].width()).abs() < 0.01);
    for (card, art) in pair.cards.iter().zip(pair.arts) {
        assert!(card.contains_rect(art), "{card:?} holds {art:?}");
        assert!(art.width() < card.width(), "the body sits beside the art");
    }
}

#[test]
fn a_split_puts_the_tracks_beside_the_card() {
    let split = split(area(1000.0), 192.0, space::S5, EXTRA);
    assert!(split.tracks.left() > split.card.right());
    assert!((split.tracks.height() - split.card.height()).abs() < 0.01);
    assert!((split.card.width() - 192.0).abs() < 0.01);
    assert!((split.tracks.right() - 1000.0).abs() < 0.01);
}

#[test]
fn the_corner_tiles_sit_on_the_cards_own_corners() {
    let card = Rect::from_min_size(pos2(10.0, 20.0), vec2(226.0, 274.0));
    let follow = follow(card);
    let more = more(card);
    let play = play(card);

    for tile in [follow, more, play] {
        assert!(card.contains_rect(tile), "{tile:?} stays inside {card:?}");
    }
    // The corner control keeps one padding step from the edge it hangs on.
    assert!((card.right() - space::S2 - more.right()).abs() < 0.01);
    assert!((card.right() - space::S2 - play.right()).abs() < 0.01);
    assert!((more.top() - card.top() - space::S2).abs() < 0.01);
    assert!((card.bottom() - space::S2 - play.bottom()).abs() < 0.01);

    assert!(!overlaps(follow, play));
    assert!(!overlaps(more, play));
    assert!(!overlaps(follow, more));
    assert!(more.center().y < card.center().y, "the menu sits high");
    assert!(play.center().y > card.center().y, "the play sits low");
}

#[test]
fn every_corner_tile_keeps_its_press_point_on_the_card() {
    for size in [vec2(120.0, 150.0), vec2(226.0, 274.0), vec2(340.0, 400.0)] {
        let card = Rect::from_min_size(pos2(0.0, 0.0), size);
        for tile in [follow(card), more(card), play(card)] {
            assert!(card.contains_rect(tile), "{tile:?} stays inside {card:?}");
            assert!(card.contains(tile.center()));
        }
    }
}
