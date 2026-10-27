use egui::Pos2;

use super::super::*;
use super::test_support::*;
use crate::landing::resolve;

#[test]
fn the_zone_names_the_first_upcoming_row_and_the_count() {
    let ctx = context();
    let mut art = Art::default();
    let queue = queue_of(6, 1);
    let now = Instant::now();
    let (reply, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());

    let zone = reply.zone.expect("a non-empty queue is a target");
    assert_eq!(
        zone.first, 2,
        "the first upcoming row is the target's first"
    );
    assert_eq!(zone.count, 4, "only the upcoming rows are counted");
    assert_eq!(zone.row_h, ROW_H);
    let first = painted_at(&output, "t2").expect("row t2 is painted");
    assert!(
        zone.hit.contains(first),
        "the target must hold the row it names: {:?}",
        zone.hit
    );
}

#[test]
fn the_pad_below_the_last_row_appends_to_the_end() {
    let ctx = context();
    let mut art = Art::default();
    let queue = queue_of(3, 0);
    let now = Instant::now();
    let (reply, _) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
    let zone = reply.zone.expect("a queue is a target");

    let at = Pos2::new(zone.list.center().x, zone.list.bottom() + space::S1);
    assert!(
        at.y < PANE.bottom() && at.y >= zone.list.bottom(),
        "the pointer must sit in the pad below the list: {at:?}"
    );
    let landing = resolve(Some(at), Some(&zone)).expect("the pad is part of the target");
    assert_eq!(
        landing.boundary,
        zone.first + zone.count,
        "a drop in the pad appends after the last row"
    );
}

#[test]
fn an_empty_queue_is_a_target_that_takes_its_first_row() {
    let ctx = context();
    let mut art = Art::default();
    let now = Instant::now();
    let (reply, _) = pass(&ctx, PANE, &Queue::default(), &mut art, now, Vec::new());
    let zone = reply.zone.expect("an armed queue takes rows");

    assert_eq!((zone.first, zone.count), (0, 0), "there is no row yet");
    let at = Pos2::new(zone.list.center().x, PANE.center().y);
    let landing = resolve(Some(at), Some(&zone)).expect("the column is a target");
    assert_eq!(landing.boundary, 0, "the first row is the only place");
}

#[test]
fn the_empty_column_below_a_short_list_appends() {
    let ctx = context();
    let mut art = Art::default();
    let queue = queue_of(2, 0);
    let now = Instant::now();
    let (reply, _) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
    let zone = reply.zone.expect("a queue is a target");

    let at = Pos2::new(zone.list.center().x, PANE.bottom() - space::S1);
    assert!(
        at.y > zone.list.bottom() + space::S4,
        "the pointer must sit well below the list and its pad: {at:?}"
    );
    let landing = resolve(Some(at), Some(&zone)).expect("the empty column is the target");
    assert_eq!(
        landing.boundary,
        zone.first + zone.count,
        "a drop in the empty column appends after the last row"
    );
}
