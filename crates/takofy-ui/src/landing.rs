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

/// One row of a list, and the box a fill for it covers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    /// The row's index in the list that owns it.
    pub row: usize,
    /// The row's own box, clipped to what the list shows.
    pub rect: Rect,
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

/// What a drop does: land at a boundary, with its marker, join the end with
/// none, or take a whole row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    At(Landing),
    /// Land in the rows of the list the row was lifted from, at a boundary.
    Reorder(Landing),
    /// Land in the sidebar's playlists, at a boundary.
    Rootlist(Landing),
    /// Add to the playlist whose row this is.
    Add(Slot),
    End,
}

impl Target {
    /// The rule the target draws where a carried block lands, if it draws one.
    ///
    /// An add has no position to show, so it draws no marker.
    pub fn marker(self) -> Option<Rect> {
        match self {
            Target::At(landing) | Target::Reorder(landing) | Target::Rootlist(landing) => {
                Some(landing.marker)
            }
            Target::Add(_) | Target::End => None,
        }
    }

    /// The row a target fills, if it fills one.
    ///
    /// A boundary target draws a rule instead: the two never meet.
    pub fn fill(self) -> Option<Rect> {
        match self {
            Target::Add(slot) => Some(slot.rect),
            Target::At(_) | Target::Reorder(_) | Target::Rootlist(_) | Target::End => None,
        }
    }
}

/// The row a pointer is over, and the box a fill for it would cover.
///
/// A boundary a pointer is inside no row at all, because a fill says "this row"
/// and there is no row between two. The gap between two rows belongs to the one
/// above it: a gap is a hairline and a row is the whole pitch.
pub fn slot_at(pointer: Option<Pos2>, zone: Option<&Zone>, drawn: f32) -> Option<Slot> {
    let pointer = pointer?;
    let zone = zone?;
    if !zone.hit.contains(pointer) {
        return None;
    }
    if !zone.row_h.is_finite() || zone.row_h <= 0.0 {
        return None;
    }
    let offset = (pointer.y - zone.list.top()) / zone.row_h;
    if !offset.is_finite() || offset < 0.0 {
        return None;
    }
    let row = offset.floor() as usize;
    if row >= zone.count {
        return None;
    }
    let top = zone.list.top() + row as f32 * zone.row_h;
    let rect = Rect::from_min_size(pos2(zone.list.left(), top), vec2(zone.list.width(), drawn))
        .intersect(zone.hit);
    Some(Slot {
        row: zone.first + row,
        rect,
    })
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
mod tests;
