use egui::Vec2;

use super::super::*;
use super::test_support::*;

#[test]
fn a_failure_at_rest_sits_between_play_and_the_slot() {
    let ctx = context();
    let failed = Bar {
        text: Some(Text {
            label: "Following",
            live: true,
            failed: Some(Failure::Save),
        }),
        ..text_bar(kind(VIEWPORT))
    };
    let frame = frame_of(&ctx, &failed, VIEWPORT, 0.0, Vec::new(), None);
    let boxes = drawn(&ctx, &failed, VIEWPORT, 0.0);
    let follow = boxes.follow.expect("the row draws the text control");

    let copy = crate::signin::failure_message(Failure::Save);
    let hint = text_rect(&frame.output, copy).expect("the failure's copy is painted");
    assert!(
        hint.left() >= boxes.play.right() + actionbar::GAP - 0.01,
        "the copy starts where the name's box starts: {hint:?} against {:?}",
        boxes.play
    );
    assert!(
        hint.right() <= follow.left() - actionbar::GAP + 0.01,
        "the copy stops short of the control: {hint:?} against {follow:?}"
    );
    assert!(
        (hint.center().y - follow.center().y).abs() < 1.0,
        "the copy shares the control's line: {hint:?} against {follow:?}"
    );
    assert!(
        text_rect(&frame.output, "Following").is_some(),
        "the label stays where the server last left it"
    );
    assert_eq!(
        frame.flow.height(),
        actionbar::REST_H,
        "a message brings no second line: the row keeps its resting height"
    );
}

#[test]
fn a_failure_replaces_the_pinned_title_and_the_bar_stays_sixty() {
    let ctx = context();
    let failed = Bar {
        text: Some(Text {
            label: "Following",
            live: true,
            failed: Some(Failure::Save),
        }),
        ..text_bar(kind(VIEWPORT))
    };
    let clean = frame_of(
        &ctx,
        &text_bar(kind(VIEWPORT)),
        VIEWPORT,
        FILLER + 50.0,
        Vec::new(),
        None,
    );
    assert!(
        text_rect(&clean.output, "Vela Moreno").is_some(),
        "a pinned row with nothing to report paints the name"
    );

    let pinned = frame_of(&ctx, &failed, VIEWPORT, FILLER + 50.0, Vec::new(), None);
    let copy = crate::signin::failure_message(Failure::Save);
    let hint = text_rect(&pinned.output, copy).expect("the failure's copy is painted");
    assert!(
        text_rect(&pinned.output, "Vela Moreno").is_none(),
        "the message takes the name's box while it stands: {:?}",
        text_rect(&pinned.output, "Vela Moreno")
    );
    let bar = pinned.rect();
    assert_eq!(
        bar.height(),
        actionbar::STUCK_H,
        "the row is still the pinned height: {bar:?}"
    );
    let boxes = drawn(&ctx, &failed, VIEWPORT, FILLER + 50.0);
    assert!(
        hint.left() >= boxes.play.right() + actionbar::GAP - 0.01,
        "the copy sits in the name's box: {hint:?}"
    );
    assert!(
        hint.right() <= boxes.follow.expect("the slot").left() - actionbar::GAP + 0.01,
        "and in front of the control: {hint:?}"
    );
    assert!(
        (hint.center().y - bar.center().y).abs() < 0.01,
        "the copy is on the row's line: {hint:?} against {bar:?}"
    );
}

#[test]
fn a_failure_with_no_box_paints_nothing() {
    let ctx = context();
    ctx.enable_accesskit();
    // Narrow enough that the row has no name's box between the play tile and
    // the text control.
    let size = Vec2::new(300.0, 400.0);
    let failed = Bar {
        text: Some(Text {
            label: "Following",
            live: true,
            failed: Some(Failure::Save),
        }),
        ..text_bar(kind(size))
    };
    let mut announced = None;
    let (frame, _) = frame_of_probed(&ctx, &failed, size, 0.0, Vec::new(), None, |ui| {
        announced = ui
            .ctx()
            .accesskit_node_builder(ui.scope_id().with("bar-status"), |node| {
                (node.role(), node.live(), node.label().map(str::to_owned))
            });
    });
    let copy = crate::signin::failure_message(Failure::Save);
    assert!(
        text_rect(&frame.output, copy).is_none(),
        "a message with no box to sit in paints nothing: {:?}",
        text_rect(&frame.output, copy)
    );
    let announced = announced.expect("the failure still speaks through the status node");
    assert_eq!(announced.0, egui::Role::Status);
    assert_eq!(announced.1, Some(egui::accesskit::Live::Polite));
    assert_eq!(announced.2.as_deref(), Some(copy));
    assert!(
        text_rect(&frame.output, "Vela Moreno").is_none(),
        "and the name has no box either, so neither is drawn"
    );
    assert!(
        drawn(&ctx, &failed, size, 0.0).follow.is_some(),
        "the control is still drawn: only its message had nowhere to go"
    );
}
