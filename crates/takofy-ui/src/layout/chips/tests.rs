use super::*;

/// The sidebar's own numbers: buttons 28 tall, one `S1` between chips and
/// between rows.
const ROW_H: f32 = 28.0;
const GAP: f32 = 4.0;

fn row(widths: &[f32]) -> Vec<Vec2> {
    widths.iter().map(|width| Vec2::new(*width, 19.4)).collect()
}

fn mins(wrapped: &Wrapped) -> Vec<(f32, f32)> {
    wrapped
        .rects
        .iter()
        .map(|rect| (rect.min.x, rect.min.y))
        .collect()
}

#[test]
fn wrap_empty_has_no_height_and_no_rects() {
    let wrapped = wrap(&[], 100.0, GAP, ROW_H, GAP);
    assert!(wrapped.rects.is_empty());
    assert_eq!(wrapped.height, 0.0);
}

#[test]
fn wrap_one_row_places_items_left_to_right_with_the_gap() {
    let wrapped = wrap(&row(&[10.0, 12.0, 8.0]), 100.0, GAP, ROW_H, GAP);
    assert_eq!(mins(&wrapped), vec![(0.0, 0.0), (14.0, 0.0), (30.0, 0.0)]);
    assert_eq!(wrapped.height, ROW_H);
}

#[test]
fn wrap_breaks_when_an_item_would_overflow() {
    // The second item ends at 14 + 12 = 26, past the 20 it has.
    let wrapped = wrap(&row(&[10.0, 12.0]), 20.0, GAP, ROW_H, GAP);
    assert_eq!(mins(&wrapped), vec![(0.0, 0.0), (0.0, ROW_H + GAP)]);
}

#[test]
fn an_item_wider_than_the_width_still_gets_its_own_row_and_is_not_dropped() {
    let wrapped = wrap(&row(&[30.0, 5.0]), 10.0, GAP, ROW_H, GAP);
    assert_eq!(wrapped.rects.len(), 2, "nothing is dropped");
    assert_eq!(wrapped.rects[0].width(), 30.0, "it keeps its own size");
    assert_eq!(mins(&wrapped), vec![(0.0, 0.0), (0.0, ROW_H + GAP)]);
}

#[test]
fn the_first_item_of_a_row_never_triggers_a_break() {
    let wrapped = wrap(&row(&[30.0, 8.0]), 10.0, GAP, ROW_H, GAP);
    assert_eq!(
        mins(&wrapped),
        vec![(0.0, 0.0), (0.0, ROW_H + GAP)],
        "the wide first item stays alone on its row rather than moving again"
    );
    assert_eq!(wrapped.height, 2.0 * ROW_H + GAP);
}

#[test]
fn height_is_rows_times_row_h_plus_gaps() {
    // Three rows: 10 + 10 does not fit 15, and neither does one more.
    let wrapped = wrap(&row(&[10.0, 10.0, 10.0]), 15.0, GAP, ROW_H, GAP);
    assert_eq!(wrapped.height, 3.0 * ROW_H + 2.0 * GAP);
}

/// The sidebar's chip positions: four band widths, wrapped with the
/// sidebar's own gaps and button pitch, pin the rects the row has to keep.
/// A change to [`wrap`]'s rule that moves a chip fails here.
#[test]
fn sidebar_numbers_are_unchanged() {
    // Measured from the bundled mono face at `text::XXS`.
    let sizes = row(&[73.213, 59.994, 53.4, 66.588]);
    // Band left + S4, and the top of the chip row.
    let left = 16.0;
    let top = 193.0;

    for (band, expected) in [
        (
            254.0,
            vec![
                (16.0, 193.0),
                (93.213, 193.0),
                (157.206, 193.0),
                (16.0, 225.0),
            ],
        ),
        (
            200.0,
            vec![(16.0, 193.0), (93.213, 193.0), (16.0, 225.0), (73.4, 225.0)],
        ),
        (
            140.0,
            vec![(16.0, 193.0), (16.0, 225.0), (16.0, 257.0), (16.0, 289.0)],
        ),
        (
            100.0,
            vec![(16.0, 193.0), (16.0, 225.0), (16.0, 257.0), (16.0, 289.0)],
        ),
    ] {
        let available = band - 2.0 * 16.0;
        let wrapped = wrap(&sizes, available, GAP, ROW_H, GAP);
        let placed: Vec<(f32, f32)> = wrapped
            .rects
            .iter()
            .map(|rect| (rect.min.x + left, rect.min.y + top))
            .collect();
        assert_eq!(
            placed.len(),
            expected.len(),
            "band width {band}: one rect per chip"
        );
        for (placed, expected) in placed.iter().zip(&expected) {
            assert!(
                (placed.0 - expected.0).abs() < 0.01 && (placed.1 - expected.1).abs() < 0.01,
                "band width {band}: {placed:?} is not {expected:?}"
            );
        }
    }
}
