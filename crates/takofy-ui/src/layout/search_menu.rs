//! The search menu's geometry: the frame under the field, and one row's parts.
//!
//! The menu is an overlay `Area` that the field itself sizes: it is the
//! field's own width, `extent::MENU_GAP` below it, and clamped back into the
//! window so a field near an edge cannot hang a menu off screen. The view keeps
//! the `Area` unconstrained, so this module is the single place that decides
//! where the menu lies.
//!
//! A row is the reference's `2.25rem minmax(0, 1fr) auto` grid with a `space-3`
//! gap and `space-3`/`space-1` padding, plus the rule/tick slot at the margin
//! that the two highlight tiers share. Every rect is clamped inside the
//! row: a row too narrow for its parts yields zero-width rects rather than
//! inverted ones.
//!
//! A row loses a column before it loses its height, which is the reference's own
//! narrow-window rule. The cover is that column: when a row cannot leave the
//! title `extent::MENU_TEXT_MIN` beside the `extent::MENU_COVER` art, the art is
//! dropped, the text starts at the inner left margin and runs to the glyph. The
//! glyph keeps its slot in both cases.
//!
//! The marker slot is the row's full height, so the first and last rows must be
//! inset or clipped by the view: the menu's `radius::MD` corners would otherwise
//! cut the marker at the ends. This geometry cannot do that for the view — it
//! does not know which row is the menu's first or last.

use egui::emath::GuiRounding as _;
use egui::{Rect, pos2, vec2};

use crate::theme::{extent, icon, space, stroke};

/// One row's parts, left to right.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Parts {
    /// The slot at the margin that the keyboard's rule and the pointer's tick
    /// are drawn in. Full row height, because the rule is the row's full
    /// height and the `space::S4` pointer tick — `stroke::RULE` wide — is centred
    /// vertically inside it, far clear of the row's own `space::S1` padding.
    pub marker: Rect,
    /// The `extent::MENU_COVER` square art. Zero-sized when the row is too
    /// narrow to keep it: the text then starts at the inner left margin.
    pub art: Rect,
    /// The title over the artist and album.
    pub text: Rect,
    /// The action glyph at the right edge: play, or pause on the loaded row,
    /// because Enter plays rather than opens.
    pub glyph: Rect,
}

/// The menu's frame under `field`, `height` tall.
///
/// The width is the field's own and the top is `field.bottom() +
/// extent::MENU_GAP`. Horizontally it shifts left to clear the window's right
/// edge first, then caps its width to the window's, so a field near either edge
/// still yields a frame inside the window.
///
/// The height is the caller's, never this module's: the view passes
/// [`rows_height`] for a menu with results, or [`line_height`] for the single
/// muted line. A negative or non-finite height becomes zero.
///
/// A non-finite `field` or `window` is replaced rather than clamped: the frame
/// is then `(0, extent::MENU_GAP)` with width zero and the caller's height, so a
/// bad read draws nothing instead of putting the menu somewhere arbitrary.
///
/// The returned frame is the menu's real extent, so the view can anchor its
/// `Area` here and hit-test and shadow the same rect it draws.
pub fn menu_rect(field: Rect, window: Rect, height: f32) -> Rect {
    let height = if height.is_finite() {
        height.max(0.0)
    } else {
        0.0
    };

    if !is_sane(field) || !is_sane(window) {
        return Rect::from_min_size(pos2(0.0, extent::MENU_GAP), vec2(0.0, height));
    }

    let field = sane(field);
    let window = sane(window);

    let mut width = field.width();
    let mut left = field.left();

    // Shift left first: a field near the right edge moves the whole menu back
    // rather than narrowing it.
    if left + width > window.right() {
        left = window.right() - width;
    }
    // Then cap: a field wider than the window cannot give the menu a width the
    // window does not have.
    if width > window.width() {
        width = window.width();
    }
    // A field that starts outside the window, or a shift that overshot, is
    // pulled back in.
    left = left.clamp(window.left(), (window.right() - width).max(window.left()));

    // On egui's own grid, like any widget: the field is measured from text, so
    // its rect is rarely whole, and a card a third of a point off draws its
    // hairline soft and trips egui's debug build with an orange "Unaligned".
    let rect = Rect::from_min_size(
        pos2(left, field.bottom() + extent::MENU_GAP),
        vec2(width, height),
    );
    Rect::from_min_max(rect.min.round_ui(), rect.max.round_ui())
}

