use super::*;
use crate::theme::space;

fn hit() -> Rect {
    Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 200.0))
}

fn zone(hit: Rect, list: Rect, first: usize, count: usize) -> Zone {
    Zone {
        hit,
        list,
        row_h: extent::ROW_H,
        first,
        count,
    }
}

fn pointer(x: f32, y: f32) -> Option<Pos2> {
    Some(pos2(x, y))
}

#[test]
fn a_reorder_carries_its_landing_and_is_not_a_queue_move() {
    let hit = hit();
    let zone = zone(hit, hit, 0, 4);
    let landing = resolve(pointer(150.0, 100.0), Some(&zone)).unwrap();
    assert_eq!(Target::Reorder(landing), Target::Reorder(landing));
    assert_ne!(
        Target::Reorder(landing),
        Target::At(landing),
        "the same boundary, but a move in another list"
    );
    assert_eq!(Target::Reorder(landing).marker(), Some(landing.marker));
}

#[test]
fn a_pointer_outside_the_hit_area_lands_nothing() {
    let hit = hit();
    let zone = zone(hit, hit, 0, 4);
    assert_eq!(resolve(pointer(150.0, 250.0), Some(&zone)), None);
    assert_eq!(resolve(pointer(-10.0, 100.0), Some(&zone)), None);
}

#[test]
fn no_pointer_lands_nothing() {
    let hit = hit();
    let zone = zone(hit, hit, 0, 4);
    assert_eq!(resolve(None, Some(&zone)), None);
}

#[test]
fn no_zone_lands_nothing() {
    assert_eq!(resolve(pointer(150.0, 100.0), None), None);
}

#[test]
fn the_top_half_of_row_j_lands_before_it_and_the_bottom_half_after_it() {
    let hit = hit();
    let zone = zone(hit, hit, 5, 4);
    let row_h = extent::ROW_H;

    let before = resolve(pointer(150.0, 2.0 * row_h + 10.0), Some(&zone)).unwrap();
    assert_eq!(before.boundary, 7);
    assert_eq!(before.marker.top(), 2.0 * row_h);

    let after = resolve(pointer(150.0, 2.0 * row_h + 34.0), Some(&zone)).unwrap();
    assert_eq!(after.boundary, 8);
    assert_eq!(after.marker.top(), 3.0 * row_h);
}

#[test]
fn over_the_caption_lands_before_the_first_row() {
    let hit = hit();
    let list = Rect::from_min_max(pos2(0.0, 36.0), hit.max);
    let zone = zone(hit, list, 5, 4);
    let landing = resolve(pointer(150.0, 12.0), Some(&zone)).unwrap();
    assert_eq!(landing.boundary, 5);
    assert_eq!(landing.marker.top(), list.top());
}

#[test]
fn below_the_last_row_lands_after_it_with_the_marker_on_its_bottom_edge() {
    let hit = hit();
    let list = Rect::from_min_max(hit.min, pos2(300.0, 176.0));
    let zone = zone(hit, list, 0, 4);
    let landing = resolve(pointer(150.0, 190.0), Some(&zone)).unwrap();
    assert_eq!(landing.boundary, 4);
    assert_eq!(landing.marker.bottom(), list.top() + 4.0 * extent::ROW_H);
}

#[test]
fn an_empty_list_lands_at_first_with_the_marker_at_the_list_top() {
    let hit = hit();
    let list = Rect::from_min_max(pos2(0.0, 36.0), hit.max);
    let zone = zone(hit, list, 7, 0);
    let landing = resolve(pointer(150.0, 100.0), Some(&zone)).unwrap();
    assert_eq!(landing.boundary, 7);
    assert_eq!(landing.marker.top(), list.top());
    assert_eq!(landing.marker.height(), stroke::RULE);
}

#[test]
fn the_marker_is_two_points_inside_the_row_it_precedes() {
    let hit = hit();
    let zone = zone(hit, hit, 0, 4);
    let landing = resolve(pointer(150.0, 2.0 * extent::ROW_H + 4.0), Some(&zone)).unwrap();
    assert_eq!(landing.marker.top(), 2.0 * extent::ROW_H);
    assert_eq!(landing.marker.height(), stroke::RULE);
    assert_eq!(landing.marker.left(), hit.left());
    assert_eq!(landing.marker.width(), hit.width());
}

#[test]
fn the_marker_is_pinned_inside_the_visible_rect_when_the_list_is_scrolled() {
    let hit = hit();
    let list = Rect::from_min_max(pos2(0.0, -100.0), pos2(300.0, 120.0));
    let zone = zone(hit, list, 3, 10);
    let landing = resolve(pointer(150.0, 5.0), Some(&zone)).unwrap();
    assert_eq!(landing.marker.top(), hit.top());
}

#[test]
fn the_marker_stays_pinned_while_the_boundary_follows_the_scroll() {
    let hit = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 120.0));
    let mut previous = None;
    for step in 0..3 {
        let top = hit.top() - step as f32 * extent::ROW_H;
        let list = Rect::from_min_max(pos2(0.0, top), pos2(300.0, top + 220.0));
        let zone = zone(hit, list, 0, 10);
        let landing = resolve(pointer(150.0, hit.bottom() - 1.0), Some(&zone)).unwrap();

        assert_eq!(landing.marker.bottom(), hit.bottom());
        if let Some(previous) = previous {
            assert_eq!(landing.boundary, previous + 1);
        }
        previous = Some(landing.boundary);
    }
}

