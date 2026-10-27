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
mod tests;
