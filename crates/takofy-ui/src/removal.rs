//! Which rows a pick takes out of a list, and the order it leaves behind.

use takofy_model::Removal;

/// The removals a pick makes, and the rows they leave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan<T> {
    /// The wire ops, highest `from` first, so each reads against the list the
    /// removal before it left.
    pub removals: Vec<Removal>,
    /// The rows after the removals, which is the replay of `removals`.
    pub kept: Vec<T>,
}

/// The removals that take `rows` out of `order`, and the order they leave.
///
/// One contiguous run is one removal. A gapped pick is one removal per run,
/// highest first. Any index the list does not hold is no removal at all.
pub fn plan<T: Clone>(order: &[T], rows: &[usize]) -> Option<Plan<T>> {
    let mut rows = rows.to_vec();
    rows.sort_unstable();
    rows.dedup();
    if rows.is_empty() || rows.iter().any(|row| *row >= order.len()) {
        return None;
    }

    let mut removals: Vec<Removal> = Vec::new();
    for row in &rows {
        match removals.last_mut() {
            Some(run) if run.from + run.length == *row => run.length += 1,
            _ => removals.push(Removal {
                from: *row,
                length: 1,
            }),
        }
    }
    removals.reverse();

    let mut kept = order.to_vec();
    for removal in &removals {
        apply_removal(&mut kept, *removal);
    }
    Some(Plan { removals, kept })
}

/// Drops `length` rows at `from`, as the server reads one `REM`.
///
/// A removal that cannot index the list changes nothing.
pub fn apply_removal<T>(list: &mut Vec<T>, removal: Removal) {
    let Some(end) = removal.from.checked_add(removal.length) else {
        return;
    };
    if removal.length == 0 || end > list.len() {
        return;
    }
    list.drain(removal.from..end);
}

#[cfg(test)]
mod tests {
    use super::{Removal, apply_removal, plan};

    fn rows(len: usize) -> Vec<usize> {
        (0..len).collect()
    }

    fn subsets(len: usize) -> impl Iterator<Item = Vec<usize>> {
        (1..1u32 << len).map(move |mask| (0..len).filter(|row| mask & (1 << row) != 0).collect())
    }

    #[test]
    fn a_contiguous_pick_is_one_removal_with_its_length() {
        let found = plan(&rows(8), &[2, 3, 4]).expect("a pick of held rows");
        assert_eq!(found.removals, vec![Removal { from: 2, length: 3 }]);
        assert_eq!(found.kept, vec![0, 1, 5, 6, 7]);
    }

    #[test]
    fn a_gapped_pick_is_descending_runs() {
        let found = plan(&rows(7), &[1, 3, 5]).expect("a pick of held rows");
        assert_eq!(
            found.removals,
            vec![
                Removal { from: 5, length: 1 },
                Removal { from: 3, length: 1 },
                Removal { from: 1, length: 1 },
            ]
        );
        assert_eq!(found.kept, vec![0, 2, 4, 6]);
    }

    #[test]
    fn a_pick_given_out_of_order_or_twice_is_sorted_and_deduped() {
        let found = plan(&rows(6), &[4, 2, 2, 3]).expect("a pick of held rows");
        assert_eq!(found.removals, vec![Removal { from: 2, length: 3 }]);
        assert_eq!(found.kept, vec![0, 1, 5]);
    }

    #[test]
    fn an_empty_pick_or_a_row_past_the_end_is_none() {
        assert_eq!(plan(&rows(4), &[]), None);
        assert_eq!(plan(&rows(4), &[4]), None);
        assert_eq!(plan(&rows(4), &[1, 9]), None);
        assert_eq!(plan::<usize>(&[], &[0]), None);
    }

    #[test]
    fn removing_every_row_is_allowed_and_empties_the_list() {
        let found = plan(&rows(4), &[0, 1, 2, 3]).expect("the whole list");
        assert_eq!(found.removals, vec![Removal { from: 0, length: 4 }]);
        assert!(found.kept.is_empty());
    }

    #[test]
    fn kept_is_the_replay_of_the_removals_for_every_pick_set() {
        for len in 1..=6 {
            let order = rows(len);
            for pick in subsets(len) {
                let found = plan(&order, &pick).expect("a pick of held rows");
                let expected: Vec<usize> = order
                    .iter()
                    .copied()
                    .filter(|row| !pick.contains(row))
                    .collect();
                assert_eq!(found.kept, expected, "len {len}, pick {pick:?}");

                let mut replay = order.clone();
                for removal in &found.removals {
                    apply_removal(&mut replay, *removal);
                }
                assert_eq!(replay, found.kept, "len {len}, pick {pick:?}");
            }
        }
    }

    #[test]
    fn every_removal_is_in_range_when_applied_in_order() {
        for len in 1..=6 {
            let order = rows(len);
            for pick in subsets(len) {
                let found = plan(&order, &pick).expect("a pick of held rows");
                let mut remaining = len;
                for pair in found.removals.windows(2) {
                    assert!(pair[0].from > pair[1].from, "len {len}, pick {pick:?}");
                }
                for removal in &found.removals {
                    assert!(removal.length > 0, "len {len}, pick {pick:?}");
                    assert!(
                        removal.from + removal.length <= remaining,
                        "len {len}, pick {pick:?}: {removal:?} past {remaining} rows"
                    );
                    remaining -= removal.length;
                }
                assert_eq!(remaining, len - pick.len(), "len {len}, pick {pick:?}");
            }
        }
    }
}
