//! The icon model: the reference sprite, flattened to plain geometry.
//!
//! Nothing here parses SVG. `data.rs` is generated offline from
//! `docs/reference/main.js` and carries the reference's own polylines. This
//! module turns them into `Shape`s.
//!
//! Fills are emitted as one convex polygon per piece, so the tessellator
//! feathers their edges. Strokes keep the reference's polylines and get round
//! ends and joins applied here, because epaint squares its caps and mitres its
//! joins where the reference asks for round.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke};

use crate::theme::icon::{GRID, Size};

mod data;

/// A point in the sprite's 24-unit viewBox.
pub type Point = (f32, f32);

/// How far a point may sit outside the viewBox before it is a bug.
///
/// Hand-decomposed fill pieces overlap by at least this much, so the margin is
/// the overlap, not slack.
pub const OVERLAP: f32 = 2.0;

/// A stroked polyline.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Line {
    pub points: &'static [Point],
    pub closed: bool,
    /// A per-element `stroke-width` in grid units, overriding the size's own.
    pub width: Option<f32>,
}

/// One glyph's flattened geometry.
pub struct Glyph {
    /// FNV-1a 64 of the reference `<symbol>` this was flattened from.
    pub source: u64,
    pub lines: &'static [Line],
    /// One convex polygon per fill piece.
    pub fills: &'static [&'static [Point]],
}

/// An in-scope glyph of the reference sprite.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Icon {
    Play,
    Pause,
    Previous,
    Next,
    Shuffle,
    Repeat,
    Home,
    Search,
    Library,
    Panel,
    ArrowLeft,
    ArrowRight,
    ChevronRight,
    ChevronDown,
    Heart,
    HeartFilled,
    More,
    Close,
    Plus,
    Volume,
    VolumeLow,
    VolumeMute,
    Devices,
    Queue,
    Expand,
    Logo,
    Clock,
    Grid,
    List,
}

impl Icon {
    pub const ALL: [Self; 29] = [
        Self::Play,
        Self::Pause,
        Self::Previous,
        Self::Next,
        Self::Shuffle,
        Self::Repeat,
        Self::Home,
        Self::Search,
        Self::Library,
        Self::Panel,
        Self::ArrowLeft,
        Self::ArrowRight,
        Self::ChevronRight,
        Self::ChevronDown,
        Self::Heart,
        Self::HeartFilled,
        Self::More,
        Self::Close,
        Self::Plus,
        Self::Volume,
        Self::VolumeLow,
        Self::VolumeMute,
        Self::Devices,
        Self::Queue,
        Self::Expand,
        Self::Logo,
        Self::Clock,
        Self::Grid,
        Self::List,
    ];

    /// The flattened geometry.
    pub fn glyph(self) -> Glyph {
        data::glyph(self)
    }

    /// The `<symbol>` id this glyph was flattened from.
    pub fn symbol_id(self) -> &'static str {
        data::symbol_id(self)
    }
}

/// The shapes for one glyph, placed in `rect`.
///
/// The caller snaps `rect` with `GuiRounding::round_to_pixels`; the box is
/// `size.side` wide and the point scale is `side / 24`.
pub fn shapes(icon: Icon, rect: Rect, size: Size, color: Color32) -> Vec<Shape> {
    shapes_of(&icon.glyph(), rect, size, color)
}

/// Adds one glyph to `painter`.
pub fn paint(painter: &Painter, icon: Icon, rect: Rect, size: Size, color: Color32) {
    painter.extend(shapes(icon, rect, size, color));
}

/// The shapes for a glyph, placed in `rect`.
fn shapes_of(glyph: &Glyph, rect: Rect, size: Size, color: Color32) -> Vec<Shape> {
    let scale = size.side / GRID;
    let mut shapes = Vec::new();

    for piece in glyph.fills {
        let points = place(piece, rect, scale);
        if points.len() >= 3 {
            shapes.push(Shape::convex_polygon(points, color, Stroke::NONE));
        }
    }

    for line in glyph.lines {
        let width = line.width.unwrap_or(size.stroke) * scale;
        let points = place(line.points, rect, scale);
        if points.len() < 2 {
            continue;
        }
        let rounded = round_corners(&points, width * 0.5);
        if line.closed {
            shapes.push(Shape::closed_line(rounded, Stroke::new(width, color)));
        } else {
            shapes.push(Shape::line(rounded.clone(), Stroke::new(width, color)));
            shapes.extend(cap_discs(&rounded, false, width, color));
        }
    }

    shapes
}