/// The height of `shown` result rows plus, when the answer is longer than the
/// menu, the floor row.
pub fn rows_height(shown: usize, floor: bool) -> f32 {
    (shown + usize::from(floor)) as f32 * extent::ROW_H
}

/// The height of the single muted line drawn for no matches or a failed read:
/// one row, since that is the room the design gives a line of text.
pub fn line_height() -> f32 {
    extent::ROW_H
}

/// One band's vertical slice: the rule above it, its heading, and its rows.
#[derive(Clone, Debug, PartialEq)]
pub struct BandRects {
    /// The hairline that separates this band from the one above; `None` on the
    /// first band.
    pub rule: Option<Rect>,
    pub head: Rect,
    pub rows: Vec<Rect>,
}

/// Where each band and the floor row sit inside `menu`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Placement {
    pub bands: Vec<BandRects>,
    pub floor: Option<Rect>,
}

/// How tall `rows` bands are, with the rules between them and the floor row.
///
/// `rows` is the shown row count per band, so a band's own heading and the rule
/// above it are part of its height. A heading is `extent::MENU_HEAD_H`; the
/// first band has no rule because the menu's top edge is the rule.
pub fn bands_height(rows: &[usize], floor: bool) -> f32 {
    let bands = rows
        .iter()
        .enumerate()
        .map(|(index, &count)| {
            let rule = if index > 0 { stroke::HAIRLINE } else { 0.0 };
            rule + extent::MENU_HEAD_H + count as f32 * extent::ROW_H
        })
        .sum::<f32>();
    bands
        + if floor && !rows.is_empty() {
            extent::ROW_H
        } else {
            0.0
        }
}

/// Stacks `rows` bands and the floor from `menu`'s top edge, with no gaps.
///
/// Every rect is the menu's full width and is stacked directly under the one
/// before, so `bands_height(rows, floor)` is the height this needs to fill
/// `menu` exactly. A zero or non-finite menu yields zero-width, finite rects.
pub fn place(menu: Rect, rows: &[usize], floor: bool, pixels_per_point: f32) -> Placement {
    let menu = sane(menu);
    let left = menu.left().round_to_pixels(pixels_per_point);
    let right = menu.right().round_to_pixels(pixels_per_point);

    // Each shared edge is snapped once and read by both neighbours, so the bands
    // meet on the pixel grid instead of a fraction of a point apart, which would
    // draw a soft rule and a seam a hairline wide.
    let mut edges = vec![menu.top()];
    let mut top = menu.top();
    for (index, &count) in rows.iter().enumerate() {
        if index > 0 {
            top += stroke::HAIRLINE;
            edges.push(top);
        }
        top += extent::MENU_HEAD_H;
        edges.push(top);
        for _ in 0..count {
            top += extent::ROW_H;
            edges.push(top);
        }
    }
    let floor = floor && !rows.is_empty();
    if floor {
        top += extent::ROW_H;
        edges.push(top);
    }
    let edges: Vec<f32> = edges
        .iter()
        .map(|edge| edge.round_to_pixels(pixels_per_point))
        .collect();

    let mut cursor = 0;
    let mut placement = Placement::default();
    for (index, &count) in rows.iter().enumerate() {
        let rule = (index > 0).then(|| slice(&edges, &mut cursor, left, right));
        let head = slice(&edges, &mut cursor, left, right);
        let row_rects = (0..count)
            .map(|_| slice(&edges, &mut cursor, left, right))
            .collect();
        placement.bands.push(BandRects {
            rule,
            head,
            rows: row_rects,
        });
    }
    if floor {
        placement.floor = Some(slice(&edges, &mut cursor, left, right));
    }
    placement
}

