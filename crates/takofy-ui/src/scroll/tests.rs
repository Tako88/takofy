use egui::{
    Context, DragAndDrop, Event, Modifiers, MouseWheelSource, MouseWheelUnit, Pos2, RawInput, Rect,
    ScrollArea, TouchPhase, pos2, vec2,
};

use super::*;

/// One frame, with the wheel events it should see, and the scroll offset the
/// area ended the frame on.
fn frame(ctx: &Context, events: Vec<Event>, time: f64) -> f32 {
    let raw = RawInput {
        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 400.0))),
        time: Some(time),
        events,
        ..Default::default()
    };
    let mut offset = 0.0;
    let mut full = ctx.run_ui(raw, |ui| {
        instant(ui.ctx());
        offset = ScrollArea::vertical()
            .show(ui, |ui| {
                // As wide as the window: a wheel notch only reaches an area
                // the pointer is over.
                for _ in 0..100 {
                    ui.allocate_space(vec2(800.0, 40.0));
                }
            })
            .state
            .clamped_offset()
            .y;
    });
    full.textures_delta.clear();
    offset
}

fn notch() -> Event {
    Event::MouseWheel {
        unit: MouseWheelUnit::Line,
        delta: vec2(0.0, -1.0),
        phase: TouchPhase::Move,
        source: MouseWheelSource::Wheel,
        modifiers: Modifiers::NONE,
    }
}

/// The pointer has to be over the area for the wheel to reach it.
fn hover() -> Event {
    Event::PointerMoved(pos2(400.0, 200.0))
}

#[test]
fn a_notch_lands_in_the_frame_it_arrives() {
    let ctx = Context::default();
    frame(&ctx, vec![hover()], 1.0);
    let first = frame(&ctx, vec![notch()], 1.016);
    assert!(
        (first - scroll::POINTS_PER_LINE).abs() < 0.5,
        "the whole notch lands at once, not a third of it: {first}"
    );

    let later = frame(&ctx, Vec::new(), 1.032);
    let later = frame(&ctx, Vec::new(), 1.048).max(later);
    assert!(
        (later - first).abs() < 0.5,
        "and no second helping trails behind it: {later} against {first}"
    );
}

#[test]
fn ctrl_and_the_wheel_is_left_to_the_zoom() {
    let ctx = Context::default();
    frame(&ctx, vec![hover()], 1.0);
    let mut event = notch();
    if let Event::MouseWheel { modifiers, .. } = &mut event {
        *modifiers = Modifiers::COMMAND;
    }
    let moved = frame(&ctx, vec![event], 1.016);
    assert!(
        moved.abs() < f32::EPSILON,
        "the zoom owns that notch, so nothing scrolls: {moved}"
    );
}

/// A viewport at the origin, 800 wide and 400 tall.
fn viewport() -> Rect {
    Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 400.0))
}

const DT: f32 = 0.05;

#[test]
fn the_middle_scrolls_nothing() {
    assert_eq!(drag_step(pos2(400.0, 200.0), viewport(), DT), 0.0);
}

#[test]
fn at_the_sixty_four_point_line_is_zero() {
    let edge = scroll::DRAG_EDGE;

    assert_eq!(drag_step(pos2(400.0, edge), viewport(), DT), 0.0);
    assert_eq!(
        drag_step(pos2(400.0, viewport().bottom() - edge), viewport(), DT),
        0.0
    );
}

#[test]
fn halfway_in_is_half_speed() {
    let edge = scroll::DRAG_EDGE;
    let half = scroll::DRAG_MAX * DT / 2.0;
    let top = drag_step(pos2(400.0, edge / 2.0), viewport(), DT);
    let bottom = drag_step(
        pos2(400.0, viewport().bottom() - edge / 2.0),
        viewport(),
        DT,
    );

    assert!((top + half).abs() < 0.01, "half speed up: {top}");
    assert!((bottom - half).abs() < 0.01, "half speed down: {bottom}");
}

