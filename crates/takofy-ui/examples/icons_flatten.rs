//! Flatten the reference's icon sprite into `icon/data.rs`.
//!
//! std-only. It reads `docs/reference/main.js`, picks the in-scope `<symbol>`s,
//! turns their SVG elements into polylines, hand-decomposes the three concave
//! fills, and writes the Rust table. The conversion rules are documented in
//! `crates/takofy-ui/assets/icons/README.md`. Run it with
//! `cargo run -p takofy-ui --example icons_flatten`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
/// The angular step arcs are flattened at. Fine for a 22 pt glyph.
const ARC_STEP: f32 = std::f32::consts::FRAC_PI_8;
/// Sides on a full circle or ellipse. Fine for a 22 pt glyph.
const CURVE_SIDES: usize = 64;

#[derive(Clone, Copy, PartialEq, Debug)]
struct P(f32, f32);

struct OutLine {
    points: Vec<P>,
    closed: bool,
    width: Option<f32>,
}

struct GlyphOut {
    variant: &'static str,
    id: &'static str,
    source: u64,
    lines: Vec<OutLine>,
    fills: Vec<Vec<P>>,
}

/// The in-scope glyphs, in the order the enum declares them.
const SCOPE: &[(&str, &str)] = &[
    ("i-play", "Play"),
    ("i-pause", "Pause"),
    ("i-prev", "Previous"),
    ("i-next", "Next"),
    ("i-shuffle", "Shuffle"),
    ("i-repeat", "Repeat"),
    ("i-home", "Home"),
    ("i-search", "Search"),
    ("i-library", "Library"),
    ("i-panel", "Panel"),
    ("i-arrow-left", "ArrowLeft"),
    ("i-arrow-right", "ArrowRight"),
    ("i-heart", "Heart"),
    ("i-more", "More"),
    ("i-plus", "Plus"),
    ("i-volume", "Volume"),
    ("i-volume-low", "VolumeLow"),
    ("i-volume-mute", "VolumeMute"),
    ("i-devices", "Devices"),
    ("i-queue", "Queue"),
    ("i-expand", "Expand"),
    ("i-screen", "Screen"),
    ("i-logo", "Logo"),
    ("i-clock", "Clock"),
];

fn main() {
    let reference = fs::read_to_string(PathBuf::from(ROOT).join("docs/reference/main.js"))
        .expect("read docs/reference/main.js");
    let symbols = symbols(&reference);

    let mut glyphs = Vec::new();
    for &(id, variant) in SCOPE {
        let source = symbols
            .get(id)
            .unwrap_or_else(|| panic!("{id} is not in the sprite"));
        glyphs.push(flatten(variant, id, source));
    }

    let mut out = String::new();
    write_table(&mut out, &glyphs);
    let target = PathBuf::from(ROOT).join("crates/takofy-ui/src/icon/data.rs");
    fs::write(&target, out).expect("write data.rs");
    eprintln!("wrote {}", target.display());
}

/// Extracts every `<symbol id="…">…</symbol>` body, keyed by id.
///
/// The value is the exact `<symbol>…</symbol>` text, which is what the guard
/// hashes.
fn symbols(reference: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut rest = reference;
    while let Some(start) = rest.find("<symbol ") {
        let after = &rest[start..];
        let end = after
            .find("</symbol>")
            .map(|e| e + "</symbol>".len())
            .expect("closed symbol");
        let source = &after[..end];
        if let Some(id) = attr(source, "id") {
            out.insert(id, source.to_string());
        }
        rest = &after[end..];
    }
    out
}

/// One attribute of an element or symbol tag.
fn attr(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_string())
}

/// A parsed `<element …/>`.
struct Element {
    tag: String,
    attrs: BTreeMap<String, String>,
}

impl Element {
    fn get(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(String::as_str)
    }
}

