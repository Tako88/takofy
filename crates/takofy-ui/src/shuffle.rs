//! A drawn order for the rows that are coming, and the way back to the list's
//! own order.
//!
//! Pure: a small seeded generator, a Fisher–Yates permutation and the restore
//! that maps a remembered order back onto the rows that are here now. Nothing
//! here touches the queue; the queue asks for a plan and applies it.

use std::collections::{HashMap, VecDeque};

/// Advances `state` and returns one value.
///
/// splitmix64, the same shape as the published algorithm: a caller that wants a
/// different value per draw threads one `state` through.
pub fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A permutation of `0..len`, drawn from `seed`.
///
/// Never the identity from two rows: a draw that would repeat the order the
/// rows already have is rotated by one, so turning shuffle on always changes
/// what plays next.
pub fn order(len: usize, seed: u64) -> Vec<usize> {
    let mut order: Vec<usize> = (0..len).collect();
    let mut state = seed;
    for row in (1..len).rev() {
        let draw = splitmix(&mut state) % (row as u64 + 1);
        order.swap(row, draw as usize);
    }
    if len > 1 && order.iter().enumerate().all(|(row, old)| row == *old) {
        order.rotate_left(1);
    }
    order
}

/// Where each row of `now` goes to rebuild `snapshot`'s order.
///
/// The result is a permutation of `now`'s indices: each snapshot uri takes the
/// earliest row holding it that no earlier uri has taken, and rows the snapshot
/// never saw keep their current order after them. A row that has since gone is
/// simply missing.
pub fn restore(snapshot: &[String], now: &[String]) -> Vec<usize> {
    // A row index per uri, earliest first, so each lookup is one hash instead
    // of a rescan of every row for every snapshot entry.
    let mut rows: HashMap<&str, VecDeque<usize>> = HashMap::with_capacity(now.len());
    for (row, uri) in now.iter().enumerate() {
        rows.entry(uri.as_str()).or_default().push_back(row);
    }

    let mut order = Vec::with_capacity(now.len());
    for uri in snapshot {
        if let Some(row) = rows.get_mut(uri.as_str()).and_then(VecDeque::pop_front) {
            order.push(row);
        }
    }

    let mut unseen: Vec<usize> = rows.into_values().flatten().collect();
    unseen.sort_unstable();
    order.extend(unseen);
    order
}

/// What the button is called in this state.
pub fn label(on: bool) -> &'static str {
    if on { "Shuffle on" } else { "Shuffle off" }
}

#[cfg(test)]
mod tests;
