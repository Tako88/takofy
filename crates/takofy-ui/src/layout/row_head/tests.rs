use super::*;
use crate::theme::extent;

const S2: f32 = space::S2;

fn head(width: f32) -> Rect {
    Rect::from_min_size(pos2(20.0, 40.0), vec2(width, 52.0))
}

fn about(width: f32) -> About {
    About {
        width,
        height: 38.0,
        seed: 20.0,
    }
}

#[test]
fn one_trailing_button_takes_the_heads_right_edge_on_the_seed_line() {
    let head = head(420.0);
    let placed = place(
        head,
        about(head.width()),
        Cluster {
            play: Some(square(extent::BUTTON_LG)),
            ..Cluster::default()
        },
        S2,
    );

    let play = placed.play.expect("the head has a play");
    let seed_centre = placed.about.top() + about(head.width()).seed / 2.0;
    assert!((play.right() - head.right()).abs() < 0.01);
    assert!(
        (play.center().y - seed_centre).abs() < 0.01,
        "the play rides the seed's line, not the middle of the block"
    );
    assert!((placed.about.center().y - head.center().y).abs() < 0.01);
    assert!(placed.about.right() <= play.left() - S2 + 0.01);
}

#[test]
fn a_ledger_and_two_buttons_are_flush_right_with_the_play_last() {
    let head = head(600.0);
    let placed = place(
        head,
        about(head.width()),
        Cluster {
            ledger: Some(vec2(70.0, 11.0)),
            reroll: Some(square(extent::BUTTON_LG)),
            play: Some(square(extent::BUTTON_LG)),
        },
        S2,
    );

    let ledger = placed.ledger.expect("the head has a ledger");
    let reroll = placed.reroll.expect("the head has a re-roll");
    let play = placed.play.expect("the head has a play");
    assert!(
        (ledger.center().y - reroll.center().y).abs() < 0.01
            && (reroll.center().y - play.center().y).abs() < 0.01,
        "the count and both controls share one line"
    );
    assert!((play.right() - head.right()).abs() < 0.01);
    assert!(reroll.right() <= play.left() - S2 + 0.01);
    assert!(ledger.right() <= reroll.left() - S2 + 0.01);
    assert!(placed.about.right() <= ledger.left() - S2 + 0.01);
}

#[test]
fn the_ledger_sits_on_the_seed_line_rather_than_the_heads_centre() {
    let head = head(600.0);
    let placed = place(
        head,
        about(head.width()),
        Cluster {
            ledger: Some(vec2(70.0, 11.0)),
            ..Cluster::default()
        },
        S2,
    );

    let ledger = placed.ledger.expect("the head has a ledger");
    let seed_centre = placed.about.top() + about(head.width()).seed / 2.0;
    assert!((ledger.center().y - seed_centre).abs() < 0.01);
    assert!(
        ledger.center().y < head.center().y,
        "the ledger is on the seed's line, not floating between the two"
    );
}

#[test]
fn a_long_seed_never_reaches_the_cluster_at_any_width() {
    for width in (320..=2000).step_by(37) {
        let head = head(width as f32);
        let placed = place(
            head,
            about(10_000.0),
            Cluster {
                ledger: Some(vec2(90.0, 11.0)),
                reroll: Some(square(extent::BUTTON_LG)),
                play: Some(square(extent::BUTTON_LG)),
            },
            S2,
        );
        let ledger = placed.ledger.expect("the head has a ledger");
        assert!(
            placed.about.right() <= ledger.left() - S2 + 0.01,
            "the about block meets the cluster at {width}: {:?} and {:?}",
            placed.about,
            ledger
        );
    }
}

#[test]
fn the_height_is_one_step_above_and_below_the_tallest_part() {
    assert!((height(38.0, 34.0) - (space::S1 + 38.0 + space::S2)).abs() < 0.01);
    assert!((height(20.0, 34.0) - (space::S1 + 34.0 + space::S2)).abs() < 0.01);
    assert!((height(20.0, 0.0) - (space::S1 + 20.0 + space::S2)).abs() < 0.01);
}

#[test]
fn no_cluster_leaves_the_about_block_the_whole_head() {
    let head = head(400.0);
    let placed = place(head, about(head.width()), Cluster::default(), S2);
    assert_eq!(placed.ledger, None);
    assert_eq!(placed.reroll, None);
    assert_eq!(placed.play, None);
    assert!((placed.about.width() - head.width()).abs() < 0.01);
}

fn square_at(x: f32, side: f32) -> Rect {
    Rect::from_min_size(pos2(x, 10.0), vec2(side, side))
}

#[test]
fn an_edge_at_nothing_is_no_line() {
    assert!(edge(square_at(0.0, 34.0), 0.0).is_empty());
    assert!(edge(square_at(0.0, 34.0), -3.0).is_empty());
}

#[test]
fn a_whole_edge_closes_on_the_corner_it_started_from() {
    let rect = square_at(0.0, 34.0);
    let points = edge(rect, 1.0);
    assert_eq!(
        points,
        vec![
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
            rect.left_top()
        ]
    );
    assert_eq!(edge(rect, 9.0), points, "more than whole is still whole");
}

#[test]
fn a_quarter_of_a_square_reaches_the_first_corner_then_lands_on_the_side() {
    let rect = square_at(0.0, 40.0);
    let quarter = edge(rect, 0.25);
    assert_eq!(quarter, vec![rect.left_top(), rect.right_top()]);

    // Three tenths of four sides is past the top edge and eight pixels down the
    // right one.
    let walked = edge(rect, 0.3);
    assert_eq!(
        walked.len(),
        3,
        "the corner it turned, then the side it is on"
    );
    assert!((walked[2].x - rect.right()).abs() < 0.01);
    assert!((walked[2].y - (rect.top() + 8.0)).abs() < 0.01);

    let half = edge(rect, 0.5);
    assert_eq!(
        half,
        vec![rect.left_top(), rect.right_top(), rect.right_bottom()]
    );
}

#[test]
fn a_tall_rect_walks_its_own_sides_not_the_square_it_is_not() {
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(20.0, 60.0));
    // Total 160; a fifth is 32, which is past the 20-wide top and 12 down the right.
    let points = edge(rect, 0.2);
    assert_eq!(points.len(), 3, "both top corners, then the side it is on");
    assert!((points[2].x - 20.0).abs() < 0.01);
    assert!((points[2].y - 12.0).abs() < 0.01);
}
