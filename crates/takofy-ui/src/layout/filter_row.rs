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
mod tests {
    use super::*;
    use crate::theme::{extent, space};

    const CHIP_H: f32 = 19.0;

    fn sizes(widths: &[f32]) -> Vec<Vec2> {
        widths.iter().map(|width| vec2(*width, CHIP_H)).collect()
    }

    fn line(mut rects: Vec<Rect>) -> Vec<Rect> {
        rects.sort_by(|a, b| {
            a.top()
                .partial_cmp(&b.top())
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(
                    a.left()
                        .partial_cmp(&b.left())
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
        });
        rects
    }

    #[test]
    fn the_cluster_is_flush_right_on_the_first_line() {
        let placed = beside(&sizes(&[40.0, 50.0]), 1000.0, CHIP_H, None);
        assert_eq!(placed.cluster.field.right(), 1000.0);
        assert_eq!(placed.cluster.field.width(), cluster::FIELD_W);
        assert_eq!(placed.cluster.field.height(), cluster::FIELD_H);
        assert_eq!(placed.height, cluster::FIELD_H);
    }

    #[test]
    fn chips_centre_on_the_first_line_when_the_cluster_is_taller() {
        let placed = beside(&sizes(&[40.0]), 1000.0, CHIP_H, None);
        let chip = placed.chips[0];
        assert!(
            (chip.center().y - placed.cluster.field.center().y).abs() < 0.01,
            "the chip sits on the cluster's centre line: {chip:?} against {:?}",
            placed.cluster.field
        );
    }

    #[test]
    fn a_shorter_cluster_centres_on_the_chip_line() {
        let tall_chip = 40.0;
        let placed = beside(&[vec2(40.0, tall_chip)], 1000.0, tall_chip, None);
        let chip = placed.chips[0];
        assert!(
            (chip.center().y - placed.cluster.field.center().y).abs() < 0.01,
            "a short cluster sits on the chip's centre line: {chip:?} against {:?}",
            placed.cluster.field
        );
        assert_eq!(placed.height, tall_chip, "the taller chip sets the pitch");
    }

    #[test]
    fn chips_wrap_in_the_space_the_cluster_leaves() {
        // The field leaves 300 - 240 - S1 = 56 for the first line.
        let placed = beside(&sizes(&[30.0, 30.0]), 300.0, CHIP_H, None);
        assert_eq!(placed.chips[0].left(), 0.0);
        assert_eq!(placed.chips[0].top(), (cluster::FIELD_H - CHIP_H) / 2.0);
        assert!(
            placed.chips[1].top() > placed.chips[0].top(),
            "the second chip wraps rather than running under the field: {:?}",
            placed.chips
        );
    }

    #[test]
    fn the_boundary_chip_stays_and_one_point_more_wraps() {
        // 56 points of first line: 40 + S1 + 12 exactly fits.
        let exact = beside(&sizes(&[40.0, 12.0]), 300.0, CHIP_H, None);
        assert_eq!(
            exact.chips[1].top(),
            exact.chips[0].top(),
            "a chip that ends exactly at the edge stays on the line"
        );

        let over = beside(&sizes(&[40.0, 13.0]), 300.0, CHIP_H, None);
        assert!(
            over.chips[1].top() > over.chips[0].top(),
            "one point past the edge wraps: {:?}",
            over.chips
        );
    }

    #[test]
    fn later_lines_use_the_chip_pitch_and_the_first_the_cluster_pitch() {
        // Three chips, each too wide for the room beside the field.
        let placed = beside(&sizes(&[200.0, 200.0, 200.0]), 400.0, CHIP_H, None);
        let rects = line(placed.chips);
        let tops: Vec<f32> = rects.iter().map(Rect::top).collect();
        assert_eq!(tops.len(), 3, "one chip per line: {tops:?}");
        assert_eq!(tops[0], (cluster::FIELD_H - CHIP_H) / 2.0);
        assert_eq!(tops[1], cluster::FIELD_H + space::S1);
        assert_eq!(
            tops[2] - tops[1],
            CHIP_H + space::S1,
            "the lines under the first use the chip pitch"
        );
        assert_eq!(placed.height, cluster::FIELD_H + 2.0 * (CHIP_H + space::S1));
    }

    #[test]
    fn a_cluster_wider_than_the_row_still_orders_the_chips() {
        let placed = beside(&sizes(&[20.0, 30.0, 40.0]), 100.0, CHIP_H, None);
        assert_eq!(placed.chips.len(), 3, "no chip is dropped");
        for pair in placed.chips.windows(2) {
            assert!(
                pair[1].top() > pair[0].top() || pair[1].left() >= pair[0].left(),
                "the chips keep their order: {:?}",
                placed.chips
            );
        }
    }

    #[test]
    fn no_part_overlaps_between_the_window_minimum_and_2000() {
        let widths = [36.0, 60.0, 40.0, 60.0, 90.0];
        for width in (640..=2000).step_by(40).map(|width| width as f32) {
            let placed = beside(&sizes(&widths), width, CHIP_H, Some(90.0));
            let parts: Vec<Rect> = placed
                .cluster
                .count
                .into_iter()
                .chain(std::iter::once(placed.cluster.field))
                .collect();
            for chip in &placed.chips {
                for part in &parts {
                    assert!(
                        !chip.intersects(*part),
                        "a chip overlaps the cluster at {width}: {chip:?} against {part:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_field_is_240_by_28_flush_right_and_centred() {
        let placed = cluster(900.0, Some(90.0));
        assert_eq!(placed.field.width(), cluster::FIELD_W);
        assert_eq!(placed.field.height(), extent::BUTTON);
        assert_eq!(placed.field.right(), 900.0);
        assert_eq!(placed.field.center().y, extent::BUTTON / 2.0);
    }

    #[test]
    fn the_count_sits_one_cluster_gap_left_of_the_field() {
        let placed = cluster(900.0, Some(90.0));
        let count = placed.count.expect("a count was given");
        assert_eq!(count.right(), placed.field.left() - cluster::GAP);
        assert_eq!(count.center().y, placed.field.center().y);
    }

    #[test]
    fn the_field_does_not_move_when_the_count_arrives() {
        let without = cluster(900.0, None);
        let with = cluster(900.0, Some(90.0));
        assert_eq!(without.field, with.field, "the count sits beside the field");
    }

    #[test]
    fn the_count_clears_a_first_line_that_ends_a_gap_short() {
        let chips = [Rect::from_min_size(pos2(0.0, 0.0), vec2(50.0, CHIP_H))];
        assert!(
            count_clears_chips(50.0 + space::S1, &chips),
            "a count a chip gap past the line's edge clears it"
        );
        assert!(
            !count_clears_chips(50.0 + space::S1 - 0.5, &chips),
            "a count half a point closer reaches the chip"
        );
    }

    #[test]
    fn only_the_first_line_holds_the_count_back() {
        let first = Rect::from_min_size(pos2(0.0, 0.0), vec2(20.0, CHIP_H));
        let second = Rect::from_min_size(pos2(0.0, CHIP_H + space::S1), vec2(90.0, CHIP_H));
        assert!(
            count_clears_chips(20.0 + space::S1, &[first, second]),
            "a wide chip on a later line does not push the count away"
        );
    }

    #[test]
    fn the_count_yields_when_it_would_reach_the_chips() {
        // The field leaves 400 - 240 - S1 = 156, and a 90-point count starts at
        // 54. A chip ending at 50 leaves exactly S1 clear, so the count stays;
        // one point more and the count gives way.
        let exact = beside(&sizes(&[50.0]), 400.0, CHIP_H, Some(90.0));
        assert!(
            exact.cluster.count.is_some(),
            "a count with exactly one chip gap to spare stays"
        );

        let over = beside(&sizes(&[51.0]), 400.0, CHIP_H, Some(90.0));
        assert!(
            over.cluster.count.is_none(),
            "a count that would reach the chips yields"
        );
    }

    #[test]
    fn the_cluster_size_adds_the_gap_and_count_only_with_a_count() {
        assert_eq!(size(None), vec2(cluster::FIELD_W, cluster::FIELD_H));
        assert_eq!(
            size(Some(90.0)),
            vec2(cluster::FIELD_W + cluster::GAP + 90.0, cluster::FIELD_H)
        );
    }
}
