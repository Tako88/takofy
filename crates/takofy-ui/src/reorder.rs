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

/// The write in flight: its playlist, the rows before it, the rows it wrote,
/// and the query it took with them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub playlist: String,
    pub before: Vec<TrackEntry>,
    pub after: Vec<TrackEntry>,
    /// The page's query, when taking the last row emptied the page and took the
    /// query too. A failed write puts it back beside the rows.
    pub query: Option<String>,
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
mod tests;
