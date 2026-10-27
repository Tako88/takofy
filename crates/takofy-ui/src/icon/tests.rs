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
fn chevron_down_is_one_open_polyline_matching_the_reference() {
    let glyph = data::glyph(Icon::ChevronDown);
    assert!(glyph.fills.is_empty());
    let [line] = glyph.lines else {
        panic!("expected one line");
    };
    assert!(!line.closed);
    assert_eq!(line.points, &[(6.0, 9.4), (12.0, 15.4), (18.0, 9.4)]);
}

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
fn the_filled_heart_is_fill_only_and_has_pieces() {
    let filled = Icon::HeartFilled.glyph();
    assert!(
        filled.lines.is_empty(),
        "a filled heart drawn with a stroke would show a seam"
    );
    assert!(!filled.fills.is_empty(), "the filled heart draws nothing");
    assert!(
        filled.fills.iter().all(|piece| piece.len() >= 3),
        "a fill piece has fewer than three points"
    );
}

#[test]
fn the_filled_heart_covers_the_outlines_extent() {
    fn bounds(points: &[Point]) -> (f32, f32, f32, f32) {
        points.iter().fold(
            (
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ),
            |(x0, y0, x1, y1), &(x, y)| (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
        )
    }
    let outline = Icon::Heart.glyph();
    let filled = Icon::HeartFilled.glyph();
    let mut outline_points = Vec::new();
    for line in outline.lines {
        outline_points.extend_from_slice(line.points);
    }
    let mut filled_points = Vec::new();
    for piece in filled.fills {
        filled_points.extend_from_slice(piece);
    }
    let (ox, oy, ox1, oy1) = bounds(&outline_points);
    let (fx, fy, fx1, fy1) = bounds(&filled_points);
    assert!(
        fx <= ox + 0.01 && fy <= oy + 0.01 && fx1 >= ox1 - 0.01 && fy1 >= oy1 - 0.01,
        "the fill {fx},{fy}..{fx1},{fy1} does not cover the outline {ox},{oy}..{ox1},{oy1}"
    );
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
