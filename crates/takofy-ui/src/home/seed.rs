//! The one draw that opens a shelf, and the way a draw asks for the next index.
//!
//! Pure: the generator is [`crate::shuffle::splitmix`], threaded through one
//! state, so a held seed repeats its shelf and a new open draws another.

use crate::shuffle::splitmix;

/// The generator state a shelf's order is drawn from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Seed(u64);

impl Seed {
    /// The seed an open of Home draws, from the clock and the open count.
    ///
    /// The open count is part of it so two opens in the same millisecond still
    /// differ, and the clock is part of it so a later session does not repeat.
    pub fn for_open(now_ms: u64, opens: u64) -> Self {
        let mut state = now_ms ^ opens.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        Self(splitmix(&mut state))
    }

    /// The generator state the first draw starts from.
    pub fn state(self) -> u64 {
        self.0
    }
}

/// A draw from `0..n`, advancing `state`.
///
/// Zero draws zero: a shelf with nothing in its pool never asks for an index,
/// and the answer is total anyway.
pub fn below(state: &mut u64, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    usize::try_from(splitmix(state) % n as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests;
