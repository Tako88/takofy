//! The genre an album or artist read votes for.
//!
//! The service's table carries a bucket named `All` beside real genres, so the
//! vote drops it with `Pop` and `Jazz` before counting. The denominator is the
//! full record count, so a pile of dropped records keeps the winner silent
//! rather than handing it a majority.

use super::wire::GenreTable;

/// The table's non-genres and the two too broad to name a row.
pub(crate) const EXCLUDED: [&str; 3] = ["All", "Pop", "Jazz"];

/// The genre most records vote for, when it holds at least half of them.
pub(crate) fn vote(records: &[i64], names: &GenreTable) -> Option<String> {
    let entries = names.entries();
    if records.is_empty() || entries.is_empty() {
        return None;
    }
    let mut counts = vec![0usize; entries.len()];
    for id in records {
        if let Some(index) = names.index_of(*id)
            && let Some(name) = names.name_at(index)
            && !is_excluded(name)
        {
            counts[index] += 1;
        }
    }
    let mut winner: Option<usize> = None;
    for (index, count) in counts.iter().enumerate() {
        if *count == 0 {
            continue;
        }
        if winner.is_none_or(|best| *count > counts[best]) {
            winner = Some(index);
        }
    }
    let winner = winner?;
    if counts[winner] * 2 < records.len() {
        return None;
    }
    names.name_at(winner).map(str::to_owned)
}

fn is_excluded(name: &str) -> bool {
    EXCLUDED
        .iter()
        .any(|excluded| name.eq_ignore_ascii_case(excluded))
}

#[cfg(test)]
mod tests;