#[test]
fn a_degenerate_pitch_does_not_panic() {
    let hit = hit();
    for row_h in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let zone = Zone {
            hit,
            list: hit,
            row_h,
            first: 2,
            count: 4,
        };
        let landing = resolve(pointer(150.0, 100.0), Some(&zone)).unwrap();
        assert_eq!(landing.boundary, 2);
        assert!(landing.marker.top().is_finite());
    }
}

/// A 28pt button centred on (100, 100), which is the size a small control
/// keeps while its hit area grows to 44.
fn button() -> Rect {
    Rect::from_center_size(pos2(100.0, 100.0), Vec2::splat(extent::BUTTON))
}

#[test]
fn an_area_is_a_44pt_square_on_its_buttons_centre() {
    let area = Area::around(button());
    assert_eq!(area.hit.size(), Vec2::splat(extent::DROP_HIT));
    assert_eq!(area.hit.center(), button().center());
}

#[test]
fn a_pointer_inside_an_area_joins_the_end() {
    let area = Area::around(button());
    let target = target(Some(area.hit.center()), None, Some(&area));
    assert_eq!(target, Some(Target::End));
}

#[test]
fn a_pointer_in_the_area_but_outside_the_glyph_still_joins_the_end() {
    let area = Area::around(button());
    let outside_the_glyph = pos2(area.hit.center().x + 19.0, area.hit.center().y);
    assert!(!button().contains(outside_the_glyph));
    assert_eq!(
        target(Some(outside_the_glyph), None, Some(&area)),
        Some(Target::End)
    );
}

#[test]
fn a_pointer_outside_the_area_lands_nothing() {
    let area = Area::around(button());
    assert_eq!(target(Some(pos2(200.0, 200.0)), None, Some(&area)), None);
}

#[test]
fn the_area_edge_is_inside_and_a_hair_past_it_is_not() {
    let area = Area::around(button());
    assert_eq!((area.hit.left(), area.hit.right()), (78.0, 122.0));
    assert_eq!((area.hit.top(), area.hit.bottom()), (78.0, 122.0));

    for edge in [
        pos2(122.0, 100.0),
        pos2(78.0, 78.0),
        pos2(78.0, 122.0),
        pos2(122.0, 122.0),
    ] {
        assert_eq!(
            target(Some(edge), None, Some(&area)),
            Some(Target::End),
            "{edge:?}"
        );
    }
    for past in [pos2(122.01, 100.0), pos2(100.0, 77.99), pos2(77.99, 122.01)] {
        assert_eq!(target(Some(past), None, Some(&area)), None, "{past:?}");
    }
}

#[test]
fn no_pointer_and_no_targets_land_nothing() {
    let area = Area::around(button());
    assert_eq!(target(None, None, Some(&area)), None);
    assert_eq!(target(None, None, None), None);
}

/// The playlists' rows box: a 4-row list of `H`-tall rows at `extent::ROW_H`.
fn slots(list: Rect, count: usize) -> Zone {
    Zone {
        hit: list,
        list,
        row_h: extent::ROW_H + space::PX,
        first: 0,
        count,
    }
}

fn drawn() -> f32 {
    extent::ROW_H
}

#[test]
fn a_pointer_over_a_row_names_that_row() {
    let list = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 4.0 * extent::ROW_H));
    let zone = slots(list, 4);
    let pitch = extent::ROW_H + space::PX;

    let found = slot_at(pointer(150.0, 2.0 * pitch + 10.0), Some(&zone), drawn()).unwrap();

    assert_eq!(found.row, 2);
    assert_eq!(found.rect.top(), 2.0 * pitch);
    assert_eq!(found.rect.height(), drawn());
}

#[test]
fn the_gap_between_two_rows_belongs_to_the_row_above() {
    let list = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 4.0 * extent::ROW_H));
    let zone = slots(list, 4);
    let pitch = extent::ROW_H + space::PX;
    let in_the_gap = pos2(150.0, pitch - space::PX / 2.0);

    let found = slot_at(Some(in_the_gap), Some(&zone), drawn()).unwrap();

    assert_eq!(found.row, 0, "a hairline gap is the row it follows");
}

#[test]
fn below_the_last_row_is_no_row() {
    let list = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 4.0 * extent::ROW_H));
    let mut zone = slots(list, 2);
    zone.hit = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 200.0));

    assert_eq!(slot_at(pointer(150.0, 150.0), Some(&zone), drawn()), None);
}

#[test]
fn outside_the_hit_area_is_no_row() {
    let list = Rect::from_min_size(pos2(0.0, 10.0), vec2(300.0, 4.0 * extent::ROW_H));
    let zone = slots(list, 4);

    assert_eq!(slot_at(pointer(150.0, 2.0), Some(&zone), drawn()), None);
    assert_eq!(slot_at(pointer(-1.0, 20.0), Some(&zone), drawn()), None);
    assert_eq!(slot_at(None, Some(&zone), drawn()), None);
    assert_eq!(slot_at(pointer(150.0, 20.0), None, drawn()), None);
}