/// Turns one symbol's inner XML into lines and fills.
fn flatten(variant: &'static str, id: &'static str, source: &str) -> GlyphOut {
    let inner_start = source.find('>').expect("symbol open tag") + 1;
    let inner_end = source.rfind("</symbol>").expect("symbol close tag");
    let inner = &source[inner_start..inner_end];

    let mut lines = Vec::new();
    let mut fills = Vec::new();
    for element in elements(inner) {
        let fill = element.get("fill") == Some("currentColor");
        let stroke = element.get("stroke") == Some("currentColor");
        let width = element
            .get("stroke-width")
            .and_then(|w| w.parse::<f32>().ok());

        let geometry: Vec<(Vec<P>, bool)> = match element.tag.as_str() {
            "path" => element
                .get("d")
                .map(parse_path)
                .unwrap_or_default()
                .into_iter()
                .map(|sub| (sub.points, sub.closed))
                .collect(),
            "rect" => vec![(rect_points(&element), true)],
            "circle" => vec![(ellipse_points(&element, true), true)],
            "ellipse" => vec![(ellipse_points(&element, false), true)],
            other => {
                eprintln!("skipping unsupported <{other}>");
                continue;
            }
        };

        for (points, closed) in geometry {
            if points.len() < 2 {
                continue;
            }
            if fill {
                fills.push(points.clone());
            }
            if stroke {
                lines.push(OutLine {
                    points,
                    closed,
                    width,
                });
            }
        }
    }

    match id {
        "i-logo" => fills = logo_fills(),
        "i-home" => fills = home_fills(),
        "i-volume" | "i-volume-low" | "i-volume-mute" => fills = speaker_fills(),
        _ => {}
    }

    GlyphOut {
        variant,
        id,
        source: fnv1a(source.as_bytes()),
        lines,
        fills,
    }
}

/// Splits an inner XML string into self-closing elements.
fn elements(inner: &str) -> Vec<Element> {
    let mut out = Vec::new();
    let mut rest = inner;
    while let Some(start) = rest.find('<') {
        let after = &rest[start + 1..];
        let end = match after.find('>') {
            Some(end) => end,
            None => break,
        };
        let mut body = &after[..end];
        if let Some(stripped) = body.strip_suffix('/') {
            body = stripped;
        }
        out.push(parse_element(body));
        rest = &after[end + 1..];
    }
    out
}

fn parse_element(body: &str) -> Element {
    let body = body.trim();
    let (tag, attrs_text) = match body.find(char::is_whitespace) {
        Some(split) => (&body[..split], &body[split..]),
        None => (body, ""),
    };
    let mut attrs = BTreeMap::new();
    let chars: Vec<char> = attrs_text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let name_start = i;
        while i < chars.len() && chars[i] != '=' && !chars[i].is_whitespace() {
            i += 1;
        }
        if name_start == i {
            break;
        }
        let name: String = chars[name_start..i].iter().collect();
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i < chars.len() && chars[i] == '=' {
            i += 1;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if i < chars.len() && (chars[i] == '"' || chars[i] == '\'') {
                let quote = chars[i];
                i += 1;
                let value_start = i;
                while i < chars.len() && chars[i] != quote {
                    i += 1;
                }
                let value: String = chars[value_start..i].iter().collect();
                i += 1;
                attrs.insert(name, value);
            }
        }
    }
    Element {
        tag: tag.to_string(),
        attrs,
    }
}

/// A path subpath: its points and whether it was closed with `z`.
struct Subpath {
    points: Vec<P>,
    closed: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Tok {
    Cmd(char),
    Num(f32),
}

fn tokenize(d: &str) -> Vec<Tok> {
    let bytes = d.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_ascii_alphabetic() {
            out.push(Tok::Cmd(c));
            i += 1;
        } else if c == ',' || c.is_ascii_whitespace() {
            i += 1;
        } else {
            let start = i;
            if c == '-' || c == '+' {
                i += 1;
            }
            // A number has at most one decimal point: `-2.6.7` is two numbers.
            let mut seen_dot = false;
            while i < bytes.len() {
                let byte = bytes[i];
                if byte.is_ascii_digit() {
                    i += 1;
                } else if byte == b'.' && !seen_dot {
                    seen_dot = true;
                    i += 1;
                } else {
                    break;
                }
            }
            if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
                i += 1;
                if i < bytes.len() && (bytes[i] == b'-' || bytes[i] == b'+') {
                    i += 1;
                }
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let value: f32 = d[start..i].parse().unwrap_or(0.0);
            out.push(Tok::Num(value));
        }
    }
    out
}

/// Reads the next number, or `None` if the next token is a command.
fn num(toks: &[Tok], i: &mut usize) -> Option<f32> {
    if let Some(Tok::Num(value)) = toks.get(*i) {
        *i += 1;
        Some(*value)
    } else {
        None
    }
}

