//! Where a dragged row lands, and the order a move leaves behind.

use takofy_model::{Move, TrackEntry};

/// One row moving out of `from` and into the gap at `to`, landing on `landed`.
///
/// `to` is the wire's index, which names the gap in the list **before** the move:
/// a move that lands later has to name the gap after its own, or the server reads
/// it as a move onto itself and does nothing. `landed` is where the row ends up in
/// the list we hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub from: usize,
    pub to: usize,
    pub landed: usize,
}

/// The move a drop at `boundary` makes, for a list of `len` rows.
///
/// The boundary is the gap the pointer is over: 0 is above the first row, `len`
/// is below the last, and `from`'s own two gaps are where the row already is.
pub fn step(len: usize, from: usize, boundary: usize) -> Option<Step> {
    if from >= len || boundary > len || boundary == from || boundary == from + 1 {
        return None;
    }
    let landed = if boundary > from {
        boundary - 1
    } else {
        boundary
    };
    Some(Step {
        from,
        to: boundary,
        landed,
    })
}

/// Moves the row at `step.from` to `step.landed`, every other row keeping its order.
///
/// Both indices come from [`step`], which has already checked them.
pub fn apply<T>(rows: &mut Vec<T>, step: Step) {
    // The step is built by `step`, so both indices name a row of `rows`.
    debug_assert!(
        step.from < rows.len() && step.landed < rows.len(),
        "a step indexes the rows it was built for"
    );
    if step.from == step.landed {
        return;
    }
    let row = rows.remove(step.from);
    rows.insert(step.landed, row);
}

/// The write in flight: its playlist, the rows before it and the rows it wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub playlist: String,
    pub before: Vec<TrackEntry>,
    pub after: Vec<TrackEntry>,
}

/// The rows a drop carries to one boundary, and where they land there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gather {
    /// The picked rows, ascending and without repeats.
    pub rows: Vec<usize>,
    /// The index the block starts at once the picks are gathered.
    pub landed: usize,
}

/// Where `rows` land when dropped at `boundary` in a list of `len` rows.
///
/// The boundary is the gap the pointer is over. A contiguous pick dropped on or
/// beside itself, a pick of the whole list, and any index the list does not
/// hold are all no move.
pub fn gather(len: usize, rows: &[usize], boundary: usize) -> Option<Gather> {
    let mut rows = rows.to_vec();
    rows.sort_unstable();
    rows.dedup();
    if boundary > len || rows.iter().any(|row| *row >= len) {
        return None;
    }
    let (&first, &last) = (rows.first()?, rows.last()?);
    if rows.len() == last - first + 1 && (first..=last + 1).contains(&boundary) {
        return None;
    }
    let below = rows.iter().filter(|row| **row < boundary).count();
    Some(Gather {
        rows,
        landed: boundary - below,
    })
}

/// Applies one wire move to `list`: lift `length` rows out of `from` and put
/// them before whatever the list held at `to` before the move.
///
/// A move that cannot index the list, or that names a gap inside the run it
/// lifts, changes nothing.
pub fn apply_move<T>(list: &mut Vec<T>, mv: Move) {
    let Some(end) = mv.from.checked_add(mv.length) else {
        return;
    };
    if mv.length == 0 || end > list.len() || mv.to > list.len() {
        return;
    }
    // A gap inside the run names a place the lift itself removes.
    if mv.to > mv.from && mv.to < end {
        return;
    }
    let at = if mv.to > mv.from {
        match mv.to.checked_sub(mv.length) {
            Some(at) => at,
            None => return,
        }
    } else {
        mv.to
    };
    if at > list.len() - mv.length {
        return;
    }
    let run: Vec<T> = list.splice(mv.from..end, []).collect();
    list.splice(at..at, run);
}

/// The moves a drop makes and the order it leaves behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block<T> {
    /// The wire ops, in the order the server applies them.
    pub moves: Vec<Move>,
    /// The rows after the moves, which is the replay of `moves`.
    pub order: Vec<T>,
    /// Where the block starts in `order`.
    pub landed: usize,
}

