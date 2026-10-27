use egui::{DragAndDrop, Event, Modifiers, Pos2, RawInput, Shape};

use super::super::*;
use super::test_support::*;
use crate::cover::Cover;
use crate::theme::opacity;

fn timed(events: Vec<Event>, time: f64) -> RawInput {
    RawInput {
        time: Some(time),
        ..input(events)
    }
}

/// The alpha the first text painted as `label` was drawn at.
fn painted_alpha(output: &egui::FullOutput, label: &str) -> Option<u8> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(text) if text.galley.text() == label => Some(text.fallback_color.a()),
            _ => None,
        })
}

#[test]
fn a_seven_point_drag_reports_a_lift_with_the_row_size_and_its_ghost() {
    let ctx = context();
    let mut art = Art::default();
    let queue = queue_of(4, 0);
    let now = Instant::now();
    let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
    let at = painted_at(&output, "t3").expect("row t3 is painted");
    let start = at + Vec2::new(4.0, 4.0);

    let mut art = Art::default();
    run(&ctx, input(Vec::new()), |ui| {
        let _ = show(ui, PANE, &queue, &mut art, now, &Tokens::DARK);
    });
    let end = start + Vec2::new(7.0, 0.0);
    let mut lift = None;
    for event in [
        Event::PointerMoved(start),
        crate::view::test_support::button(start, true),
        Event::PointerMoved(end),
        crate::view::test_support::button(end, false),
    ] {
        let lift = &mut lift;
        art.begin_frame();
        run(&ctx, input(vec![event]), |ui| {
            let reply = show(ui, PANE, &queue, &mut art, now, &Tokens::DARK);
            if let Some(next) = reply.lift {
                *lift = Some(next);
            }
        });
    }

    let lift = lift.expect("a seven-point drag lifts the row");
    assert_eq!(lift.index, 3);
    assert_eq!(
        lift.ghost,
        queue_row::Lifted::new(
            "t3".to_owned(),
            "a3".to_owned(),
            Some("0:04".to_owned()),
            Cover::Missing
        ),
        "the ghost is the row as it was drawn, cover look included"
    );
    assert_eq!(lift.size.y, ROW_H, "the ghost is one row tall");
    assert!(
        (lift.size.x - (PANE.width() - 2.0 * space::S3)).abs() < 0.5,
        "the ghost is the list's width: {:?}",
        lift.size
    );

    // A three-point twitch is a click, not a drag.
    let reply = click_with(
        &ctx,
        PANE,
        &queue,
        at + Vec2::new(4.0, 4.0),
        Modifiers::NONE,
        1,
    );
    assert_eq!(reply.lift, None, "a click does not lift the row");
}

#[test]
fn a_carried_pane_scrolls_when_the_pointer_holds_its_bottom_edge() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let now = Instant::now();
    let mut art = Art::default();
    let queue = queue_of(40, 0);
    let at = Pos2::new(PANE.center().x, PANE.bottom() - 2.0);

    // The pointer arrives over the pane, then a block is carried.
    run(&ctx, timed(vec![Event::PointerMoved(at)], 1.0), |ui| {
        let _ = show(ui, PANE, &queue, &mut art, now, &tokens);
    });
    DragAndDrop::set_payload(&ctx, ());

    let mut first = Vec::new();
    let mut last = Vec::new();
    for frame in 0..12 {
        let mut output = ctx.run_ui(
            timed(vec![Event::PointerMoved(at)], 1.0 + 0.016 * frame as f64),
            |ui| {
                let _ = show(ui, PANE, &queue, &mut art, now, &tokens);
            },
        );
        output.textures_delta.clear();
        if frame == 0 {
            first = painted_rows(&output);
        }
        last = painted_rows(&output);
    }

    assert!(
        !first.is_empty() && !last.is_empty(),
        "the pane must list rows: {first:?} {last:?}"
    );
    assert!(
        last[0] > first[0],
        "the carried pane must scroll under the pointer: {first:?} -> {last:?}"
    );
}

#[test]
fn a_carried_pane_does_not_scroll_to_its_focused_row() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let now = Instant::now();
    let mut art = Art::default();
    // The sixth row hangs past the fold, so it is drawn and focused while
    // only part of it is on screen.
    let mut queue = queue_of(40, 0);
    queue.refocus_at(6);

    let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
    assert!(
        painted_at(&output, "t6").is_some(),
        "the focused row must be laid out"
    );
    let rest = painted_at(&output, "t1").expect("row t1 is painted");

    carrying_rows(&ctx, vec![6]);
    for frame in 0..8 {
        let mut output = ctx.run_ui(timed(Vec::new(), 1.0 + 0.016 * f64::from(frame)), |ui| {
            let _ = show(ui, PANE, &queue, &mut art, now, &tokens);
        });
        output.textures_delta.clear();
        assert_eq!(
            painted_at(&output, "t1"),
            Some(rest),
            "the pane scrolled to the focused row while carrying: frame {frame}"
        );
    }
}

#[test]
fn a_carried_row_is_lifted_and_paints_dimmed() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let now = Instant::now();
    let mut art = Art::default();
    let queue = queue_of(6, 0);
    carrying_rows(&ctx, vec![3]);

    let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
    let payload = carry::carried(&ctx).expect("a carry");
    assert!(
        standing(&queue, Some(payload.as_ref()), 3).lifted,
        "the carried row is the lifted one"
    );
    assert!(
        standing(&queue, Some(payload.as_ref()), 4).carrying
            && !standing(&queue, Some(payload.as_ref()), 4).lifted,
        "the rest of the queue only knows a block is carried"
    );

    let ink = tokens.palette.text;
    assert_eq!(
        painted_alpha(&output, "t3"),
        Some(ink.gamma_multiply(opacity::LIFTED).a()),
        "the lifted row steps back"
    );
    assert_eq!(
        painted_alpha(&output, "t4"),
        Some(ink.a()),
        "the row beside it does not"
    );
}