/// Parses a `d` attribute into subpaths.
///
/// A second `M…z` run in one attribute is a second subpath, not a continuation:
/// a pause bar is a rectangle, not half a bow-tie.
fn parse_path(d: &str) -> Vec<Subpath> {
    let toks = tokenize(d);
    let mut subpaths: Vec<Subpath> = Vec::new();
    let mut sub: Option<Subpath> = None;
    let mut pos = P(0.0, 0.0);
    let mut sub_start = P(0.0, 0.0);
    let mut command = ' ';
    let mut i = 0;

    while i < toks.len() {
        if let Tok::Cmd(c) = toks[i] {
            command = c;
            i += 1;
        }

        match command {
            'M' | 'm' => {
                let (Some(x), Some(y)) = (num(&toks, &mut i), num(&toks, &mut i)) else {
                    break;
                };
                flush(&mut sub, &mut subpaths);
                pos = if command == 'm' {
                    P(pos.0 + x, pos.1 + y)
                } else {
                    P(x, y)
                };
                sub_start = pos;
                sub = Some(Subpath {
                    points: vec![pos],
                    closed: false,
                });
                command = if command == 'm' { 'l' } else { 'L' };
            }
            'L' | 'l' => {
                let (Some(x), Some(y)) = (num(&toks, &mut i), num(&toks, &mut i)) else {
                    break;
                };
                pos = if command == 'l' {
                    P(pos.0 + x, pos.1 + y)
                } else {
                    P(x, y)
                };
                push(&mut sub, pos, sub_start);
            }
            'H' | 'h' => {
                let Some(x) = num(&toks, &mut i) else { break };
                pos = if command == 'h' {
                    P(pos.0 + x, pos.1)
                } else {
                    P(x, pos.1)
                };
                push(&mut sub, pos, sub_start);
            }
            'V' | 'v' => {
                let Some(y) = num(&toks, &mut i) else { break };
                pos = if command == 'v' {
                    P(pos.0, pos.1 + y)
                } else {
                    P(pos.0, y)
                };
                push(&mut sub, pos, sub_start);
            }
            'A' | 'a' => {
                let (Some(rx), Some(ry), Some(_rot), Some(large), Some(sweep), Some(ex), Some(ey)) = (
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                ) else {
                    break;
                };
                let end = if command == 'a' {
                    P(pos.0 + ex, pos.1 + ey)
                } else {
                    P(ex, ey)
                };
                let arc = arc_points(pos, rx, ry, large != 0.0, sweep != 0.0, end);
                if let Some(active) = sub.as_mut() {
                    if arc.is_empty() {
                        active.points.push(end);
                    } else {
                        active.points.extend(arc);
                    }
                }
                pos = end;
            }
            'Z' | 'z' => {
                if let Some(active) = sub.as_mut() {
                    active.closed = true;
                }
                flush(&mut sub, &mut subpaths);
                pos = sub_start;
            }
            _ => {
                i += 1;
            }
        }
    }
    flush(&mut sub, &mut subpaths);
    subpaths
}

fn flush(sub: &mut Option<Subpath>, out: &mut Vec<Subpath>) {
    if let Some(active) = sub.take()
        && active.points.len() >= 2
    {
        out.push(active);
    }
}

fn push(sub: &mut Option<Subpath>, pos: P, start: P) {
    if let Some(active) = sub.as_mut() {
        active.points.push(pos);
    } else {
        *sub = Some(Subpath {
            points: vec![start, pos],
            closed: false,
        });
    }
}

