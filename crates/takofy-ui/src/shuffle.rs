//! A drawn order for the rows that are coming, and the way back to the list's
//! own order.
//!
//! Pure: a small seeded generator, a Fisher–Yates permutation and the restore
//! that maps a remembered order back onto the rows that are here now. Nothing
//! here touches the queue; the queue asks for a plan and applies it.

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
    let mut taken = vec![false; now.len()];
    let mut order = Vec::with_capacity(now.len());
    for uri in snapshot {
        for (row, here) in now.iter().enumerate() {
            if !taken[row] && here == uri {
                taken[row] = true;
                order.push(row);
                break;
            }
        }
    }
    for (row, used) in taken.iter().enumerate() {
        if !used {
            order.push(row);
        }
    }
    order
}

/// What the button is called in this state.
pub fn label(on: bool) -> &'static str {
    if on { "Shuffle on" } else { "Shuffle off" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uris(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn numbered(len: usize) -> Vec<String> {
        (0..len).map(|row| format!("t{row}")).collect()
    }

    /// The list `order` produces from `rows`, which is what the queue rebuilds.
    fn drawn<T: Clone>(rows: &[T], order: &[usize]) -> Vec<T> {
        order.iter().map(|row| rows[*row].clone()).collect()
    }

    fn sorted(order: &[usize]) -> Vec<usize> {
        let mut sorted = order.to_vec();
        sorted.sort_unstable();
        sorted
    }

    fn subsets(len: usize) -> Vec<Vec<usize>> {
        (0..(1usize << len) as u64)
            .map(|mask| {
                (0..len)
                    .filter(|row| mask >> row & 1 == 1)
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    #[test]
    fn splitmix_is_deterministic_and_advances() {
        // The purpose is determinism and the algorithm's published shape, not a
        // memorised constant: these are the values this implementation computes.
        let mut state = 0;
        let drawn: [u64; 3] = [
            splitmix(&mut state),
            splitmix(&mut state),
            splitmix(&mut state),
        ];
        assert_eq!(
            drawn,
            [
                0xe220_a839_7b1d_cdaf,
                0x6e78_9e6a_a1b9_65f4,
                0x06c4_5d18_8009_454f,
            ]
        );
        assert_eq!(state, 0xdaa6_6d2c_7ddf_743f, "every draw moves the state");

        let mut again = 0;
        let repeated: [u64; 3] = [
            splitmix(&mut again),
            splitmix(&mut again),
            splitmix(&mut again),
        ];
        assert_eq!(drawn, repeated, "the same state must draw the same values");
    }

    #[test]
    fn order_is_a_permutation() {
        for len in 0..=9 {
            for seed in 0..200 {
                assert_eq!(sorted(&order(len, seed)), (0..len).collect::<Vec<_>>());
            }
        }
    }

    #[test]
    fn order_is_never_the_identity_from_two_rows() {
        for len in 2..=9 {
            for seed in 0..200 {
                let drawn = order(len, seed);
                assert!(
                    drawn.iter().enumerate().any(|(row, old)| row != *old),
                    "len {len} seed {seed} drew the order it found"
                );
            }
        }
    }

    #[test]
    fn order_is_deterministic_per_seed_and_varies_across_seeds() {
        for len in 0..=9 {
            for seed in 0..50 {
                assert_eq!(order(len, seed), order(len, seed), "len {len} seed {seed}");
            }
        }
        let orders: Vec<Vec<usize>> = (0..200).map(|seed| order(4, seed)).collect();
        assert!(
            orders.iter().any(|drawn| *drawn != orders[0]),
            "every seed drew the same order"
        );
    }

    #[test]
    fn order_reaches_all_five_non_identity_arrangements_of_three() {
        let mut seen: Vec<Vec<usize>> = Vec::new();
        for seed in 0..200 {
            let drawn = order(3, seed);
            if !seen.contains(&drawn) {
                seen.push(drawn);
            }
        }
        assert_eq!(
            seen.len(),
            5,
            "the generator reached {} of the five non-identity orders",
            seen.len()
        );
        assert!(!seen.contains(&vec![0, 1, 2]), "the identity was drawn");
    }

    #[test]
    fn restore_undoes_an_unedited_shuffle_for_every_seed() {
        for len in 0..=8 {
            let snapshot = numbered(len);
            for seed in 0..50 {
                let shuffled = drawn(&snapshot, &order(len, seed));
                let back = restore(&snapshot, &shuffled);
                assert_eq!(drawn(&shuffled, &back), snapshot, "len {len} seed {seed}");
            }
        }
    }

    #[test]
    fn restore_skips_removed_rows() {
        let snapshot = uris(&["a", "b", "c", "d"]);
        let now = uris(&["d", "a", "c"]);

        assert_eq!(restore(&snapshot, &now), vec![1, 2, 0]);
    }

    #[test]
    fn restore_puts_unseen_rows_last_in_their_current_order() {
        let snapshot = uris(&["a", "b"]);
        let now = uris(&["x", "b", "a", "y"]);

        assert_eq!(restore(&snapshot, &now), vec![2, 1, 0, 3]);
    }

    #[test]
    fn restore_with_duplicate_uris_keeps_the_multiset() {
        let snapshot = uris(&["a", "a", "b"]);
        let now = uris(&["b", "a", "a"]);

        let back = restore(&snapshot, &now);
        assert_eq!(back, vec![1, 2, 0]);
        assert_eq!(drawn(&now, &back), snapshot);
    }

    #[test]
    fn restore_with_an_empty_snapshot_is_the_identity() {
        for len in 0..=5 {
            assert_eq!(restore(&[], &numbered(len)), (0..len).collect::<Vec<_>>());
        }
    }

    #[test]
    fn restore_is_a_permutation_for_every_removal_subset_and_append() {
        for len in 0..=5 {
            let snapshot = numbered(len);
            for removed in subsets(len) {
                let kept: Vec<String> = snapshot
                    .iter()
                    .enumerate()
                    .filter(|(row, _)| !removed.contains(row))
                    .map(|(_, uri)| uri.clone())
                    .collect();
                let appended = uris(&["x", "y"]);
                let mut now = kept.clone();
                now.extend(appended.iter().cloned());

                for seed in 0..10 {
                    let shuffled = drawn(&now, &order(now.len(), seed));
                    let back = restore(&snapshot, &shuffled);
                    assert_eq!(
                        sorted(&back),
                        (0..shuffled.len()).collect::<Vec<_>>(),
                        "len {len} removed {removed:?} seed {seed}"
                    );
                    // The surviving rows come back in the list's order; the rows
                    // the snapshot never saw keep the order they have now.
                    let rebuilt = drawn(&shuffled, &back);
                    assert_eq!(rebuilt[..kept.len()], kept[..], "len {len} seed {seed}");
                    let unseen: Vec<String> = shuffled
                        .iter()
                        .filter(|uri| !kept.contains(uri))
                        .cloned()
                        .collect();
                    assert_eq!(
                        rebuilt[kept.len()..],
                        unseen[..],
                        "len {len} removed {removed:?} seed {seed}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_labels_read_off_and_on() {
        assert_eq!(label(false), "Shuffle off");
        assert_eq!(label(true), "Shuffle on");
    }
}
