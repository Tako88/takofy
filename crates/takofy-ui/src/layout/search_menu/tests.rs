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
