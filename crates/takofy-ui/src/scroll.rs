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
mod tests;
