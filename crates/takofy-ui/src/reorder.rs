//! Where a dragged row lands, and the order a move leaves behind.

use takofy_model::TrackEntry;

/// One row moving from `from` to `to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub from: usize,
    pub to: usize,
}

/// The move a drop at `boundary` makes, for a list of `len` rows.
///
/// The boundary is the gap the pointer is over: 0 is above the first row, `len`
/// is below the last, and `from`'s own two gaps are where the row already is.
pub fn step(len: usize, from: usize, boundary: usize) -> Option<Step> {
    if from >= len || boundary > len || boundary == from || boundary == from + 1 {
        return None;
    }
    let to = if boundary > from {
        boundary - 1
    } else {
        boundary
    };
    Some(Step { from, to })
}

/// Moves the row at `step.from` to `step.to`, every other row keeping its order.
///
/// Both indices come from [`step`], which has already checked them.
pub fn apply<T>(rows: &mut Vec<T>, step: Step) {
    // The step is built by `step`, so both indices name a row of `rows`.
    debug_assert!(
        step.from < rows.len() && step.to < rows.len(),
        "a step indexes the rows it was built for"
    );
    if step.from == step.to {
        return;
    }
    let row = rows.remove(step.from);
    rows.insert(step.to, row);
}

/// The write in flight: its playlist, the rows before it and the rows it wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub playlist: String,
    pub before: Vec<TrackEntry>,
    pub after: Vec<TrackEntry>,
}

#[cfg(test)]
mod tests {
    use super::{Step, apply, step};

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn moved(values: &[&str], from: usize, boundary: usize) -> Vec<String> {
        let mut rows = strings(values);
        let found = step(rows.len(), from, boundary).expect("a move");
        apply(&mut rows, found);
        rows
    }

    #[test]
    fn down_one_lands_after_the_next_row() {
        assert_eq!(step(3, 0, 2), Some(Step { from: 0, to: 1 }));
        assert_eq!(moved(&["a", "b", "c"], 0, 2), strings(&["b", "a", "c"]));
    }

    #[test]
    fn up_to_the_top() {
        assert_eq!(step(4, 2, 0), Some(Step { from: 2, to: 0 }));
        assert_eq!(
            moved(&["a", "b", "c", "d"], 2, 0),
            strings(&["c", "a", "b", "d"])
        );
    }

    #[test]
    fn to_the_very_bottom() {
        assert_eq!(step(3, 0, 3), Some(Step { from: 0, to: 2 }));
        assert_eq!(moved(&["a", "b", "c"], 0, 3), strings(&["b", "c", "a"]));
    }

    #[test]
    fn a_boundary_at_the_end_is_a_move_to_the_last_index() {
        assert_eq!(step(5, 2, 5), Some(Step { from: 2, to: 4 }));
        assert_eq!(
            step(5, 4, 5),
            None,
            "the last row's own bottom gap is where it already is"
        );
    }

    #[test]
    fn a_boundary_past_the_end_is_no_move() {
        assert_eq!(step(3, 0, 4), None);
        assert_eq!(step(0, 0, 1), None);
    }

    #[test]
    fn beside_itself_is_no_move() {
        assert_eq!(step(4, 1, 1), None, "the gap above the row");
        assert_eq!(step(4, 1, 2), None, "the gap below it");
        assert_eq!(step(1, 0, 1), None);
    }

    #[test]
    fn a_row_or_boundary_the_list_does_not_hold_is_no_move() {
        assert_eq!(step(3, 3, 0), None);
        assert_eq!(step(3, 9, 1), None);
        assert_eq!(step(3, 0, 7), None);
        assert_eq!(step(0, 0, 0), None);
    }

    #[test]
    fn a_one_row_list_never_moves() {
        assert_eq!(step(1, 0, 0), None);
        assert_eq!(step(1, 0, 1), None);
    }
}
