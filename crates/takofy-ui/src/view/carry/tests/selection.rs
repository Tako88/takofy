use super::super::*;
use super::test_support::*;

#[test]
fn select_rows_replaces_a_lists_selection() {
    let ctx = context();
    let first = Id::unique("first");
    let second = Id::unique("second");
    let mut list = List::open(&ctx, first, 6);
    list.press(1, Press::Replace);
    list.press(4, Press::Toggle);
    list.finish(&ctx);
    let mut other = List::open(&ctx, second, 6);
    other.press(2, Press::Replace);
    other.finish(&ctx);

    select_rows(&ctx, first, 3..4);

    let list = List::open(&ctx, first, 6);
    assert!(list.look(3).selected, "the row that landed is the pick");
    assert!(
        !list.look(1).selected && !list.look(4).selected,
        "every pick before it is gone"
    );
    assert!(
        List::open(&ctx, second, 6).look(2).selected,
        "another list keeps its own selection"
    );
}

#[test]
fn look_marks_picked_and_lifted_rows() {
    let ctx = context();
    let id = Id::unique("list");
    let mut list = List::open(&ctx, id, 5);
    list.press(1, Press::Replace);
    list.finish(&ctx);

    let list = List::open(&ctx, id, 5);
    assert_eq!(
        list.look(1),
        Look {
            selected: true,
            ..Look::default()
        }
    );
    assert_eq!(list.look(2), Look::default());

    // A carry marks the rows that travel and tells the whole list it is
    // carrying.
    lift(&ctx, carry(Source::List(id), vec![1]));
    let list = List::open(&ctx, id, 5);
    assert_eq!(
        list.look(1),
        Look {
            selected: true,
            lifted: true,
            carrying: true
        }
    );
    assert_eq!(
        list.look(2),
        Look {
            carrying: true,
            ..Look::default()
        }
    );

    // Another list sees the carry but none of the rows.
    let other = List::open(&ctx, Id::unique("other"), 5);
    assert_eq!(
        other.look(1),
        Look {
            carrying: true,
            ..Look::default()
        }
    );
}

#[test]
fn a_press_is_stored_on_finish_and_seen_by_the_next_open() {
    let ctx = context();
    let id = Id::unique("list");
    let mut list = List::open(&ctx, id, 5);
    assert_eq!(list.look(3), Look::default());

    list.press(3, Press::Replace);
    list.finish(&ctx);

    assert!(List::open(&ctx, id, 5).look(3).selected);
}

#[test]
fn a_stored_selection_beyond_the_count_is_dropped() {
    let ctx = context();
    let id = Id::unique("list");
    let mut list = List::open(&ctx, id, 8);
    list.press(6, Press::Replace);
    list.press(1, Press::Toggle);
    list.finish(&ctx);

    // The short list keeps only the row it still names, and stores that.
    let list = List::open(&ctx, id, 3);
    assert!(list.look(1).selected);
    assert!(!list.look(6).selected);
    list.finish(&ctx);

    assert!(
        !List::open(&ctx, id, 8).look(6).selected,
        "the dropped row must not come back with the long list"
    );
}

#[test]
fn restrict_prunes_a_stale_pick_and_marks_it_changed() {
    let ctx = context();
    let id = Id::unique("list");
    let mut list = List::open(&ctx, id, 6);
    list.press(1, Press::Replace);
    list.press(4, Press::Toggle);
    list.finish(&ctx);

    let mut list = List::open(&ctx, id, 6);
    list.restrict(|row| row != 4);

    assert!(!list.look(4).selected, "a hidden row is dropped");
    assert!(list.look(1).selected);
    assert!(list.changed, "the drop is stored on finish");
    list.finish(&ctx);
    assert!(!List::open(&ctx, id, 6).look(4).selected);
}

#[test]
fn restrict_that_changes_nothing_leaves_changed_false() {
    let ctx = context();
    let id = Id::unique("list");
    let mut list = List::open(&ctx, id, 6);
    list.press(1, Press::Replace);
    list.finish(&ctx);

    let mut list = List::open(&ctx, id, 6);
    list.restrict(|row| row != 4);

    assert!(list.look(1).selected);
    assert!(!list.changed, "a no-op restriction stores nothing");
}

#[test]
fn lists_with_other_ids_keep_their_own_selection() {
    let ctx = context();
    let first = Id::unique("first");
    let second = Id::unique("second");
    let mut list = List::open(&ctx, first, 4);
    list.press(1, Press::Replace);
    list.finish(&ctx);
    let mut list = List::open(&ctx, second, 4);
    list.press(3, Press::Replace);
    list.finish(&ctx);

    assert!(List::open(&ctx, first, 4).look(1).selected);
    assert!(!List::open(&ctx, first, 4).look(3).selected);
    assert!(List::open(&ctx, second, 4).look(3).selected);
}

#[test]
fn follow_page_forgets_on_navigation() {
    let ctx = context();
    let page = |name: &str| Page::Playlist(name.to_owned());
    follow_page(&ctx, Some(&page("a")));

    let id = Id::unique("list");
    let mut list = List::open(&ctx, id, 4);
    list.press(2, Press::Replace);
    list.finish(&ctx);
    assert!(List::open(&ctx, id, 4).look(2).selected);

    follow_page(&ctx, Some(&page("b")));
    assert!(
        !List::open(&ctx, id, 4).look(2).selected,
        "a page change forgets every selection"
    );
}

#[test]
fn forget_rows_shifts_the_lists_selection() {
    let ctx = context();
    let id = Id::unique("list");
    let mut list = List::open(&ctx, id, 6);
    list.press(1, Press::Replace);
    list.press(3, Press::Toggle);
    list.press(5, Press::Toggle);
    list.finish(&ctx);

    forget_rows(&ctx, id, &[1, 3]);

    let list = List::open(&ctx, id, 6);
    assert!(
        list.look(3).selected,
        "the row below the pick did not shift"
    );
    assert!(!list.look(1).selected && !list.look(5).selected);
}
