//! Where a carried row would land, and where the marker that says so sits.
//!
//! A list reports a `Zone`: the box a pointer has to be inside, the rows box,
//! their pitch and the first index it may take a drop at. A control that takes a
//! whole drop reports an `Area`, because an add has no position to choose.
//! Resolving the pointer against them is the whole of the drop arithmetic;
//! nothing here draws or touches a queue.

use egui::{Pos2, Rect, Vec2, pos2, vec2};

use crate::layout::table::boundary_at;
use crate::theme::{extent, stroke};

/// A list's rows, as a drop target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zone {
    /// The box a pointer has to be inside to land here, caption band included.
    pub hit: Rect,
    /// The rows themselves, which a caption may push down from `hit`.
    pub list: Rect,
    /// The pitch the rows were laid out at.
    pub row_h: f32,
    /// The index the zone's first row has in the list that owns it.
    pub first: usize,
    /// How many rows the zone holds.
    pub count: usize,
}

/// Where a drop lands, and the rule that shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Landing {
    /// The boundary the rows arrive at: before the entry now at this index.
    pub boundary: usize,
    /// The marker, a rule the width of the list.
    pub marker: Rect,
}

/// A target that takes a drop whole, with no position: "add" is an area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub hit: Rect,
}

impl Area {
    /// A `DROP_HIT` square on `button`'s centre, never smaller than the button.
    pub fn around(button: Rect) -> Self {
        Self {
            hit: Rect::from_center_size(button.center(), Vec2::splat(extent::DROP_HIT))
                .union(button),
        }
    }
}

/// What a drop does: land at a boundary, with its marker, or join the end with
/// none.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    At(Landing),
    End,
}

/// Resolves a pointer against a zone, or `None` when it lands nowhere.
pub fn resolve(pointer: Option<Pos2>, zone: Option<&Zone>) -> Option<Landing> {
    let pointer = pointer?;
    let zone = zone?;
    if !zone.hit.contains(pointer) {
        return None;
    }

    let row_h = if zone.row_h.is_finite() && zone.row_h > 0.0 {
        zone.row_h
    } else {
        0.0
    };
    let row = boundary_at(zone.list, pointer.y, row_h, zone.count);
    let boundary = zone.first + row;
    let top = if row < zone.count || zone.count == 0 {
        zone.list.top() + row as f32 * row_h
    } else {
        zone.list.top() + zone.count as f32 * row_h - stroke::RULE
    };

    // The rule holds at the visible edge while the list scrolls under it.
    let limit = (zone.hit.bottom() - stroke::RULE).max(zone.hit.top());
    let top = top.clamp(zone.hit.top(), limit);
    let marker = Rect::from_min_size(
        pos2(zone.list.left(), top),
        vec2(zone.list.width(), stroke::RULE),
    );
    Some(Landing { boundary, marker })
}

/// Resolves a pointer against the frame's targets. A zone the pointer is in
/// wins.
///
/// The area's edge is inside it: egui's `Rect::contains` is inclusive.
pub fn target(pointer: Option<Pos2>, zone: Option<&Zone>, area: Option<&Area>) -> Option<Target> {
    resolve(pointer, zone).map(Target::At).or_else(|| {
        let pointer = pointer?;
        area.filter(|area| area.hit.contains(pointer))
            .map(|_| Target::End)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
