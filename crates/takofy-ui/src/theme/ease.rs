//! The one easing curve the design uses.
//!
//! `motion::EASE_OUT` is a CSS `cubic-bezier`, which is a parametric curve, not
//! a function of time: both x and y run off a shared parameter. `out` solves
//! the curve for time first and then reads y, which is what
//! `Context::animate_bool_with_time_and_easing` needs. It is a plain `fn` for
//! the same reason.

use super::motion;

/// The eased fraction at `t`, with `t` clamped to 0..1.
pub fn out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    let [x1, y1, x2, y2] = motion::EASE_OUT;
    coordinate(y1, y2, solve(x1, x2, t))
}

/// One axis of a cubic bezier whose first and last control points are 0 and 1.
fn coordinate(c1: f32, c2: f32, s: f32) -> f32 {
    let inv = 1.0 - s;
    3.0 * inv * inv * s * c1 + 3.0 * inv * s * s * c2 + s * s * s
}

/// Finds the curve parameter whose x is `target`, by bisection.
///
/// The reference's curve has a single x over 0..1, but bisection costs nothing
/// here and cannot diverge the way Newton's method can on a shallow curve.
fn solve(x1: f32, x2: f32, target: f32) -> f32 {
    let (mut low, mut high) = (0.0_f32, 1.0_f32);
    for _ in 0..32 {
        let mid = (low + high) * 0.5;
        if coordinate(x1, x2, mid) < target {
            low = mid;
        } else {
            high = mid;
        }
    }
    (low + high) * 0.5
}

#[cfg(test)]
mod tests;
