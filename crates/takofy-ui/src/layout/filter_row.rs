//! The filter row's trailing cluster: the count and the name field, flush right.
//!
//! The chips own the reading order at the left; the cluster is one group at the
//! right. The field never shrinks, so a row that runs out of room wraps its
//! chips around the field instead of squeezing it. The count is the first thing
//! to give: it is shown only while it has room left of the field, and it never
//! moves the field when it arrives or leaves.
//!
//! Everything here is pure geometry: the view paints the rects this module
//! places, and the page can reserve the row's slot before the field is drawn.

use egui::{Rect, Vec2, pos2, vec2};

use crate::theme::{cluster, space};

/// The field and, when it has room, the count left of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Cluster {
    /// The name field, flush with the row's trailing edge.
    pub field: Rect,
    /// The count, one cluster gap left of the field, when it fits.
    pub count: Option<Rect>,
}

/// A row's chips and its trailing cluster, relative to the row's top-left.
#[derive(Clone, Debug, PartialEq)]
pub struct Beside {
    /// One rect per chip, in the order the caller gave them.
    pub chips: Vec<Rect>,
    /// The cluster at the row's trailing edge.
    pub cluster: Cluster,
    /// How tall the whole row is: the first line's pitch and the chip lines
    /// under it.
    pub height: f32,
}

/// The size the cluster asks the row to reserve.
///
/// The count is part of the reservation only while it is drawn: a row reserves
/// the field's column whether or not a query is on, and the count joins it one
/// gap to the left.
pub fn size(count_w: Option<f32>) -> Vec2 {
    let count = count_w.map_or(0.0, |width| cluster::GAP + width);
    vec2(cluster::FIELD_W + count, cluster::FIELD_H)
}

/// Places the field flush right, and the count one gap left of it.
///
/// The rects are relative to the row's top-left; the row is a single line of
/// `cluster::FIELD_H`, so both are centred on it.
pub fn cluster(width: f32, count_w: Option<f32>) -> Cluster {
    let field = Rect::from_min_size(
        pos2((width - cluster::FIELD_W).max(0.0), 0.0),
        vec2(cluster::FIELD_W, cluster::FIELD_H),
    );
    let count = count_w.map(|count_w| {
        Rect::from_min_size(
            pos2(field.left() - cluster::GAP - count_w, 0.0),
            vec2(count_w, cluster::FIELD_H),
        )
    });
    Cluster { field, count }
}

/// Lays the chips in the space left of the cluster.
///
/// A chip row wraps in the space the field leaves. When the count has room
/// there too, the row reserves it as well, so the chips do not run under it.
/// A count that would reach the chips gives way: the field is the tool here, and
/// the count is the first thing to go.
pub fn beside(sizes: &[Vec2], width: f32, chip_h: f32, count_w: Option<f32>) -> Beside {
    let first_h = chip_h.max(cluster::FIELD_H);
    // The field is the floor: a count never makes the chips overlap the field.
    let field_only = width - cluster::FIELD_W - space::S1;
    let chips = place(sizes, field_only.max(0.0), width.max(0.0), chip_h, first_h);

    // The count has room exactly when the field-only first line already ends a
    // chip gap before it. The chips were placed in the field's own width, so a
    // count that clears that line cannot cover any chip: the first line is the
    // whole of the comparison, and the later lines already wrap in the row's
    // full width. There is no second wrap to do.
    let (chips, cluster) = match count_w {
        Some(count_w) => {
            let candidate = cluster(width, Some(count_w));
            let fits = candidate
                .count
                .is_some_and(|count| count_clears_chips(count.left(), &chips));
            if fits {
                (chips, candidate)
            } else {
                (chips, cluster(width, None))
            }
        }
        None => (chips, cluster(width, None)),
    };

    let dy = (first_h - cluster::FIELD_H) / 2.0;
    let cluster = Cluster {
        field: cluster.field.translate(vec2(0.0, dy)),
        count: cluster.count.map(|count| count.translate(vec2(0.0, dy))),
    };
    let height = first_h + wrapped_rows(&chips).saturating_sub(1) as f32 * (chip_h + space::S1);
    Beside {
        chips,
        cluster,
        height,
    }
}

/// Wraps chips into the first line's width and then the row's full width.
///
/// The first line is its own pitch: it holds the cluster, which is taller than a
/// chip. The lines under it use the chip pitch, because the cluster is only on
/// the first line.
fn place(
    sizes: &[Vec2],
    first_width: f32,
    later_width: f32,
    chip_h: f32,
    first_h: f32,
) -> Vec<Rect> {
    let mut rects = Vec::with_capacity(sizes.len());
    let mut x = 0.0;
    let mut row = 0usize;
    let mut first_in_row = true;

    for size in sizes {
        let available = if row == 0 { first_width } else { later_width };
        if !first_in_row && x + size.x > available {
            x = 0.0;
            row += 1;
        }
        let top = if row == 0 {
            (first_h - chip_h) / 2.0
        } else {
            first_h + space::S1 + (row - 1) as f32 * (chip_h + space::S1)
        };
        rects.push(Rect::from_min_size(pos2(x, top), *size));
        x += size.x + space::S1;
        first_in_row = false;
    }
    rects
}

/// How many chip lines there are, counted from where their tops sit.
fn wrapped_rows(chips: &[Rect]) -> usize {
    let mut tops: Vec<f32> = chips.iter().map(Rect::top).collect();
    tops.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    tops.dedup_by(|a, b| (*a - *b).abs() < 0.01);
    tops.len()
}

/// The right edge of the last chip on the first line.
fn first_line_right<'a>(chips: impl IntoIterator<Item = &'a Rect>) -> f32 {
    let mut chips = chips.into_iter();
    let Some(first) = chips.next() else {
        return 0.0;
    };
    let top = first.top();
    chips
        .filter(|rect| (rect.top() - top).abs() < 0.01)
        .map(Rect::right)
        .fold(first.right(), f32::max)
}

/// Whether a count whose left edge is at `count_left` clears the chips' first
/// line, or would sit under one of them.
///
/// A count clears the line when it starts one chip gap after the line's last
/// chip; every chip on it ends further left, and the chips on later lines
/// already wrap in the row's full width.
pub(crate) fn count_clears_chips<'a>(
    count_left: f32,
    chips: impl IntoIterator<Item = &'a Rect>,
) -> bool {
    count_left >= first_line_right(chips) + space::S1
}

#[cfg(test)]
mod tests;