#[test]
fn a_scrolled_list_names_the_row_under_the_pointer() {
    let pitch = extent::ROW_H + space::PX;
    let visible = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 200.0));
    // The list's own top is row 0, seven pitches above the visible top.
    let list = Rect::from_min_size(pos2(0.0, -7.0 * pitch), vec2(300.0, 400.0));
    let mut zone = slots(list, 20);
    zone.hit = visible;

    let found = slot_at(pointer(150.0, 2.0 * pitch + 4.0), Some(&zone), drawn()).unwrap();

    assert_eq!(found.row, 9, "row 7 is the first one on screen");
    assert_eq!(found.rect.top(), 2.0 * pitch);
}

#[test]
fn the_fill_is_the_drawn_row_not_the_pitch() {
    let list = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 4.0 * extent::ROW_H));
    let zone = slots(list, 4);

    let found = slot_at(pointer(150.0, 10.0), Some(&zone), drawn()).unwrap();

    assert_eq!(
        found.rect.height(),
        extent::ROW_H,
        "the gap under the row takes no fill"
    );
    assert!(found.rect.height() < zone.row_h);
}

#[test]
fn the_fill_is_clipped_to_the_visible_list() {
    let pitch = extent::ROW_H + space::PX;
    let visible = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 30.0));
    // Row 0 sits a pitch above the visible top, and row 1 is cut off below.
    let list = Rect::from_min_size(pos2(0.0, -pitch), vec2(300.0, 400.0));
    let mut zone = slots(list, 10);
    zone.hit = visible;

    let found = slot_at(pointer(150.0, 20.0), Some(&zone), drawn()).unwrap();

    assert_eq!(found.row, 1);
    assert_eq!(found.rect.top(), visible.top());
    assert_eq!(
        found.rect.bottom(),
        visible.bottom(),
        "the fill stops where the list does"
    );
}

#[test]
fn a_degenerate_pitch_names_no_row() {
    let list = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 200.0));
    for row_h in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let mut zone = slots(list, 4);
        zone.row_h = row_h;
        assert_eq!(slot_at(pointer(150.0, 10.0), Some(&zone), drawn()), None);
    }
}

#[test]
fn an_add_draws_a_fill_and_no_rule() {
    let slot = Slot {
        row: 1,
        rect: Rect::from_min_size(pos2(0.0, 44.0), vec2(300.0, extent::ROW_H)),
    };
    let target = Target::Add(slot);
    assert_eq!(target.fill(), Some(slot.rect));
    assert_eq!(target.marker(), None);
}

#[test]
fn a_rootlist_move_draws_a_rule_and_no_fill() {
    let landing = resolve(
        pointer(150.0, 2.0 * extent::ROW_H + 10.0),
        Some(&slots(
            Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 4.0 * extent::ROW_H)),
            4,
        )),
    )
    .unwrap();

    let target = Target::Rootlist(landing);

    assert_eq!(target.marker(), Some(landing.marker));
    assert_eq!(target.fill(), None);
}

#[test]
fn no_target_draws_both_a_rule_and_a_fill() {
    let landing = Landing {
        boundary: 1,
        marker: Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, stroke::RULE)),
    };
    let slot = Slot {
        row: 0,
        rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, extent::ROW_H)),
    };

    for target in [
        Target::At(landing),
        Target::Reorder(landing),
        Target::Rootlist(landing),
        Target::Add(slot),
        Target::End,
    ] {
        // No wildcard: a variant added without a class is a compile error.
        let fills = match target {
            Target::Add(_) => true,
            Target::At(_) | Target::Reorder(_) | Target::Rootlist(_) | Target::End => false,
        };
        assert_eq!(target.fill().is_some(), fills, "{target:?}");
        assert!(
            !(target.marker().is_some() && target.fill().is_some()),
            "{target:?} must not draw both a rule and a fill"
        );
    }
}

#[test]
fn a_zone_wins_over_an_area_it_overlaps() {
    let hit = hit();
    let zone = zone(hit, hit, 0, 4);
    let area = Area::around(button());
    let pointer = pointer(100.0, 100.0);
    assert!(hit.contains(pos2(100.0, 100.0)));
    assert_eq!(
        target(pointer, Some(&zone), Some(&area)),
        resolve(pointer, Some(&zone)).map(Target::At)
    );
    assert_eq!(
        target(pointer, Some(&zone), None),
        target(pointer, Some(&zone), Some(&area)),
        "the area must not change a zone's answer"
    );
}

#[test]
fn a_zone_alone_resolves_as_before() {
    let hit = hit();
    let zone = zone(hit, hit, 0, 4);
    let pointer = pointer(150.0, 2.0 * extent::ROW_H + 10.0);
    let landing = resolve(pointer, Some(&zone)).unwrap();
    assert_eq!(landing.boundary, 2);
    assert_eq!(
        target(pointer, Some(&zone), None),
        Some(Target::At(landing))
    );
}
