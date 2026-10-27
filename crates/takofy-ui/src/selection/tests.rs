use super::*;

fn rows(selection: &Selection) -> Vec<usize> {
    selection.rows().collect()
}

fn picked(rows: impl IntoIterator<Item = usize>) -> Selection {
    let mut selection = Selection::default();
    selection.set(rows);
    selection
}

#[test]
fn a_plain_press_selects_exactly_that_row() {
    let mut selection = Selection::default();
    selection.press(3, Press::Replace);
    assert_eq!(rows(&selection), vec![3]);
}

#[test]
fn a_plain_press_replaces_a_larger_selection() {
    let mut selection = picked([1, 2, 5]);
    selection.press(4, Press::Replace);
    assert_eq!(rows(&selection), vec![4]);
}

#[test]
fn shift_replaces_the_selection_with_the_span_from_the_anchor() {
    let mut selection = Selection::default();
    selection.press(2, Press::Replace);
    selection.press(5, Press::Extend);
    assert_eq!(rows(&selection), vec![2, 3, 4, 5]);
}

#[test]
fn shift_spans_backwards_too() {
    let mut selection = Selection::default();
    selection.press(4, Press::Replace);
    selection.press(1, Press::Extend);
    assert_eq!(rows(&selection), vec![1, 2, 3, 4]);
}

#[test]
fn a_second_shift_press_re_spans_from_the_same_anchor() {
    let mut selection = Selection::default();
    selection.press(2, Press::Replace);
    selection.press(5, Press::Extend);
    selection.press(3, Press::Extend);
    assert_eq!(rows(&selection), vec![2, 3]);
}

#[test]
fn shift_with_no_anchor_behaves_as_a_plain_press() {
    let mut selection = Selection::default();
    selection.press(6, Press::Extend);
    assert_eq!(rows(&selection), vec![6]);
}

#[test]
fn ctrl_toggles_one_row_and_keeps_the_rest() {
    let mut selection = picked([1, 2]);
    selection.press(4, Press::Toggle);
    assert_eq!(rows(&selection), vec![1, 2, 4]);
}

#[test]
fn ctrl_on_a_picked_row_drops_it() {
    let mut selection = picked([1, 2, 4]);
    selection.press(2, Press::Toggle);
    assert_eq!(rows(&selection), vec![1, 4]);
}

#[test]
fn ctrl_moves_the_anchor_to_the_toggled_row() {
    let mut selection = Selection::default();
    selection.press(1, Press::Replace);
    selection.press(7, Press::Toggle);
    selection.press(5, Press::Extend);
    assert_eq!(rows(&selection), vec![5, 6, 7]);
}

#[test]
fn lifting_a_picked_row_keeps_the_whole_selection() {
    let mut selection = picked([2, 3, 6]);
    selection.lift(3);
    assert_eq!(rows(&selection), vec![2, 3, 6]);
}

#[test]
fn lifting_an_unpicked_row_selects_only_it() {
    let mut selection = picked([2, 3, 6]);
    selection.lift(1);
    assert_eq!(rows(&selection), vec![1]);
}

#[test]
fn keep_drops_rows_and_an_anchor_outside_the_range() {
    let mut selection = picked([1, 3, 5]);
    selection.keep(2..4);
    assert_eq!(rows(&selection), vec![3]);
    assert_eq!(selection.anchor, None);
}

#[test]
fn retain_drops_hidden_rows_and_a_hidden_anchor() {
    let mut selection = Selection::default();
    selection.press(2, Press::Replace);
    selection.press(5, Press::Toggle);
    selection.press(9, Press::Toggle);

    selection.retain(|row| row != 5 && row != 9);

    assert_eq!(rows(&selection), vec![2]);
    assert_eq!(selection.anchor, None, "the anchor fell with its row");
}

#[test]
fn retain_keeps_a_visible_anchor() {
    let mut selection = Selection::default();
    selection.press(2, Press::Replace);
    selection.press(5, Press::Toggle);
    selection.press(9, Press::Toggle);

    selection.retain(|row| row != 5);

    assert_eq!(rows(&selection), vec![2, 9]);
    assert_eq!(selection.anchor, Some(9));
}

#[test]
fn an_extend_after_a_dropped_anchor_replaces() {
    let mut selection = Selection::default();
    selection.press(2, Press::Replace);
    selection.press(5, Press::Toggle);
    selection.press(9, Press::Toggle);
    selection.retain(|row| row != 5 && row != 9);
    assert_eq!(selection.anchor, None);

    selection.press(4, Press::Extend);

    assert_eq!(rows(&selection), vec![4], "a lost anchor falls to Replace");
}

#[test]
fn toggling_the_anchor_row_off_keeps_it_where_a_span_starts() {
    let mut selection = picked([2, 5]);
    selection.press(2, Press::Toggle);
    selection.press(4, Press::Extend);
    assert_eq!(rows(&selection), vec![2, 3, 4]);
}

#[test]
fn without_drops_the_rows_and_shifts_the_later_ones_down() {
    let mut selection = picked([1, 3, 4, 6]);
    selection.without(&[3, 4]);
    assert_eq!(rows(&selection), vec![1, 4]);
    assert_eq!(selection.anchor, Some(1));
}

#[test]
fn without_moves_the_anchor_with_its_row() {
    let mut selection = picked([2, 4]);
    selection.press(4, Press::Replace);
    selection.without(&[2]);
    assert_eq!(rows(&selection), vec![3]);
    assert_eq!(selection.anchor, Some(3));
}

