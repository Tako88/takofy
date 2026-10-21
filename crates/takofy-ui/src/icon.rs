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
    Heart,
    More,
    Close,
    Plus,
    Volume,
    VolumeLow,
    VolumeMute,
    Devices,
    Queue,
    Expand,
    Screen,
    Logo,
    Clock,
    Grid,
    List,
}

impl Icon {
    pub const ALL: [Self; 28] = [
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
        Self::Heart,
        Self::More,
        Self::Close,
        Self::Plus,
        Self::Volume,
        Self::VolumeLow,
        Self::VolumeMute,
        Self::Devices,
        Self::Queue,
        Self::Expand,
        Self::Screen,
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
mod tests {
    use super::*;
    use crate::theme::Tokens;

    fn color() -> Color32 {
        Tokens::DARK.palette.text
    }

    fn box16() -> Rect {
        Rect::from_min_size(Pos2::ZERO, egui::vec2(16.0, 16.0))
    }

    const FILLS: &[&[Point]] = &[
        &[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0)],
        &[(6.0, 6.0), (8.0, 6.0), (8.0, 8.0)],
    ];
    const FILL_GLYPH: Glyph = Glyph {
        source: 0,
        lines: &[],
        fills: FILLS,
    };

    const LINES: &[Line] = &[
        Line {
            points: &[(0.0, 0.0), (16.0, 16.0)],
            closed: false,
            width: None,
        },
        Line {
            points: &[(0.0, 0.0), (0.0, 16.0), (16.0, 16.0)],
            closed: true,
            width: None,
        },
    ];
    const LINE_GLYPH: Glyph = Glyph {
        source: 0,
        lines: LINES,
        fills: &[],
    };

    #[test]
    fn a_fill_becomes_one_convex_polygon_per_piece() {
        let shapes = shapes_of(&FILL_GLYPH, box16(), Size::BASE, color());
        let filled: Vec<_> = shapes
            .iter()
            .filter_map(|shape| match shape {
                Shape::Path(path) if path.fill == color() => Some(path),
                _ => None,
            })
            .collect();
        assert_eq!(filled.len(), 2);
        assert!(filled.iter().all(|path| path.closed));
    }

    #[test]
    fn an_open_line_is_a_path_and_a_closed_one_closes() {
        let shapes = shapes_of(&LINE_GLYPH, box16(), Size::BASE, color());
        let paths: Vec<_> = shapes
            .iter()
            .filter_map(|shape| match shape {
                Shape::Path(path) => Some(path),
                _ => None,
            })
            .collect();
        assert_eq!(paths.len(), 2);
        assert!(!paths[0].closed, "the first line is open");
        assert!(paths[1].closed, "the second line closes");
    }

    #[test]
    fn points_scale_from_the_grid_to_the_icon_box() {
        const LINE: &[Line] = &[Line {
            points: &[(0.0, 0.0), (12.0, 12.0)],
            closed: false,
            width: None,
        }];
        let glyph = Glyph {
            source: 0,
            lines: LINE,
            fills: &[],
        };
        let shapes = shapes_of(&glyph, box16(), Size::BASE, color());
        let Shape::Path(path) = &shapes[0] else {
            panic!("expected a path");
        };
        assert_eq!(path.points, vec![Pos2::ZERO, egui::pos2(8.0, 8.0)]);
    }

    #[test]
    fn stroke_width_scales_with_the_icon_size() {
        let shapes = shapes_of(&LINE_GLYPH, box16(), Size::MD, color());
        let Shape::Path(path) = &shapes[0] else {
            panic!("expected a path");
        };
        assert!((path.stroke.width - 1.2).abs() < 1e-6);
    }

    #[test]
    fn a_width_override_beats_the_size_stroke() {
        const LINE: &[Line] = &[Line {
            points: &[(0.0, 0.0), (12.0, 12.0)],
            closed: false,
            width: Some(2.0),
        }];
        let glyph = Glyph {
            source: 0,
            lines: LINE,
            fills: &[],
        };
        let shapes = shapes_of(&glyph, box16(), Size::LG, color());
        let Shape::Path(path) = &shapes[0] else {
            panic!("expected a path");
        };
        assert!((path.stroke.width - 2.0 * 22.0 / 24.0).abs() < 1e-6);
    }

    #[test]
    fn placement_is_translation_only() {
        let origin = shapes_of(&FILL_GLYPH, box16(), Size::BASE, color());
        let moved = Rect::from_min_size(egui::pos2(5.0, 7.0), egui::vec2(16.0, 16.0));
        let shifted = shapes_of(&FILL_GLYPH, moved, Size::BASE, color());
        let (Shape::Path(a), Shape::Path(b)) = (&origin[0], &shifted[0]) else {
            panic!("expected paths");
        };
        for (before, after) in a.points.iter().zip(&b.points) {
            assert_eq!(*after - *before, egui::vec2(5.0, 7.0));
        }
    }

    // --- guards over the real data ---

    fn symbol_source(id: &str) -> &'static str {
        let reference = data::REFERENCE;
        let needle = format!("<symbol id=\"{id}\"");
        let start = reference.find(&needle).expect("symbol in the reference");
        let rest = &reference[start..];
        let end = rest.find("</symbol>").expect("closing tag") + "</symbol>".len();
        &rest[..end]
    }

