//! What is picked in a list of rows, and what a click on one means.
//!
//! A list keeps one `Selection`; the rows are indices into the list that owns
//! it, so a selection is only ever read against the same list it was made on.
//! The gesture vocabulary — replace, extend, toggle — is here rather than in the
//! row, because a row only knows which modifiers were held.

use std::collections::{BTreeSet, HashMap};
use std::ops::Range;

use egui::{Id, Modifiers};

use crate::route::Page;

/// What a press on a row does to the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    /// Pick this row alone.
    Replace,
    /// Add every row from the anchor to this one.
    Extend,
    /// Add this row if it was not picked, drop it if it was.
    Toggle,
}

impl Press {
    /// The press the held modifiers ask for. Shift outranks the command key,
    /// because both together can only mean a span.
    pub fn from_modifiers(modifiers: Modifiers) -> Self {
        if modifiers.shift {
            Press::Extend
        } else if modifiers.command {
            Press::Toggle
        } else {
            Press::Replace
        }
    }
}

/// The rows picked in one list, and the row a shift-click extends from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    anchor: Option<usize>,
    rows: BTreeSet<usize>,
}

impl Selection {
    /// Applies a press to `row`.
    pub fn press(&mut self, row: usize, press: Press) {
        match press {
            Press::Replace => {
                self.rows.clear();
                self.rows.insert(row);
                self.anchor = Some(row);
            }
            Press::Extend => match self.anchor {
                Some(anchor) => {
                    let (low, high) = if anchor <= row {
                        (anchor, row)
                    } else {
                        (row, anchor)
                    };
                    self.rows = (low..=high).collect();
                    self.anchor = Some(anchor);
                }
                None => self.press(row, Press::Replace),
            },
            Press::Toggle => {
                if !self.rows.remove(&row) {
                    self.rows.insert(row);
                }
                self.anchor = Some(row);
            }
        }
    }

    /// Picks `row` for dragging, without losing a selection it already sits in.
    pub fn lift(&mut self, row: usize) {
        if !self.contains(row) {
            self.press(row, Press::Replace);
        }
    }

    pub fn contains(&self, row: usize) -> bool {
        self.rows.contains(&row)
    }

    /// The picked rows, lowest first.
    pub fn rows(&self) -> impl Iterator<Item = usize> + '_ {
        self.rows.iter().copied()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn clear(&mut self) {
        self.rows.clear();
        self.anchor = None;
    }

    /// Drops every row and the anchor that falls outside `keep`.
    pub fn keep(&mut self, keep: Range<usize>) {
        self.rows.retain(|row| keep.contains(row));
        if !self.anchor.is_some_and(|anchor| keep.contains(&anchor)) {
            self.anchor = None;
        }
    }

    /// Replaces the selection with exactly these rows, anchored at the lowest.
    pub fn set(&mut self, rows: impl IntoIterator<Item = usize>) {
        self.rows = rows.into_iter().collect();
        self.anchor = self.rows.first().copied();
    }

    /// Moves every row at or after `at` down by `by`, so a gap opens there for
    /// rows inserted above them.
    pub fn open_gap(&mut self, at: usize, by: usize) {
        self.rows = self
            .rows
            .iter()
            .map(|row| if *row >= at { row + by } else { *row })
            .collect();
        if let Some(anchor) = self.anchor
            && anchor >= at
        {
            self.anchor = Some(anchor + by);
        }
    }
}

/// Every list's selection on the page in view, kept in the frame's temp memory.
///
/// The page is the boundary: a list's rows are indices into one page's data, so
/// a page change forgets every selection rather than carrying a stale index
/// somewhere else. Lists on one page are told apart by their own id.
#[derive(Clone, Default)]
pub struct Picks {
    page: Option<Page>,
    lists: HashMap<Id, Selection>,
}

impl Picks {
    /// Forgets every selection when the page changes.
    pub fn follow(&mut self, page: Option<&Page>) {
        if self.page.as_ref() != page {
            self.lists.clear();
            self.page = page.cloned();
        }
    }

    /// A list's selection, made empty the first time the list asks.
    pub fn get_mut(&mut self, id: Id) -> &mut Selection {
        self.lists.entry(id).or_default()
    }
}

#[cfg(test)]
mod tests {
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
    fn toggling_the_anchor_row_off_keeps_it_where_a_span_starts() {
        let mut selection = picked([2, 5]);
        selection.press(2, Press::Toggle);
        selection.press(4, Press::Extend);
        assert_eq!(rows(&selection), vec![2, 3, 4]);
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
}
