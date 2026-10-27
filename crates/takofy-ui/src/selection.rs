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

    /// The rows a command opened on `row` acts on.
    ///
    /// A row inside the pick acts on the whole pick and leaves it alone: the
    /// menu must never collapse what the user gathered. A row outside it is
    /// picked first, so a menu about one row can never stand over another row
    /// that looks picked.
    pub fn act_on(&mut self, row: usize) -> Vec<usize> {
        if !self.contains(row) {
            self.press(row, Press::Replace);
        }
        self.rows().collect()
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

    /// Drops every row and the anchor that `keep` rejects.
    ///
    /// A filtered list uses this where `keep` would need a range: the visible
    /// rows are not contiguous in the list's own index space.
    pub fn retain(&mut self, keep: impl Fn(usize) -> bool) {
        self.rows.retain(|row| keep(*row));
        if !self.anchor.is_some_and(&keep) {
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

    /// Drops `rows` and moves every row below them down, the inverse of
    /// [`Selection::open_gap`].
    pub fn without(&mut self, rows: &[usize]) {
        let dropped: BTreeSet<usize> = rows.iter().copied().collect();
        if dropped.is_empty() {
            return;
        }
        let shift = |row: usize| row - dropped.range(..row).count();
        self.rows = self
            .rows
            .iter()
            .filter(|row| !dropped.contains(row))
            .map(|row| shift(*row))
            .collect();
        self.anchor = self
            .anchor
            .filter(|anchor| !dropped.contains(anchor))
            .map(shift);
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
mod tests;
