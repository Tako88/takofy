//! The pool a shelf draws from, and the draws over it.
//!
//! Pure: a pool is the newest dated entries, and a draw is a forward partial
//! Fisher–Yates whose chosen positions are read back in date order. A wider
//! shelf therefore takes one more entry off the end, never a different set.

use takofy_model::SavedAt;

use super::seed::{Seed, below};

/// How many of the newest dated entries the dated shelf pools.
///
/// It is a fixed count rather than the number of cards on screen, so a resize
/// never changes which cards were drawn.
const POOL: usize = 24;

/// The newest `want` dated entries, as indices into `dates`.
///
/// A tie keeps the input's order, so the same dates always pool the same way,
/// and an undated entry never pools.
pub fn newest(dates: &[Option<SavedAt>], want: usize) -> Vec<usize> {
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
        .take(want)
        .map(|(index, _)| index)
        .collect()
}

/// The dated shelf's pool: the newest [`POOL`] entries.
pub fn pool(dates: &[Option<SavedAt>]) -> Vec<usize> {
    newest(dates, POOL)
}

/// `cards` positions drawn from `pool`, ascending, without replacement.
///
/// `pool` is newest first, so drawing over positions and then reading the
/// chosen positions in ascending order yields the newest first.
pub fn draw(pool: &[usize], seed: Seed, cards: usize) -> Vec<usize> {
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
}

/// `want` of `items` positions, drawn without replacement and ascending.
///
/// This is what a shelf with no dates of its own draws with: a flat pool of
/// positions, from which a shelf takes as many as it has room for.
pub fn sample(items: usize, seed: Seed, want: usize) -> Vec<usize> {
    let pool: Vec<usize> = (0..items).collect();
    draw(&pool, seed, want)
}

/// `cards` indices of `dates` drawn from `pool`, in date order.
///
/// `dates` is the slice the pool was built from; a pool index that is undated or
/// out of range there is dropped rather than drawn.
pub fn pick(dates: &[Option<SavedAt>], pool: &[usize], seed: Seed, cards: usize) -> Vec<usize> {
    draw(pool, seed, cards)
        .into_iter()
        .filter_map(|position| pool.get(position).copied())
        .filter(|index| matches!(dates.get(*index), Some(Some(_))))
        .collect()
}

#[cfg(test)]
mod tests;