#[test]
fn without_the_anchor_row_clears_the_anchor() {
    let mut selection = picked([2, 5]);
    selection.without(&[2]);
    assert_eq!(rows(&selection), vec![4]);
    assert_eq!(selection.anchor, None);
}

#[test]
fn without_every_row_leaves_it_empty() {
    let mut selection = picked([0, 1]);
    selection.without(&[0, 1]);
    assert!(selection.is_empty());
    assert_eq!(selection.anchor, None);
}

#[test]
fn without_is_the_inverse_of_open_gap() {
    let mut selection = picked([1, 4, 8]);
    selection.press(6, Press::Toggle);
    let before = selection.clone();
    selection.open_gap(3, 2);
    selection.without(&[3, 4]);
    assert_eq!(selection, before);
}

#[test]
fn a_zero_gap_changes_nothing_and_an_empty_keep_clears() {
    let mut selection = picked([2, 4]);
    selection.open_gap(1, 0);
    assert_eq!(rows(&selection), vec![2, 4]);
    assert_eq!(selection.anchor, Some(2));
    selection.keep(0..0);
    assert!(selection.is_empty());
    assert_eq!(selection.anchor, None);
}

#[test]
fn open_gap_moves_rows_at_and_after_the_gap() {
    let mut selection = picked([1, 4]);
    selection.press(5, Press::Toggle);
    selection.open_gap(3, 2);
    assert_eq!(rows(&selection), vec![1, 6, 7]);
    assert_eq!(selection.anchor, Some(7));
}

#[test]
fn set_anchors_at_the_lowest_row() {
    let mut selection = Selection::default();
    selection.set([5, 2, 9]);
    assert_eq!(selection.anchor, Some(2));
}

#[test]
fn rows_come_out_ascending() {
    let mut selection = Selection::default();
    for row in [7, 1, 4] {
        selection.press(row, Press::Toggle);
    }
    assert_eq!(rows(&selection), vec![1, 4, 7]);
}

#[test]
fn act_on_a_row_inside_the_pick_returns_the_whole_pick_and_keeps_it() {
    let mut selection = picked([2, 4, 5]);
    assert_eq!(selection.act_on(4), vec![2, 4, 5]);
    assert_eq!(rows(&selection), vec![2, 4, 5], "the pick was collapsed");
    assert_eq!(selection.anchor, Some(2));
}

#[test]
fn act_on_a_row_outside_replaces_the_pick_with_that_row() {
    let mut selection = picked([2, 4]);
    assert_eq!(selection.act_on(1), vec![1]);
    assert_eq!(rows(&selection), vec![1]);
    assert_eq!(selection.anchor, Some(1));
}

#[test]
fn act_on_with_no_pick_picks_the_row() {
    let mut selection = Selection::default();
    assert_eq!(selection.act_on(3), vec![3]);
    assert_eq!(rows(&selection), vec![3]);
}

#[test]
fn act_on_returns_rows_ascending() {
    let mut selection = Selection::default();
    for row in [7, 1, 4] {
        selection.press(row, Press::Toggle);
    }
    assert_eq!(selection.act_on(7), vec![1, 4, 7]);
}

#[test]
fn a_bare_press_replaces() {
    assert_eq!(Press::from_modifiers(Modifiers::default()), Press::Replace);
}

#[test]
fn shift_asks_for_a_span() {
    let modifiers = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    assert_eq!(Press::from_modifiers(modifiers), Press::Extend);
}

#[test]
fn the_command_key_asks_for_a_toggle() {
    let modifiers = Modifiers {
        command: true,
        ..Modifiers::default()
    };
    assert_eq!(Press::from_modifiers(modifiers), Press::Toggle);
}

#[test]
fn shift_wins_over_command() {
    let modifiers = Modifiers {
        shift: true,
        command: true,
        ..Modifiers::default()
    };
    assert_eq!(Press::from_modifiers(modifiers), Press::Extend);
}

#[test]
fn alt_alone_replaces() {
    let modifiers = Modifiers {
        alt: true,
        ..Modifiers::default()
    };
    assert_eq!(Press::from_modifiers(modifiers), Press::Replace);
}

fn page(name: &str) -> Option<Page> {
    Some(Page::Playlist(name.to_owned()))
}

#[test]
fn forget_everything_when_the_page_changes() {
    let mut picks = Picks::default();
    picks.follow(page("a").as_ref());
    *picks.get_mut(Id::unique("first")) = picked([1]);
    picks.follow(page("b").as_ref());
    assert!(picks.get_mut(Id::unique("first")).is_empty());
}

#[test]
fn keep_a_selection_while_the_page_stays() {
    let mut picks = Picks::default();
    picks.follow(page("a").as_ref());
    *picks.get_mut(Id::unique("first")) = picked([1, 2]);
    picks.follow(page("a").as_ref());
    assert_eq!(rows(picks.get_mut(Id::unique("first"))), vec![1, 2]);
}

#[test]
fn keep_lists_apart() {
    let mut picks = Picks::default();
    picks.follow(page("a").as_ref());
    *picks.get_mut(Id::unique("first")) = picked([1]);
    *picks.get_mut(Id::unique("second")) = picked([4]);
    assert_eq!(rows(picks.get_mut(Id::unique("first"))), vec![1]);
    assert_eq!(rows(picks.get_mut(Id::unique("second"))), vec![4]);
}