#[test]
fn the_edge_and_beyond_is_full_speed_either_way() {
    let full = scroll::DRAG_MAX * DT;

    for y in [0.0, -50.0] {
        assert_eq!(drag_step(pos2(400.0, y), viewport(), DT), -full, "{y}");
    }
    for y in [400.0, 450.0] {
        assert_eq!(drag_step(pos2(400.0, y), viewport(), DT), full, "{y}");
    }
}

#[test]
fn outside_the_columns_scrolls_nothing() {
    assert_eq!(drag_step(pos2(-1.0, 0.0), viewport(), DT), 0.0);
    assert_eq!(drag_step(pos2(801.0, 400.0), viewport(), DT), 0.0);
}

#[test]
fn a_short_viewport_picks_the_nearer_edge() {
    let short = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 100.0));

    assert!(drag_step(pos2(400.0, 30.0), short, DT) < 0.0);
    assert!(drag_step(pos2(400.0, 70.0), short, DT) > 0.0);
}

#[test]
fn a_stall_is_capped() {
    let capped = scroll::DRAG_MAX * 0.1;

    assert_eq!(drag_step(pos2(400.0, 0.0), viewport(), 1.0), -capped);
}

#[test]
fn non_finite_is_zero() {
    let broken = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, f32::NAN));
    let cases = [
        (pos2(f32::NAN, 10.0), viewport(), DT),
        (pos2(400.0, f32::INFINITY), viewport(), DT),
        (pos2(400.0, 10.0), broken, DT),
        (pos2(400.0, 10.0), viewport(), f32::NAN),
        (pos2(400.0, 10.0), viewport(), f32::INFINITY),
    ];

    for (pointer, area, dt) in cases {
        assert_eq!(drag_step(pointer, area, dt), 0.0, "{pointer:?} {dt}");
    }
}

/// One frame in a scrolling area with `follow_drag` first in it, given the
/// pointer's own position, and the offset the area ended on.
fn dragged(ctx: &Context, pointer: Pos2, time: f64) -> f32 {
    let raw = RawInput {
        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 400.0))),
        time: Some(time),
        events: vec![Event::PointerMoved(pointer)],
        ..Default::default()
    };
    let mut offset = 0.0;
    let mut full = ctx.run_ui(raw, |ui| {
        offset = ScrollArea::vertical()
            .show(ui, |ui| {
                follow_drag(ui);
                for _ in 0..100 {
                    ui.allocate_space(vec2(800.0, 40.0));
                }
            })
            .state
            .clamped_offset()
            .y;
    });
    full.textures_delta.clear();
    offset
}

#[test]
fn a_held_carry_at_the_bottom_edge_scrolls_the_area() {
    let ctx = Context::default();
    let pointer = pos2(400.0, 395.0);
    let _ = dragged(&ctx, pointer, 1.0);
    DragAndDrop::set_payload(&ctx, ());

    // The offset lands on the frame after the delta, because a scroll area
    // animates every delta, however short the animation.
    let scrolled = dragged(&ctx, pointer, 1.016);
    let scrolled = dragged(&ctx, pointer, 1.032).max(scrolled);

    assert!(scrolled > 1.0, "the area follows the carry: {scrolled}");
}

#[test]
fn no_payload_no_scroll() {
    let ctx = Context::default();
    let pointer = pos2(400.0, 395.0);
    let _ = dragged(&ctx, pointer, 1.0);

    assert_eq!(dragged(&ctx, pointer, 1.016), 0.0);
}

#[test]
fn a_pointer_in_the_middle_does_not_scroll() {
    let ctx = Context::default();
    let pointer = pos2(400.0, 200.0);
    let _ = dragged(&ctx, pointer, 1.0);
    DragAndDrop::set_payload(&ctx, ());

    assert_eq!(dragged(&ctx, pointer, 1.016), 0.0);
}
