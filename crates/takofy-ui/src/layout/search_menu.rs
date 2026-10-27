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
mod tests {
    use super::*;
    use crate::layout::WINDOW_MIN;
    use egui::Pos2;

    fn window() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))
    }

    fn field() -> Rect {
        Rect::from_min_size(pos2(100.0, 12.0), vec2(320.0, extent::SEARCH_H))
    }

    fn row_of(width: f32) -> Rect {
        Rect::from_min_size(pos2(10.0, 20.0), vec2(width, extent::ROW_H))
    }

    fn assert_close(left: f32, right: f32) {
        assert!((left - right).abs() < 0.01, "{left} is not {right}");
    }

    fn outline(parts: Parts) -> [(&'static str, Rect); 4] {
        [
            ("marker", parts.marker),
            ("art", parts.art),
            ("text", parts.text),
            ("glyph", parts.glyph),
        ]
    }

    #[test]
    fn the_menu_is_the_fields_width_and_six_below_it() {
        let field = field();
        let height = rows_height(5, true);
        let menu = menu_rect(field, window(), height);
        assert_eq!(menu.width(), field.width());
        assert_eq!(menu.left(), field.left());
        assert_eq!(menu.top(), field.bottom() + extent::MENU_GAP);
        assert_eq!(menu.height(), height);
        assert_eq!(extent::MENU_GAP, 6.0);
    }

    #[test]
    fn the_menu_is_as_tall_as_the_view_asks() {
        let field = field();
        assert_eq!(
            menu_rect(field, window(), line_height()).height(),
            extent::ROW_H
        );
        assert_eq!(
            menu_rect(field, window(), rows_height(5, false)).height(),
            220.0
        );
        // A frame cannot be turned inside out, and a height that is not a number
        // is not a height.
        assert_eq!(menu_rect(field, window(), -20.0).height(), 0.0);
        assert_eq!(menu_rect(field, window(), f32::NAN).height(), 0.0);
        assert_eq!(menu_rect(field, window(), f32::INFINITY).height(), 0.0);
    }

    #[test]
    fn five_rows_plus_a_floor_is_six_rows() {
        assert_eq!(rows_height(5, true), 6.0 * extent::ROW_H);
        assert_eq!(rows_height(5, true), 264.0);
        assert_eq!(rows_height(0, true), extent::ROW_H);
    }

    #[test]
    fn without_a_floor_five_rows_are_five_rows() {
        assert_eq!(rows_height(5, false), 5.0 * extent::ROW_H);
        assert_eq!(rows_height(5, false), 220.0);
        assert_eq!(rows_height(0, false), 0.0);
    }

    #[test]
    fn the_empty_line_is_one_row() {
        assert_eq!(line_height(), extent::ROW_H);
        assert_eq!(line_height(), rows_height(1, false));
    }

    #[test]
    fn the_rows_tile_down_from_the_menu_top() {
        let menu = menu_rect(field(), window(), rows_height(5, true));
        for index in 0..6 {
            let row = row_rect(menu, index);
            assert_eq!(row.left(), menu.left());
            assert_eq!(row.width(), menu.width());
            assert_eq!(row.height(), extent::ROW_H);
            assert_close(row.top(), menu.top() + index as f32 * extent::ROW_H);
        }
        assert_eq!(
            row_rect(menu, 0).bottom(),
            row_rect(menu, 1).top(),
            "the rows tile without a gap"
        );
        assert_eq!(
            row_rect(menu, 5).bottom(),
            menu.bottom(),
            "the six rows fill the frame the view asked for"
        );
    }

    #[test]
    fn the_cover_and_the_glyph_are_their_tokens() {
        let parts = row_parts(row_rect(
            menu_rect(field(), window(), rows_height(5, true)),
            0,
        ));
        assert_eq!(parts.art.width(), extent::MENU_COVER);
        assert_eq!(parts.art.height(), extent::MENU_COVER);
        assert_eq!(parts.glyph.width(), icon::Size::SM.side);
        assert_eq!(parts.glyph.height(), icon::Size::SM.side);
        assert_eq!(parts.marker.width(), stroke::RULE);
        assert_eq!(parts.marker.height(), extent::ROW_H);
    }

    #[test]
    fn a_narrow_row_drops_the_cover_and_keeps_the_text() {
        // The field's floor: the cover is the column this row gives up, so the
        // text starts where the art would have been.
        let narrow = row_parts(row_of(extent::SEARCH_MIN_W));
        assert_eq!(narrow.art.width(), 0.0, "the narrow row keeps no art");
        assert_eq!(narrow.art.height(), 0.0);
        assert_eq!(
            narrow.text.left(),
            row_of(extent::SEARCH_MIN_W).left() + space::S3,
            "the text starts at the inner left margin"
        );
        assert!(
            narrow.text.width() >= extent::MENU_TEXT_MIN,
            "the narrow text is {} wide, under the {} floor",
            narrow.text.width(),
            extent::MENU_TEXT_MIN
        );

        // The field's ceiling: the cover is back and the text is wider still.
        let wide = row_parts(row_of(extent::SEARCH_MAX_W));
        assert_eq!(wide.art.width(), extent::MENU_COVER);
        assert_eq!(wide.art.height(), extent::MENU_COVER);
        assert!(
            wide.text.width() > narrow.text.width(),
            "{} is not wider than {}",
            wide.text.width(),
            narrow.text.width()
        );

        // Across the field's whole range the text never collapses and the glyph
        // never loses its slot.
        let steps = (extent::SEARCH_MAX_W - extent::SEARCH_MIN_W) as usize;
        for step in 0..=steps {
            let width = extent::SEARCH_MIN_W + step as f32;
            let parts = row_parts(row_of(width));
            assert_eq!(parts.glyph.width(), icon::Size::SM.side, "at {width}");
            assert!(parts.text.width() > 0.0, "the text vanishes at {width}");
            assert!(
                parts.art.width() == 0.0 || parts.text.width() >= extent::MENU_TEXT_MIN,
                "the art crowds the text at {width}"
            );
        }
    }

    #[test]
    fn the_pointer_tick_fits_inside_the_marker_slot() {
        let parts = row_parts(row_rect(
            menu_rect(field(), window(), rows_height(5, true)),
            0,
        ));
        let tick_h = space::S4;
        assert_eq!(tick_h, 16.0);
        let tick = Rect::from_min_size(
            pos2(
                parts.marker.center().x - stroke::RULE / 2.0,
                parts.marker.center().y - tick_h / 2.0,
            ),
            vec2(stroke::RULE, tick_h),
        );
        assert_eq!(parts.marker.height(), extent::ROW_H);
        assert!(tick.height() <= parts.marker.height());
        assert!(
            tick.top() >= parts.marker.top() && tick.bottom() <= parts.marker.bottom(),
            "the tick at {tick:?} leaves the marker {marker:?}",
            marker = parts.marker
        );
        assert!(tick.left() >= parts.marker.left());
        assert!(tick.right() <= parts.marker.right() + 0.01);
    }

    #[test]
    fn the_parts_tile_without_overlap_and_stay_inside_the_row() {
        for width in [
            extent::SEARCH_MIN_W,
            extent::SEARCH_BASIS_W,
            extent::SEARCH_MAX_W,
        ] {
            let row = Rect::from_min_size(pos2(10.0, 20.0), vec2(width, extent::ROW_H));
            let parts = row_parts(row);
            for (name, rect) in outline(parts) {
                assert!(
                    rect.left() >= row.left() - 0.01 && rect.right() <= row.right() + 0.01,
                    "{name} at {width} is outside {row:?}: {rect:?}"
                );
                assert!(
                    rect.top() >= row.top() - 0.01 && rect.bottom() <= row.bottom() + 0.01,
                    "{name} at {width} is outside {row:?}: {rect:?}"
                );
                assert!(rect.width() >= 0.0, "{name} at {width} is negative");
                assert!(rect.height() >= 0.0, "{name} at {width} is negative");
            }
            assert!(
                parts.marker.right() <= parts.art.left() + 0.01,
                "the marker overlaps the art at {width}"
            );
            assert!(
                parts.art.right() <= parts.text.left() + 0.01,
                "the art overlaps the text at {width}"
            );
            assert!(
                parts.text.right() <= parts.glyph.left() + 0.01,
                "the text overlaps the glyph at {width}"
            );
        }
    }

    #[test]
    fn a_narrow_row_clamps_to_zero_without_inverting() {
        for width in [0.0, 1.0, space::S3, extent::MENU_COVER, 60.0] {
            let row = Rect::from_min_size(pos2(10.0, 20.0), vec2(width, extent::ROW_H));
            let parts = row_parts(row);
            for (name, rect) in outline(parts) {
                assert!(rect.width() >= 0.0, "{name} at {width}: {rect:?}");
                assert!(rect.height() >= 0.0, "{name} at {width}: {rect:?}");
                assert!(
                    rect.left() >= row.left() - 0.01 && rect.right() <= row.right() + 0.01,
                    "{name} at {width} is outside {row:?}: {rect:?}"
                );
            }
            assert!(parts.art.right() <= parts.text.left() + 0.01, "{width}");
            assert!(parts.text.right() <= parts.glyph.left() + 0.01, "{width}");
        }
    }

    #[test]
    fn a_field_near_the_right_edge_yields_a_menu_inside_the_window() {
        let window = Rect::from_min_size(Pos2::ZERO, vec2(WINDOW_MIN[0], WINDOW_MIN[1]));
        // The field runs 40 points past the window's right edge, as a field at
        // the topbar's right padding can once the trail grows.
        let field = Rect::from_min_size(
            pos2(window.right() - extent::SEARCH_MIN_W + 40.0, 8.0),
            vec2(extent::SEARCH_MIN_W, extent::SEARCH_H),
        );
        let menu = menu_rect(field, window, line_height());
        assert_eq!(menu.width(), field.width());
        assert!(menu.left() >= window.left() - 0.01, "{menu:?}");
        assert!(menu.right() <= window.right() + 0.01, "{menu:?}");
    }

    #[test]
    fn a_field_wider_than_the_window_is_capped_to_it() {
        let window = Rect::from_min_size(Pos2::ZERO, vec2(WINDOW_MIN[0], WINDOW_MIN[1]));
        let field = Rect::from_min_size(
            pos2(-50.0, 8.0),
            vec2(window.width() + 160.0, extent::SEARCH_H),
        );
        let menu = menu_rect(field, window, line_height());
        assert_eq!(menu.left(), window.left());
        assert_eq!(menu.width(), window.width());
        assert!(menu.right() <= window.right() + 0.01);
    }

    #[test]
    fn the_minimum_window_holds_the_tallest_menu() {
        // The menu hangs from the field, which sits inside the topbar, so the
        // topbar's own height is the most room the field can take above it.
        let tallest = extent::TOPBAR_H + extent::MENU_GAP + bands_height(&[3, 2, 2], true);
        assert!(
            WINDOW_MIN[1] >= tallest,
            "the minimum window is {} tall but the tallest menu needs {tallest}",
            WINDOW_MIN[1]
        );
    }

    #[test]
    fn the_bands_height_stacks_heads_rows_and_the_rules_between_them() {
        assert_close(bands_height(&[3, 2, 2], true), 435.0);
        assert_close(bands_height(&[3], false), 159.0);
        assert_close(bands_height(&[3, 2], false), 275.0);
        assert_close(bands_height(&[1, 1], true), 187.0);
        assert_close(bands_height(&[], false), 0.0);
        // A floor under no band is a shape the index cannot produce, and the
        // floor needs a band to sit under.
        assert_close(bands_height(&[], true), 0.0);
        assert_close(extent::MENU_HEAD_H, 27.0);
    }

    #[test]
    fn place_tiles_every_band_and_the_floor_with_no_gap() {
        let menu = menu_rect(field(), window(), bands_height(&[3, 2, 2], true));
        let placed = place(menu, &[3, 2, 2], true, 1.0);
        assert_eq!(placed.bands.len(), 3);
        let mut bottom = menu.top();
        for band in &placed.bands {
            if let Some(rule) = band.rule {
                assert_close(rule.top(), bottom);
                assert_close(rule.height(), stroke::HAIRLINE);
                assert_eq!(rule.width(), menu.width());
                bottom = rule.bottom();
            }
            assert_close(band.head.top(), bottom);
            assert_close(band.head.height(), extent::MENU_HEAD_H);
            assert_eq!(band.head.width(), menu.width());
            bottom = band.head.bottom();
            for row in &band.rows {
                assert_close(row.top(), bottom);
                assert_close(row.height(), extent::ROW_H);
                assert_eq!(row.width(), menu.width());
                bottom = row.bottom();
            }
        }
        let floor = placed.floor.expect("a floor was asked for");
        assert_close(floor.top(), bottom);
        assert_close(floor.height(), extent::ROW_H);
        assert_close(floor.bottom(), menu.bottom());
    }

    #[test]
    fn every_edge_lands_on_the_physical_pixel_grid() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let menu = menu_rect(field(), window(), bands_height(&[3, 2, 2], true));
            let placed = place(menu, &[3, 2, 2], true, scale);
            let rects = placed
                .bands
                .iter()
                .flat_map(|band| {
                    band.rule
                        .into_iter()
                        .chain([band.head])
                        .chain(band.rows.clone())
                })
                .chain(placed.floor);
            let mut edges = Vec::new();
            for rect in rects {
                edges.push(rect.top());
                edges.push(rect.bottom());
            }
            for edge in &edges {
                let pixels = edge * scale;
                assert_close(pixels, pixels.round());
            }
        }
    }

    #[test]
    fn neighbouring_rects_share_one_edge() {
        let menu = menu_rect(field(), window(), bands_height(&[3, 2, 2], true));
        let placed = place(menu, &[3, 2, 2], true, 1.25);
        let mut previous_bottom: Option<f32> = None;
        let mut rects = Vec::new();
        for band in &placed.bands {
            rects.extend(band.rule);
            rects.push(band.head);
            rects.extend(band.rows.iter().copied());
        }
        rects.extend(placed.floor);
        for rect in rects {
            if let Some(bottom) = previous_bottom {
                assert_eq!(rect.top(), bottom);
            }
            previous_bottom = Some(rect.bottom());
        }
    }

    #[test]
    fn the_first_band_has_no_rule_and_every_later_band_does() {
        let menu = menu_rect(field(), window(), bands_height(&[2, 1, 1], false));
        let placed = place(menu, &[2, 1, 1], false, 1.0);
        assert_eq!(placed.bands[0].rule, None);
        for band in &placed.bands[1..] {
            let rule = band.rule.expect("a later band sits under a rule");
            assert_eq!(rule.width(), menu.width());
            assert_close(rule.top() + stroke::HAIRLINE, band.head.top());
        }
        assert_eq!(placed.floor, None);
    }

    #[test]
    fn the_last_placed_rect_ends_at_the_menus_bottom() {
        for (rows, floor) in [(&[3, 2, 2][..], true), (&[3][..], false)] {
            let menu = menu_rect(field(), window(), bands_height(rows, floor));
            let placed = place(menu, rows, floor, 1.0);
            let last = placed.floor.unwrap_or_else(|| {
                placed
                    .bands
                    .last()
                    .and_then(|band| band.rows.last().copied())
                    .expect("a band with rows")
            });
            assert_close(last.bottom(), menu.bottom());
        }
    }

    #[test]
    fn the_place_driven_menu_height_uses_the_pixel_grid_the_view_passes() {
        let rows = [3, 2, 2];
        let menu = menu_rect(field(), window(), bands_height(&rows, true));
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let placed = place(menu, &rows, true, scale);
            let last = placed.floor.expect("a floor was asked for");
            assert_close(last.bottom(), menu.bottom().round_to_pixels(scale));
            assert_close(menu.height(), bands_height(&rows, true));
            for edge in [placed.bands[0].head.top(), last.bottom()] {
                let pixels = edge * scale;
                assert_close(pixels, pixels.round());
            }
        }
    }

    #[test]
    fn place_yields_finite_non_negative_rects_for_a_degenerate_menu() {
        let degenerate = [
            Rect::ZERO,
            Rect::from_min_size(pos2(f32::NAN, 0.0), vec2(100.0, 400.0)),
            Rect::from_min_size(pos2(0.0, f32::NAN), vec2(100.0, 400.0)),
            Rect::from_min_size(pos2(0.0, 0.0), vec2(f32::INFINITY, 400.0)),
            Rect::from_min_size(pos2(0.0, 0.0), vec2(-100.0, -400.0)),
        ];
        for menu in degenerate {
            let placed = place(menu, &[3, 2, 2], true, 1.0);
            let rects = placed
                .bands
                .iter()
                .flat_map(|band| {
                    band.rule
                        .into_iter()
                        .chain([band.head])
                        .chain(band.rows.clone())
                })
                .chain(placed.floor);
            for rect in rects {
                assert!(
                    rect.min.x.is_finite()
                        && rect.min.y.is_finite()
                        && rect.width().is_finite()
                        && rect.height().is_finite(),
                    "{rect:?} in {menu:?} is not finite"
                );
                assert!(rect.width() >= 0.0, "{rect:?} in {menu:?}");
                assert!(rect.height() >= 0.0, "{rect:?} in {menu:?}");
            }
        }
    }

    #[test]
    fn a_non_finite_field_or_window_falls_back_to_the_documented_frame() {
        let height = line_height();
        let nan_field = Rect::from_min_size(pos2(f32::NAN, 0.0), vec2(100.0, extent::SEARCH_H));
        let nan_window = Rect::from_min_size(pos2(0.0, f32::NAN), vec2(100.0, 100.0));
        for (field, window) in [(nan_field, window()), (field(), nan_window)] {
            let menu = menu_rect(field, window, height);
            assert_eq!(menu.min, pos2(0.0, extent::MENU_GAP), "in {field:?}");
            assert_eq!(menu.width(), 0.0, "in {field:?}");
            assert_eq!(menu.height(), height, "in {field:?}");
        }
    }

    #[test]
    fn a_zero_or_non_finite_rect_yields_finite_rows() {
        let height = rows_height(5, true);
        let degenerate = [
            Rect::from_min_size(pos2(f32::NAN, 0.0), vec2(100.0, extent::ROW_H)),
            Rect::from_min_size(pos2(0.0, f32::NAN), vec2(100.0, extent::ROW_H)),
            Rect::from_min_size(pos2(0.0, 0.0), vec2(f32::INFINITY, extent::ROW_H)),
            Rect::from_min_size(pos2(0.0, 0.0), vec2(-100.0, -extent::ROW_H)),
            Rect::ZERO,
        ];
        // The field and the window each get the bad rects in turn.
        let cases: Vec<(Rect, Rect)> = degenerate
            .iter()
            .flat_map(|&rect| [(rect, window()), (field(), rect)])
            .collect();
        for (field, window) in cases {
            let menu = menu_rect(field, window, height);
            // A real row, not a zero-height menu: the parts are what the view
            // would draw into this row.
            let parts = row_parts(row_rect(menu, 0));
            for (name, rect) in outline(parts) {
                assert!(
                    rect.min.x.is_finite()
                        && rect.min.y.is_finite()
                        && rect.width().is_finite()
                        && rect.height().is_finite(),
                    "{name} in {field:?} / {window:?} is not finite: {rect:?}"
                );
                assert!(
                    rect.width() >= 0.0,
                    "{name} in {field:?} / {window:?}: {rect:?}"
                );
                assert!(
                    rect.height() >= 0.0,
                    "{name} in {field:?} / {window:?}: {rect:?}"
                );
            }
        }
    }
}
