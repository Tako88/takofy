//! Wheel scrolling, applied in the frame it arrives.
//!
//! egui spreads a discrete wheel notch over about a fifth of a second, and the
//! notch moves less than a row, so the wheel reads as both slow and laggy. The
//! scroll areas read `smooth_scroll_delta`, so replacing that with the frame's
//! own wheel motion takes the wheel back: a notch lands at once, and whatever
//! egui was holding back is overwritten before anything can consume it.

use egui::style::ScrollAnimation;
use egui::{Context, DragAndDrop, Event, MouseWheelUnit, Pos2, Rect, TouchPhase, Ui, Vec2, vec2};

use crate::theme::scroll;

/// Replaces this frame's smoothed scroll with the wheel's own delta.
///
/// Runs once per frame, before any scroll area is shown. A `Page` unit is left
/// to egui, which is the only side that knows the viewport's height.
pub fn instant(ctx: &Context) {
    let zoom_modifier = ctx.options(|options| options.input_options.zoom_modifier);

    ctx.input_mut(|input| {
        let wheel = input.events.iter().filter_map(|event| match event {
            // Only a moving gesture carries travel; egui itself reads no delta
            // from the start or the end of one.
            Event::MouseWheel {
                unit,
                delta,
                modifiers,
                phase: TouchPhase::Start | TouchPhase::Move,
                ..
            } => Some((unit, *delta, *modifiers)),
            _ => None,
        });

        let mut delta = Vec2::ZERO;
        let mut pages = false;
        let mut zooming = false;
        for (unit, travel, modifiers) in wheel {
            zooming |= modifiers.matches_any(zoom_modifier);
            match unit {
                MouseWheelUnit::Point => delta += travel,
                MouseWheelUnit::Line => delta += travel * scroll::POINTS_PER_LINE,
                MouseWheelUnit::Page => pages = true,
            }
        }

        // Ctrl and the wheel is a zoom, and a page is egui's to measure: leave
        // both exactly as egui worked them out.
        if zooming || pages {
            return;
        }
        input.smooth_scroll_delta = delta;
    });
}

/// How far a scroll area should move this frame while a row is carried over it,
/// in points, negative upwards.
///
/// A pointer held inside `DRAG_EDGE` of the top or bottom edge scrolls towards
/// that edge, faster the deeper into the margin it is. A pointer outside the
/// area's columns, or a stalled frame, scrolls nothing.
pub fn drag_step(pointer: Pos2, viewport: Rect, dt: f32) -> f32 {
    if !pointer.x.is_finite()
        || !pointer.y.is_finite()
        || !dt.is_finite()
        || !viewport.is_finite()
        || !viewport.x_range().contains(pointer.x)
    {
        return 0.0;
    }

    let up = ((viewport.top() + scroll::DRAG_EDGE - pointer.y) / scroll::DRAG_EDGE).clamp(0.0, 1.0);
    let down =
        ((pointer.y - (viewport.bottom() - scroll::DRAG_EDGE)) / scroll::DRAG_EDGE).clamp(0.0, 1.0);
    let depth = if up > down { -up } else { down };

    depth * scroll::DRAG_MAX * dt.clamp(0.0, 0.1)
}

/// Scrolls the area a carried row is held near an edge of.
///
/// Runs first inside a scroll area's content, so this frame's rows are laid out
/// at the offset the frame will draw and a drop marker resolves against it.
pub fn follow_drag(ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    if !DragAndDrop::has_any_payload(&ctx) {
        return;
    }
    let Some(pointer) = ctx.pointer_latest_pos() else {
        return;
    };
    let dt = ctx.input(|input| input.stable_dt);
    let step = drag_step(pointer, ui.clip_rect(), dt);
    if step == 0.0 {
        return;
    }

    ui.scroll_with_delta_animation(vec2(0.0, -step), ScrollAnimation::none());
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use egui::{
        Context, DragAndDrop, Event, Modifiers, MouseWheelSource, MouseWheelUnit, Pos2, RawInput,
        Rect, ScrollArea, TouchPhase, pos2, vec2,
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
}