/// SVG endpoint-to-center arc, sampled, excluding the start point.
fn arc_points(from: P, rx: f32, ry: f32, large: bool, sweep: bool, to: P) -> Vec<P> {
    let (x1, y1) = (from.0, from.1);
    let (x2, y2) = (to.0, to.1);
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx < 1e-9 || ry < 1e-9 {
        return Vec::new();
    }
    let dx = (x1 - x2) * 0.5;
    let dy = (y1 - y2) * 0.5;
    let lambda = (dx * dx) / (rx * rx) + (dy * dy) / (ry * ry);
    if lambda > 1.0 {
        let scale = lambda.sqrt();
        rx *= scale;
        ry *= scale;
    }
    let (rx2, ry2) = (rx * rx, ry * ry);
    let numerator = (rx2 * ry2 - rx2 * dy * dy - ry2 * dx * dx).max(0.0);
    let denominator = rx2 * dy * dy + ry2 * dx * dx;
    let root = if denominator <= 0.0 {
        0.0
    } else {
        numerator.sqrt() / denominator.sqrt()
    };
    let coefficient = if large == sweep { -root } else { root };
    let cxp = coefficient * (rx * dy / ry);
    let cyp = coefficient * (-ry * dx / rx);
    let cx = cxp + (x1 + x2) * 0.5;
    let cy = cyp + (y1 + y2) * 0.5;
    let ux = (dx - cxp) / rx;
    let uy = (dy - cyp) / ry;
    let vx = (-dx - cxp) / rx;
    let vy = (-dy - cyp) / ry;
    let start = uy.atan2(ux);
    let mut delta = (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    if !sweep && delta > 0.0 {
        delta -= std::f32::consts::TAU;
    }
    if sweep && delta < 0.0 {
        delta += std::f32::consts::TAU;
    }
    let steps = ((delta.abs() / ARC_STEP).ceil() as usize).max(2);
    (1..=steps)
        .map(|k| {
            let angle = start + delta * (k as f32 / steps as f32);
            P(cx + rx * angle.cos(), cy + ry * angle.sin())
        })
        .collect()
}

/// A `<rect>`, with `rx` corners.
fn rect_points(element: &Element) -> Vec<P> {
    let x = number(element, "x");
    let y = number(element, "y");
    let w = number(element, "width");
    let h = number(element, "height");
    let rx = number(element, "rx").min(w * 0.5).min(h * 0.5);
    if rx <= 0.05 {
        return vec![P(x, y), P(x + w, y), P(x + w, y + h), P(x, y + h)];
    }
    let mut points = Vec::new();
    let corners = [
        (P(x + w - rx, y + rx), 270.0, 360.0),
        (P(x + w - rx, y + h - rx), 0.0, 90.0),
        (P(x + rx, y + h - rx), 90.0, 180.0),
        (P(x + rx, y + rx), 180.0, 270.0),
    ];
    for (center, from, to) in corners {
        points.extend(arc_degrees(center, rx, rx, from, to));
    }
    points
}

/// A `<circle>` or `<ellipse>`, sampled as a closed polygon.
fn ellipse_points(element: &Element, circle: bool) -> Vec<P> {
    let cx = number(element, "cx");
    let cy = number(element, "cy");
    let rx = number(element, "r").max(if circle { 0.0 } else { number(element, "rx") });
    let ry = number(element, "r").max(if circle { 0.0 } else { number(element, "ry") });
    let rx = if rx > 0.0 { rx } else { number(element, "rx") };
    let ry = if ry > 0.0 { ry } else { number(element, "ry") };
    (0..CURVE_SIDES)
        .map(|i| {
            let angle = std::f32::consts::TAU * (i as f32 / CURVE_SIDES as f32);
            P(cx + rx * angle.cos(), cy + ry * angle.sin())
        })
        .collect()
}

fn number(element: &Element, name: &str) -> f32 {
    element
        .get(name)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0)
}

/// Samples an arc by degrees, excluding the start angle.
fn arc_degrees(center: P, rx: f32, ry: f32, from: f32, to: f32) -> Vec<P> {
    let from = from.to_radians();
    let to = to.to_radians();
    let delta = to - from;
    let steps = ((delta.abs() / ARC_STEP).ceil() as usize).max(2);
    (1..=steps)
        .map(|k| {
            let angle = from + delta * (k as f32 / steps as f32);
            P(center.0 + rx * angle.cos(), center.1 + ry * angle.sin())
        })
        .collect()
}

/// The T monogram, split into two overlapping rectangles.
fn logo_fills() -> Vec<Vec<P>> {
    vec![
        vec![P(2.5, 3.0), P(21.5, 3.0), P(21.5, 7.5), P(2.5, 7.5)],
        vec![P(9.5, 5.5), P(14.5, 5.5), P(14.5, 21.0), P(9.5, 21.0)],
    ]
}