    fn fnv1a(bytes: &[u8]) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }

    fn is_convex(points: &[Point]) -> bool {
        let count = points.len();
        if count < 3 {
            return false;
        }
        let mut sign = 0_i32;
        for i in 0..count {
            let (ax, ay) = points[i];
            let (bx, by) = points[(i + 1) % count];
            let (cx, cy) = points[(i + 2) % count];
            let cross = (bx - ax) * (cy - by) - (by - ay) * (cx - bx);
            let current = if cross > 1e-4 {
                1
            } else if cross < -1e-4 {
                -1
            } else {
                0
            };
            if current != 0 {
                if sign == 0 {
                    sign = current;
                } else if sign != current {
                    return false;
                }
            }
        }
        true
    }

    #[test]
    fn every_icon_has_a_glyph_and_a_symbol_id() {
        for icon in Icon::ALL {
            let glyph = icon.glyph();
            assert!(
                !glyph.lines.is_empty() || !glyph.fills.is_empty(),
                "{icon:?} has no geometry"
            );
            assert!(icon.symbol_id().starts_with("i-"), "{icon:?}");
        }
    }

    #[test]
    fn every_symbol_id_exists_in_the_reference() {
        for icon in Icon::ALL {
            let needle = format!("id=\"{}\"", icon.symbol_id());
            assert!(
                data::REFERENCE.contains(&needle),
                "{:?} names {} which is not in the sprite",
                icon,
                icon.symbol_id()
            );
        }
    }

    #[test]
    fn each_glyph_records_the_hash_of_its_reference_symbol() {
        for icon in Icon::ALL {
            let source = symbol_source(icon.symbol_id());
            assert_eq!(
                icon.glyph().source,
                fnv1a(source.as_bytes()),
                "{:?}: regenerate the table from the reference",
                icon
            );
        }
    }

    #[test]
    fn every_fill_piece_is_convex_with_three_or_more_points() {
        for icon in Icon::ALL {
            for piece in icon.glyph().fills {
                assert!(
                    piece.len() >= 3,
                    "{:?}: a fill piece has {} points",
                    icon,
                    piece.len()
                );
                assert!(is_convex(piece), "{icon:?} has a concave or twisted fill");
            }
        }
    }

    #[test]
    fn every_line_has_two_or_more_points() {
        for icon in Icon::ALL {
            for line in icon.glyph().lines {
                assert!(
                    line.points.len() >= 2,
                    "{:?}: a line has {} points",
                    icon,
                    line.points.len()
                );
            }
        }
    }

    #[test]
    fn the_density_icons_are_filled_squares_and_stroked_rows() {
        let grid = Icon::Grid.glyph();
        assert_eq!(grid.fills.len(), 4, "the grid is four filled tiles");
        assert!(grid.lines.is_empty(), "the grid is drawn as fills alone");

        let list = Icon::List.glyph();
        assert!(list.fills.is_empty(), "the list is drawn as strokes alone");
        assert_eq!(list.lines.len(), 6, "the list is three rows of two strokes");

        for icon in [Icon::Grid, Icon::List] {
            let shapes = shapes_of(&icon.glyph(), box16(), Size::BASE, color());
            assert!(!shapes.is_empty(), "{icon:?} draws nothing");
        }
    }

    #[test]
    fn every_point_lies_inside_the_viewbox_with_the_overlap_margin() {
        for icon in Icon::ALL {
            let glyph = icon.glyph();
            let mut points = Vec::new();
            for line in glyph.lines {
                points.extend_from_slice(line.points);
            }
            for piece in glyph.fills {
                points.extend_from_slice(piece);
            }
            for &(x, y) in &points {
                assert!(
                    (-OVERLAP..=GRID + OVERLAP).contains(&x),
                    "{:?}: x = {x}",
                    icon
                );
                assert!(
                    (-OVERLAP..=GRID + OVERLAP).contains(&y),
                    "{:?}: y = {y}",
                    icon
                );
            }
        }
    }
}
