//! The dated pool a shelf draws from, and the partial draw over it.
//!
//! Pure: the pool is the newest dated entries, and the draw is a forward
//! partial Fisher–Yates whose chosen positions are read back in date order. A
//! wider shelf therefore takes one more card off the end, never a different
//! set.

use takofy_model::SavedAt;

use super::seed::{Seed, below};

/// How many of the newest dated entries make up the pool.
///
/// It is a fixed count rather than the number of cards on screen, so a resize
/// never changes which cards were drawn.
pub const POOL: usize = 24;

/// The newest dated entries, as indices into `dates`, at most [`POOL`] of them.
///
/// A tie keeps the input's order, so the same dates always pool the same way.
pub fn pool(dates: &[Option<SavedAt>]) -> Vec<usize> {
    let mut dated: Vec<(usize, SavedAt)> = dates
        .iter()
        .enumerate()
        .filter_map(|(index, at)| Some((index, (*at)?)))
        .collect();
    dated.sort_by(|(left_index, left), (right_index, right)| {
        right.cmp(left).then(left_index.cmp(right_index))
    });
    dated
        .into_iter()
        .take(POOL)
        .map(|(index, _)| index)
        .collect()
}

/// `cards` indices drawn from `pool`, in date order.
///
/// `pool` is newest first, so drawing over positions and then reading the
/// chosen positions in ascending order yields the newest first. `dates` is the
/// slice the pool was built from; a pool index that is undated or out of range
/// there is dropped rather than drawn.
pub fn pick(dates: &[Option<SavedAt>], pool: &[usize], seed: Seed, cards: usize) -> Vec<usize> {
    let k = cards.min(pool.len());
    let mut positions: Vec<usize> = (0..pool.len()).collect();
    let mut state = seed.state();
    for i in 0..k {
        let j = i + below(&mut state, pool.len() - i);
        positions.swap(i, j);
    }
    positions.truncate(k);
    positions.sort_unstable();
    positions
        .into_iter()
        .filter_map(|pos| pool.get(pos).copied())
        .filter(|index| matches!(dates.get(*index), Some(Some(_))))
        .collect()
}

#[cfg(test)]
mod tests;