/// The speaker shared by the three volume glyphs: box and cone, overlapping.
fn speaker_fills() -> Vec<Vec<P>> {
    vec![
        vec![P(7.6, 9.4), P(12.0, 5.4), P(12.0, 18.6), P(7.6, 14.6)],
        vec![P(4.0, 9.4), P(9.6, 9.4), P(9.6, 14.6), P(4.0, 14.6)],
    ]
}

/// The house, split into a roof block and two legs; the door is the gap.
fn home_fills() -> Vec<Vec<P>> {
    let mut left = vec![P(2.6, 13.2), P(8.6, 13.2), P(8.6, 21.4), P(4.0, 21.4)];
    left.extend(arc_degrees(P(4.0, 20.0), 1.4, 1.4, 90.0, 180.0));
    let mut right = vec![P(15.4, 13.2), P(21.4, 13.2), P(21.4, 20.0)];
    right.extend(arc_degrees(P(20.0, 20.0), 1.4, 1.4, 0.0, 90.0));
    right.push(P(15.4, 21.4));
    vec![
        vec![
            P(12.0, 2.9),
            P(2.6, 10.4),
            P(2.6, 15.2),
            P(21.4, 15.2),
            P(21.4, 10.4),
        ],
        left,
        right,
    ]
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn write_table(out: &mut String, glyphs: &[GlyphOut]) {
    out.push_str(
        "//! Flattened icon geometry, generated from `docs/reference/main.js`.\n\
         //!\n\
         //! Regenerate with `cargo run -p takofy-ui --example icons_flatten`; the\n\
         //! conversion is documented in `crates/takofy-ui/assets/icons/README.md`.\n\n\
         use super::{Glyph, Icon, Line, Point};\n\n\
         #[cfg(test)]\n\
         pub(super) const REFERENCE: &str =\n    include_str!(\"../../../../docs/reference/main.js\");\n\n",
    );

    for glyph in glyphs {
        let const_name = scream(glyph.variant);
        write!(out, "const {const_name}_LINES: &[Line] = ").expect("write");
        if glyph.lines.is_empty() {
            out.push_str("&[];\n");
        } else {
            out.push_str("&[\n");
            for line in &glyph.lines {
                out.push_str("    Line { points: &[");
                write_points(out, &line.points);
                let width = match line.width {
                    Some(w) => format!("Some({})", f(w)),
                    None => "None".to_string(),
                };
                writeln!(out, "], closed: {}, width: {width} }},", line.closed).expect("write");
            }
            out.push_str("];\n");
        }

        write!(out, "const {const_name}_FILLS: &[&[Point]] = ").expect("write");
        if glyph.fills.is_empty() {
            out.push_str("&[];\n\n");
        } else {
            out.push_str("&[\n");
            for piece in &glyph.fills {
                out.push_str("    &[");
                write_points(out, piece);
                out.push_str("],\n");
            }
            out.push_str("];\n\n");
        }
    }

    out.push_str("pub(super) fn glyph(icon: Icon) -> Glyph {\n    match icon {\n");
    for glyph in glyphs {
        let const_name = scream(glyph.variant);
        writeln!(
            out,
            "        Icon::{} => Glyph {{ source: 0x{:016x}, lines: {const_name}_LINES, fills: {const_name}_FILLS }},",
            glyph.variant, glyph.source
        )
        .expect("write");
    }
    out.push_str("    }\n}\n\n");

    out.push_str("pub(super) fn symbol_id(icon: Icon) -> &'static str {\n    match icon {\n");
    for glyph in glyphs {
        writeln!(out, "        Icon::{} => \"{}\",", glyph.variant, glyph.id).expect("write");
    }
    out.push_str("    }\n}\n");
}

fn write_points(out: &mut String, points: &[P]) {
    for (i, P(x, y)) in points.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write!(out, "({}, {})", f(*x), f(*y)).expect("write");
    }
}

/// Three-decimal rounding, with a decimal point always present.
fn f(value: f32) -> String {
    let mut text = format!("{value:.3}");
    while text.ends_with('0') && !text.ends_with(".0") && !text.ends_with("0.") {
        text.pop();
    }
    if !text.contains('.') {
        text.push_str(".0");
    }
    text
}

fn scream(variant: &str) -> String {
    let mut out = String::new();
    for (i, c) in variant.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}
