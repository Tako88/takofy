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