/// The moves that gather `rows` at `boundary`, and the order they leave.
///
/// A contiguous pick is one move of its own length. A gapped pick is a chain of
/// one-row moves, each index read against the list the move before it left.
pub fn block<T: Clone>(order: &[T], rows: &[usize], boundary: usize) -> Option<Block<T>> {
    let found = gather(order.len(), rows, boundary)?;
    let len = order.len();
    let (&first, &last) = (found.rows.first()?, found.rows.last()?);
    let count = found.rows.len();

    let mut moves = Vec::new();
    if last - first + 1 == count {
        moves.push(Move {
            from: first,
            length: count,
            to: boundary,
        });
    } else {
        // Rows by identity, so each move's index is read against the last move's list.
        let mut working: Vec<usize> = (0..len).collect();
        let mut gap = boundary;
        let split = found.rows.iter().filter(|row| **row < boundary).count();
        let (above, below) = found.rows.split_at(split);
        for row in above.iter().chain(below) {
            let from = working.iter().position(|held| held == row)?;
            if let Some(found) = step(len, from, gap) {
                let mv = Move {
                    from: found.from,
                    length: 1,
                    to: found.to,
                };
                moves.push(mv);
                apply_move(&mut working, mv);
            }
            // A row from above the gap lands just before it, which leaves the gap
            // where it is; a row from below it lands on the gap, which grows by one.
            if *row >= boundary {
                gap += 1;
            }
        }
        if moves.is_empty() {
            return None;
        }
    }

    let mut replay = order.to_vec();
    for mv in &moves {
        apply_move(&mut replay, *mv);
    }
    Some(Block {
        moves,
        order: replay,
        landed: found.landed,
    })
}

#[cfg(test)]
mod tests {
    use super::{Gather, Step, apply, apply_move, block, gather, step};
    use takofy_model::Move;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn labels(len: usize) -> Vec<String> {
        (0..len).map(|row| format!("r{row}")).collect()
    }