/// Maps viewBox points into `rect`.
fn place(points: &[Point], rect: Rect, scale: f32) -> Vec<Pos2> {
    points
        .iter()
        .map(|&(x, y)| rect.min + egui::vec2(x * scale, y * scale))
        .collect()
}

/// A disc at each free end of an open line.
///
/// epaint extrudes a square end; the reference rounds it, and a disc of the
/// stroke's own radius costs two circles.
pub fn cap_discs(points: &[Pos2], closed: bool, width: f32, color: Color32) -> Vec<Shape> {
    if closed || points.len() < 2 {
        return Vec::new();
    }
    let (first, last) = (points[0], points[points.len() - 1]);
    if first == last {
        return Vec::new();
    }
    let radius = width * 0.5;
    vec![
        Shape::circle_filled(first, radius, color),
        Shape::circle_filled(last, radius, color),
    ]
}

/// Turns joins sharper than the tessellator's mitre into short arcs.
///
/// A near-straight join is left alone, because epaint only cuts corners that
/// turn by more than a right angle and a fillet there would only distort the
/// line.
pub fn round_corners(points: &[Pos2], radius: f32) -> Vec<Pos2> {
    if points.len() <= 2 || radius <= 0.0 {
        return points.to_vec();
    }
    let mut out = Vec::with_capacity(points.len() * 2);
    out.push(points[0]);
    for window in points.windows(3) {
        match fillet(window[0], window[1], window[2], radius) {
            Some(mut arc) => out.append(&mut arc),
            None => out.push(window[1]),
        }
    }
    out.push(points[points.len() - 1]);
    out
}

/// The turn below which a join is treated as smooth.
const SMOOTH_TURN: f32 = 0.35;
/// The angular step a fillet is sampled at.
const ARC_STEP: f32 = std::f32::consts::FRAC_PI_8;

/// The arc that replaces vertex `b` between `a` and `c`, or `None` to keep it.
fn fillet(a: Pos2, b: Pos2, c: Pos2, radius: f32) -> Option<Vec<Pos2>> {
    let d1 = a - b;
    let d2 = c - b;
    let (l1, l2) = (d1.length(), d2.length());
    if l1 < f32::EPSILON || l2 < f32::EPSILON {
        return None;
    }
    let (u1, u2) = (d1 / l1, d2 / l2);
    let angle = u1.dot(u2).clamp(-1.0, 1.0).acos();
    if std::f32::consts::PI - angle < SMOOTH_TURN {
        return None;
    }
    let half = angle * 0.5;
    let tan = half.tan();
    if tan <= f32::EPSILON {
        return None;
    }
    let tangent = (radius / tan).min(l1 * 0.5).min(l2 * 0.5);
    if tangent <= f32::EPSILON {
        return None;
    }
    let sin = half.sin();
    if sin <= f32::EPSILON {
        return None;
    }
    let bisector = (u1 + u2).normalized();
    if !bisector.is_finite() {
        return None;
    }
    let center = b + bisector * (radius / sin);
    let start = (b + u1 * tangent - center).angle();
    let end = (b + u2 * tangent - center).angle();
    let mut delta = end - start;
    while delta > std::f32::consts::PI {
        delta -= std::f32::consts::TAU;
    }
    while delta < -std::f32::consts::PI {
        delta += std::f32::consts::TAU;
    }
    let segments = ((delta.abs() / ARC_STEP).ceil() as usize).max(2);
    let mut arc = Vec::with_capacity(segments + 1);
    for step in 0..=segments {
        let angle = start + delta * (step as f32 / segments as f32);
        arc.push(center + egui::vec2(angle.cos(), angle.sin()) * radius);
    }
    Some(arc)
}

#[cfg(test)]
mod tests;
