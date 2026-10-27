use egui::{Context, Pos2, Vec2};

use super::super::*;
use super::test_support::*;

use crate::view::test_support::click_events;

/// One press on a control, event by event, and the reply the last pass made.
fn press(ctx: &Context, bar: &Bar<'_>, size: Vec2, offset: f32, at: Pos2) -> Reply {
    let mut last = Reply::quiet();
    for event in click_events(at) {
        last = frame_of(ctx, bar, size, offset, vec![event], None).reply;
    }
    last
}

#[test]
fn the_text_slot_reports_its_press() {
    let ctx = context();
    let text = text_bar(kind(VIEWPORT));
    let follow = drawn(&ctx, &text, VIEWPORT, 0.0)
        .follow
        .expect("the row draws the page's text control");

    let reply = press(&ctx, &text, VIEWPORT, 0.0, follow.center());
    assert!(reply.followed, "the text control reports its press");
    assert!(
        !reply.played && !reply.shuffled && reply.saved.is_none() && reply.more_door.is_none(),
        "a press on the word is a press on nothing else"
    );
}

#[test]
fn a_slot_that_is_not_live_reports_nothing() {
    let ctx = context();
    let quiet = Bar {
        text: Some(Text {
            label: "Following",
            live: false,
            failed: None,
        }),
        ..text_bar(kind(VIEWPORT))
    };
    let follow = drawn(&ctx, &quiet, VIEWPORT, 0.0)
        .follow
        .expect("a waiting control is still drawn in its place");
    let reply = press(&ctx, &quiet, VIEWPORT, 0.0, follow.center());
    assert!(
        !reply.followed,
        "a control that cannot act reports nothing when pressed"
    );
}

#[test]
fn the_slot_draws_the_pages_label() {
    let ctx = context();
    let following = Bar {
        text: Some(Text {
            label: "Following",
            live: true,
            failed: None,
        }),
        ..text_bar(kind(VIEWPORT))
    };
    let frame = frame_of(&ctx, &following, VIEWPORT, 0.0, Vec::new(), None);
    let follow = drawn(&ctx, &following, VIEWPORT, 0.0)
        .follow
        .expect("the row draws the page's text control");
    let label = text_rect(&frame.output, "Following").expect("the word is painted");
    assert!(
        follow.contains_rect(label),
        "the word sits inside the control's box: {label:?} against {follow:?}"
    );
    assert_eq!(
        text_rect(&frame.output, "Follow"),
        None,
        "the label is the page's, not a word the row chose"
    );
}

#[test]
fn a_bar_with_a_slot_has_no_heart() {
    let ctx = context();
    let text = text_bar(kind(VIEWPORT));
    let boxes = drawn(&ctx, &text, VIEWPORT, 0.0);
    assert!(boxes.follow.is_some(), "the row draws the text control");
    assert!(
        boxes.heart.is_none(),
        "a page with a word in the heart's place has no heart: {:?}",
        boxes.heart
    );
    assert!(boxes.shuffle.is_some(), "the tools are still there");
}
