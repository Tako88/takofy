use egui::vec2;

use super::super::*;
use super::test_support::*;

#[test]
fn lifting_a_page_row_selects_only_it_and_sets_the_payload_on_finish() {
    let ctx = context();
    let id = Id::unique("list");
    let mut list = List::open(&ctx, id, 5);
    list.press(2, Press::Replace);
    list.lift(4, vec2(10.0, 20.0), ghost(), |_| Some(entry("x")));
    assert!(
        !carrying(&ctx),
        "the payload is set when the list finishes, not while it draws"
    );
    list.finish(&ctx);

    let carry = carried(&ctx).expect("a payload");
    assert_eq!(carry.source, Source::List(id));
    assert_eq!(carry.rows, vec![4]);
    assert_eq!(carry.entries, vec![entry("x")]);
    assert_eq!(carry.size, vec2(10.0, 20.0));

    let list = List::open(&ctx, id, 5);
    assert!(list.look(4).selected, "the lifted row is the selection");
    assert!(!list.look(2).selected, "the rest of the selection is gone");
}

/// A page whose every row still holds an entry named for its index.
fn every_row(row: usize) -> Option<TrackEntry> {
    Some(entry(&row.to_string()))
}

#[test]
fn lifting_a_picked_row_carries_the_whole_pick() {
    let ctx = context();
    let id = Id::unique("list");
    let mut list = List::open(&ctx, id, 6);
    list.press(4, Press::Replace);
    list.press(1, Press::Toggle);
    list.press(3, Press::Toggle);
    list.finish(&ctx);

    let mut list = List::open(&ctx, id, 6);
    list.lift(4, vec2(10.0, 20.0), ghost(), every_row);
    list.finish(&ctx);

    let carry = carried(&ctx).expect("a payload");
    assert_eq!(
        carry.rows,
        vec![1, 3, 4],
        "the whole pick travels, in order"
    );
    assert_eq!(
        carry.entries,
        vec![entry("1"), entry("3"), entry("4")],
        "each picked row brings its own entry"
    );
    let list = List::open(&ctx, id, 6);
    assert!(
        list.look(1).selected && list.look(3).selected && list.look(4).selected,
        "the pick is not collapsed by the lift"
    );
}

#[test]
fn lifting_an_unpicked_row_carries_only_it() {
    let ctx = context();
    let id = Id::unique("list");
    let mut list = List::open(&ctx, id, 6);
    list.press(1, Press::Replace);
    list.press(3, Press::Toggle);
    list.finish(&ctx);

    let mut list = List::open(&ctx, id, 6);
    list.lift(5, vec2(10.0, 20.0), ghost(), every_row);
    list.finish(&ctx);

    let carry = carried(&ctx).expect("a payload");
    assert_eq!(carry.rows, vec![5]);
    assert_eq!(carry.entries, vec![entry("5")]);
    let list = List::open(&ctx, id, 6);
    assert!(
        !list.look(1).selected && !list.look(3).selected && list.look(5).selected,
        "a press on an unpicked row replaces the pick"
    );
}

#[test]
fn a_lift_whose_entry_is_missing_carries_nothing() {
    let ctx = context();
    let id = Id::unique("list");
    let mut list = List::open(&ctx, id, 6);
    list.press(1, Press::Replace);
    list.press(4, Press::Toggle);
    list.finish(&ctx);

    let mut list = List::open(&ctx, id, 6);
    list.lift(4, vec2(10.0, 20.0), ghost(), |row| {
        (row == 1).then(|| entry("1"))
    });
    list.finish(&ctx);

    assert!(!carrying(&ctx), "a block with a missing row is no carry");
    let list = List::open(&ctx, id, 6);
    assert!(
        list.look(1).selected && list.look(4).selected,
        "the pick is unchanged"
    );
}

#[test]
fn a_pick_beyond_the_count_is_not_carried() {
    let ctx = context();
    let id = Id::unique("list");
    let mut list = List::open(&ctx, id, 6);
    list.press(2, Press::Replace);
    list.press(5, Press::Toggle);
    list.finish(&ctx);

    let mut list = List::open(&ctx, id, 6);
    list.lift(2, vec2(10.0, 20.0), ghost(), |row| {
        (row < 3).then(|| entry(&row.to_string()))
    });
    list.finish(&ctx);

    assert!(
        !carrying(&ctx),
        "a picked row the list can no longer name is refused"
    );
    let list = List::open(&ctx, id, 6);
    assert!(list.look(2).selected && list.look(5).selected);
}