/// The slice between the cursor's edge and the next, advancing the cursor.
fn slice(edges: &[f32], cursor: &mut usize, left: f32, right: f32) -> Rect {
    let rect = Rect::from_min_max(pos2(left, edges[*cursor]), pos2(right, edges[*cursor + 1]));
    *cursor += 1;
    rect
}

/// The `index`-th row inside `menu`, the rows tiling with no gap.
///
/// The pitch is the shared `extent::ROW_H`, so the floor and the empty line are
/// rows like any other. A zero or non-finite menu yields zero-size rows rather
/// than panicking ones.
pub fn row_rect(menu: Rect, index: usize) -> Rect {
    let menu = sane(menu);
    let top = menu.top() + index as f32 * extent::ROW_H;
    Rect::from_min_size(pos2(menu.left(), top), vec2(menu.width(), extent::ROW_H))
}

/// Splits one row into its marker, art, text and glyph.
pub fn row_parts(row: Rect) -> Parts {
    let row = sane(row);
    let inner = inner_rect(row);

    let marker = Rect::from_min_size(
        pos2(row.left(), row.top()),
        vec2(stroke::RULE.min(row.width()), row.height()),
    );

    // The glyph takes its icon's own box, flush with the inner right edge. The
    // reference draws this action glyph at 0.875 rem, 14 points, which is the
    // midpoint of this design's 12 and 16 and so an icon size the sprite does
    // not have. The menu takes `Size::SM`, the box the app's own rows draw their
    // play and action glyphs in, so a menu action is never larger than a row's.
    let glyph_side = icon::Size::SM.side.min(inner.width()).min(inner.height());
    let glyph = Rect::from_min_size(
        pos2(
            inner.right() - glyph_side,
            inner.center().y - glyph_side / 2.0,
        ),
        vec2(glyph_side, glyph_side),
    );

    // The cover keeps its `MENU_COVER` box only while the text beside it still
    // clears `MENU_TEXT_MIN`; a row that cannot hold both drops the art and
    // hands its width to the text, because a narrow row loses a column before it
    // loses a line. The glyph keeps its slot either way.
    let text_with_art = inner.width() - glyph_side - extent::MENU_COVER - 2.0 * space::S3;
    let side = if text_with_art >= extent::MENU_TEXT_MIN {
        extent::MENU_COVER
            .min((inner.width() - glyph_side - space::S3).max(0.0))
            .min(inner.height())
    } else {
        0.0
    };
    let art = Rect::from_min_size(
        pos2(inner.left(), inner.center().y - side / 2.0),
        vec2(side, side),
    );

    let text_left = if side > 0.0 {
        art.right() + space::S3
    } else {
        inner.left()
    };
    let text_right = (glyph.left() - space::S3).max(text_left).min(inner.right());

    Parts {
        marker,
        art,
        text: Rect::from_min_max(
            pos2(text_left, inner.top()),
            pos2(text_right, inner.bottom()),
        ),
        glyph,
    }
}

/// The row's content box: the row less its own `space-3`/`space-1` padding.
fn inner_rect(row: Rect) -> Rect {
    let left = (row.left() + space::S3).min(row.right());
    let right = (row.right() - space::S3).max(left).min(row.right());
    let top = (row.top() + space::S1).min(row.bottom());
    let bottom = (row.bottom() - space::S1).max(top).min(row.bottom());
    Rect::from_min_max(pos2(left, top), pos2(right, bottom))
}

/// Whether a rect has a finite origin and size, so the clamps below are real
/// arithmetic and not NaN.
pub(super) fn is_sane(rect: Rect) -> bool {
    rect.min.x.is_finite()
        && rect.min.y.is_finite()
        && rect.width().is_finite()
        && rect.height().is_finite()
}

/// A rect with a finite origin and a non-negative size, so a bad caller cannot
/// turn an arithmetic clamp into a NaN.
pub(super) fn sane(rect: Rect) -> Rect {
    if !is_sane(rect) {
        return Rect::ZERO;
    }
    Rect::from_min_size(
        rect.min,
        vec2(rect.width().max(0.0), rect.height().max(0.0)),
    )
}

#[cfg(test)]
mod tests;