    /// Every non-empty pick set over `len` rows, each ascending.
    fn subsets(len: usize) -> Vec<Vec<usize>> {
        (1..(1usize << len) as u64)
            .map(|mask| {
                (0..len)
                    .filter(|row| mask >> row & 1 == 1)
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Where the picks land, worked out from the list rather than from the moves.
    fn formula(order: &[String], rows: &[usize], boundary: usize) -> Vec<String> {
        let below = rows.iter().filter(|row| **row < boundary).count();
        let landed = boundary - below;
        let mut rest: Vec<String> = order
            .iter()
            .enumerate()
            .filter(|(row, _)| !rows.contains(row))
            .map(|(_, value)| value.clone())
            .collect();
        let picks: Vec<String> = rows.iter().map(|row| order[*row].clone()).collect();
        let tail = rest.split_off(landed);
        rest.extend(picks);
        rest.extend(tail);
        rest
    }

    /// The brief's wire reading, written out on its own: lift the run, then put
    /// it before whatever the list held at `to` before the move.
    fn server(list: &[String], moves: &[Move]) -> Vec<String> {
        let mut list = list.to_vec();
        for mv in moves {
            let run: Vec<String> = list.splice(mv.from..mv.from + mv.length, []).collect();
            let at = if mv.to > mv.from {
                mv.to - mv.length
            } else {
                mv.to
            };
            let tail = list.split_off(at);
            list.extend(run);
            list.extend(tail);
        }
        list
    }

    #[test]
    fn apply_move_lifts_the_run_and_puts_it_before_the_old_to() {
        let mut rows = strings(&["A", "B", "C", "D"]);
        apply_move(
            &mut rows,
            Move {
                from: 0,
                length: 2,
                to: 4,
            },
        );
        assert_eq!(rows, strings(&["C", "D", "A", "B"]));

        let mut rows = strings(&["A", "B", "C", "D", "E"]);
        apply_move(
            &mut rows,
            Move {
                from: 3,
                length: 1,
                to: 2,
            },
        );
        assert_eq!(rows, strings(&["A", "B", "D", "C", "E"]));

        // `to` is the pre-move gap, so a run moved down names the slot after itself.
        let mut rows = strings(&["A", "B", "C", "D"]);
        apply_move(
            &mut rows,
            Move {
                from: 1,
                length: 2,
                to: 4,
            },
        );
        assert_eq!(rows, strings(&["A", "D", "B", "C"]));
    }

    #[test]
    fn apply_move_leaves_a_list_a_move_that_cannot_index_it_alone() {
        let cases = [
            Move {
                from: 4,
                length: 1,
                to: 0,
            },
            Move {
                from: 0,
                length: 5,
                to: 0,
            },
            Move {
                from: 0,
                length: 1,
                to: 9,
            },
            Move {
                from: 0,
                length: 0,
                to: 2,
            },
            // The gap is inside the run the move lifts, which no wire op names.
            Move {
                from: 1,
                length: 3,
                to: 2,
            },
            Move {
                from: 1,
                length: 3,
                to: 3,
            },
        ];
        for mv in cases {
            let mut rows = strings(&["A", "B", "C", "D"]);
            apply_move(&mut rows, mv);
            assert_eq!(rows, strings(&["A", "B", "C", "D"]), "{mv:?}");
        }
    }

    #[test]
    fn gather_reports_the_rows_the_boundary_and_where_they_land() {
        assert_eq!(
            gather(4, &[2, 0], 4),
            Some(Gather {
                rows: vec![0, 2],
                landed: 2,
            })
        );
        assert_eq!(
            gather(4, &[0, 0], 2),
            Some(Gather {
                rows: vec![0],
                landed: 1,
            }),
            "a row picked twice is one row"
        );
        assert_eq!(
            gather(4, &[1], 0),
            Some(Gather {
                rows: vec![1],
                landed: 0,
            })
        );
    }

    #[test]
    fn gather_refuses_no_picks() {
        assert_eq!(gather(4, &[], 0), None);
        assert_eq!(gather(4, &[], 4), None);
        assert_eq!(gather(0, &[], 0), None);
    }

    #[test]
    fn gather_refuses_a_pick_past_the_end() {
        assert_eq!(gather(4, &[4], 0), None);
        assert_eq!(gather(4, &[1, 7], 2), None);
    }

    #[test]
    fn gather_refuses_a_boundary_past_the_end() {
        assert_eq!(gather(4, &[1], 5), None);
    }

    #[test]
    fn gather_refuses_a_pick_of_every_row() {
        assert_eq!(gather(4, &[0, 1, 2, 3], 0), None);
        assert_eq!(gather(4, &[0, 1, 2, 3], 4), None);
    }

    #[test]
    fn a_gapped_pair_gathered_at_the_end_is_the_measured_chain() {
        let found = block(&strings(&["A", "B", "C", "D"]), &[0, 2], 4).expect("a block");
        assert_eq!(
            found.moves,
            vec![
                Move {
                    from: 0,
                    length: 1,
                    to: 4
                },
                Move {
                    from: 1,
                    length: 1,
                    to: 4
                },
            ]
        );
        assert_eq!(found.order, strings(&["B", "D", "A", "C"]));
        assert_eq!(found.landed, 2);
    }

    #[test]
    fn a_contiguous_block_is_one_mov_with_its_length() {
        let found = block(&strings(&["A", "B", "C", "D"]), &[0, 1], 4).expect("a block");
        assert_eq!(
            found.moves,
            vec![Move {
                from: 0,
                length: 2,
                to: 4
            }]
        );
        assert_eq!(found.order, strings(&["C", "D", "A", "B"]));
        assert_eq!(found.landed, 2);

        let found = block(&labels(6), &[1, 2, 3], 0).expect("a block");
        assert_eq!(
            found.moves,
            vec![Move {
                from: 1,
                length: 3,
                to: 0
            }]
        );
        assert_eq!(found.order, strings(&["r1", "r2", "r3", "r0", "r4", "r5"]));
        assert_eq!(found.landed, 0);
    }

    #[test]
    fn a_mixed_chain_skips_the_pick_already_beside_the_gap() {
        let found = block(&strings(&["A", "B", "C", "D", "E"]), &[1, 3], 2).expect("a block");
        assert_eq!(
            found.moves,
            vec![Move {
                from: 3,
                length: 1,
                to: 2
            }]
        );
        assert_eq!(found.order, strings(&["A", "B", "D", "C", "E"]));
        assert_eq!(found.landed, 1);
    }

    #[test]
    fn a_contiguous_block_dropped_on_or_inside_itself_is_none() {
        let order = labels(6);
        for first in 0..6 {
            for last in first..6 {
                let rows: Vec<usize> = (first..=last).collect();
                for boundary in first..=last + 1 {
                    assert_eq!(
                        block(&order, &rows, boundary),
                        None,
                        "{rows:?} at {boundary}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_gapped_block_is_never_a_noop() {
        for len in 1..=6 {
            for rows in subsets(len) {
                if rows.len() == rows[rows.len() - 1] - rows[0] + 1 {
                    continue;
                }
                for boundary in 0..=len {
                    assert!(
                        block(&labels(len), &rows, boundary).is_some(),
                        "{rows:?} at {boundary} of {len}"
                    );
                }
            }
        }
    }

    #[test]
    fn one_row_agrees_with_step() {
        for len in 0..=6 {
            let order = labels(len);
            for from in 0..len {
                for boundary in 0..=len {
                    let found = block(&order, &[from], boundary);
                    match step(len, from, boundary) {
                        None => assert_eq!(found, None, "{len} {from} {boundary}"),
                        Some(found_step) => {
                            let found = found.expect("a block where a step is");
                            assert_eq!(
                                found.moves,
                                vec![Move {
                                    from: found_step.from,
                                    length: 1,
                                    to: found_step.to
                                }]
                            );
                            assert_eq!(found.landed, found_step.landed);
                            let mut expected = order.clone();
                            apply(&mut expected, found_step);
                            assert_eq!(found.order, expected);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_order_is_the_replay_of_the_moves_for_every_pick_set() {
        for len in 1..=6 {
            let order = labels(len);
            for rows in subsets(len) {
                for boundary in 0..=len {
                    let found = block(&order, &rows, boundary);
                    assert_eq!(
                        found.is_some(),
                        gather(len, &rows, boundary).is_some(),
                        "{rows:?} at {boundary} of {len}"
                    );
                    let Some(found) = found else {
                        continue;
                    };
                    let working = format!("{rows:?} at {boundary} of {len}");
                    assert_eq!(
                        found.order,
                        formula(&order, &rows, boundary),
                        "the formula: {working}"
                    );
                    assert_eq!(
                        found.order,
                        server(&order, &found.moves),
                        "the server's reading: {working} with {:?}",
                        found.moves
                    );
                    assert_eq!(
                        found.landed,
                        boundary - rows.iter().filter(|row| **row < boundary).count(),
                        "the landing: {working}"
                    );
                    for mv in &found.moves {
                        assert!(
                            mv.to != mv.from && mv.to != mv.from + 1,
                            "a move onto itself: {mv:?} in {working}"
                        );
                    }
                }
            }
        }
    }

    fn moved(values: &[&str], from: usize, boundary: usize) -> Vec<String> {
        let mut rows = strings(values);
        let found = step(rows.len(), from, boundary).expect("a move");
        apply(&mut rows, found);
        rows
    }

    #[test]
    fn down_one_lands_after_the_next_row() {
        assert_eq!(
            step(3, 0, 2),
            Some(Step {
                from: 0,
                to: 2,
                landed: 1
            })
        );
        assert_eq!(moved(&["a", "b", "c"], 0, 2), strings(&["b", "a", "c"]));
    }

    #[test]
    fn up_to_the_top() {
        assert_eq!(
            step(4, 2, 0),
            Some(Step {
                from: 2,
                to: 0,
                landed: 0
            })
        );
        assert_eq!(
            moved(&["a", "b", "c", "d"], 2, 0),
            strings(&["c", "a", "b", "d"])
        );
    }

    #[test]
    fn to_the_very_bottom() {
        assert_eq!(
            step(3, 0, 3),
            Some(Step {
                from: 0,
                to: 3,
                landed: 2
            })
        );
        assert_eq!(moved(&["a", "b", "c"], 0, 3), strings(&["b", "c", "a"]));
    }

    #[test]
    fn a_boundary_at_the_end_is_a_move_to_the_last_index() {
        assert_eq!(
            step(5, 2, 5),
            Some(Step {
                from: 2,
                to: 5,
                landed: 4
            })
        );
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
    fn the_wire_index_is_the_boundary_the_row_lands_after() {
        // The server reads `to` as a gap in the list before the move, so a move
        // downward has to name the gap after the row it would otherwise sit above.
        let cases = [
            (
                4,
                0,
                3,
                Step {
                    from: 0,
                    to: 3,
                    landed: 2,
                },
            ),
            (
                4,
                0,
                4,
                Step {
                    from: 0,
                    to: 4,
                    landed: 3,
                },
            ),
            (
                4,
                1,
                4,
                Step {
                    from: 1,
                    to: 4,
                    landed: 3,
                },
            ),
            (
                4,
                3,
                0,
                Step {
                    from: 3,
                    to: 0,
                    landed: 0,
                },
            ),
            (
                4,
                3,
                1,
                Step {
                    from: 3,
                    to: 1,
                    landed: 1,
                },
            ),
        ];
        for (len, from, boundary, expected) in cases {
            assert_eq!(step(len, from, boundary), Some(expected));
        }
    }

    #[test]
    fn a_one_row_list_never_moves() {
        assert_eq!(step(1, 0, 0), None);
        assert_eq!(step(1, 0, 1), None);
    }
}
