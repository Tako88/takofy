//! What Home can say under the greeting, from what its three sources are doing
//! and how much of the shelf can be drawn.
//!
//! Pure: no clock and no I/O, so every state is reachable in a test. The page
//! always says one of six things, and never sits on the greeting alone.

use crate::message::Failure;

/// What one source of the shelf is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    /// Nothing is known yet: the read has not run or has not answered.
    #[default]
    Pending,
    /// The read answered, so the source has a state of its own.
    Arrived,
    /// The read will not answer without a reload.
    Failed(Failure),
}

/// The three sources the shelf draws from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Intake {
    /// The rootlist, which is also what gates the other two reads.
    pub playlists: Phase,
    /// The saved albums.
    pub albums: Phase,
    /// The liked songs.
    pub likes: Phase,
}

/// How much the shelf has to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tally {
    /// Any playlist, album or liked song exists, dated or not.
    pub any: bool,
    /// Pooled liked songs whose label has not been answered.
    pub unanswered: usize,
    /// Pooled entries that can draw a card.
    pub usable: usize,
}

/// The one thing Home says under the greeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outlook {
    /// One read is still coming, or a label is; the shelf waits whole.
    Waiting,
    /// The rootlist failed, so there is no library to build a shelf from.
    Unreadable(Failure),
    /// The shelf can draw.
    Cards,
    /// Nothing is drawable, and a source that would have drawn failed.
    Incomplete(Failure),
    /// Nothing is saved at all.
    DayOne,
    /// Things are saved, but none of them is recent enough to draw.
    Quiet,
}

/// What the page says, in the order that keeps it from ever going blank.
///
/// A failed rootlist is the one state that outranks everything: the other two
/// reads never run without it, so there is nothing else to say. A pending source
/// or an unanswered label comes next, because drawing a partial shelf would
/// reshuffle it when the rest lands. A failure below that only speaks when the
/// pool has nothing usable, so one failed source cannot hide the cards another
/// still supplies.
pub fn outlook(intake: Intake, tally: Tally) -> Outlook {
    if let Phase::Failed(failure) = intake.playlists {
        return Outlook::Unreadable(failure);
    }
    let pending = [intake.playlists, intake.albums, intake.likes]
        .into_iter()
        .any(|phase| phase == Phase::Pending);
    if pending || tally.unanswered > 0 {
        return Outlook::Waiting;
    }
    if tally.usable > 0 {
        return Outlook::Cards;
    }
    if let Phase::Failed(failure) = intake.albums {
        return Outlook::Incomplete(failure);
    }
    if let Phase::Failed(failure) = intake.likes {
        return Outlook::Incomplete(failure);
    }
    if !tally.any {
        return Outlook::DayOne;
    }
    Outlook::Quiet
}

#[cfg(test)]
mod tests;
