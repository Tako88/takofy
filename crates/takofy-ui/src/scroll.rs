//! Wheel scrolling, applied in the frame it arrives.
//!
//! egui spreads a discrete wheel notch over about a fifth of a second, and the
//! notch moves less than a row, so the wheel reads as both slow and laggy. The
//! scroll areas read `smooth_scroll_delta`, so replacing that with the frame's
//! own wheel motion takes the wheel back: a notch lands at once, and whatever
//! egui was holding back is overwritten before anything can consume it.

use egui::{Context, Event, MouseWheelUnit, TouchPhase, Vec2};

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

#[cfg(test)]
mod tests {
    use egui::{
        Context, Event, Modifiers, MouseWheelSource, MouseWheelUnit, RawInput, Rect, ScrollArea,
        TouchPhase, pos2, vec2,
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
}
