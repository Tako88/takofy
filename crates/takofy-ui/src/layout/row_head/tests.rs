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
fn one_trailing_button_takes_the_heads_right_edge_and_centres_the_words() {
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
    assert!((play.right() - head.right()).abs() < 0.01);
    assert!((play.center().y - head.center().y).abs() < 0.01);
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
