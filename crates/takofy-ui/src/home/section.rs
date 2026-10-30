//! One shelf's state, what the page reads off it, and the reads one open allows.
//!
//! Pure: [`settle`] is a table over the phases of the reads a shelf draws from,
//! so two shelves fed the same phases always say the same thing, and the page's
//! own rules can be written against [`Presence`] alone.

use crate::message::Failure;

use super::outlook::Phase;

/// One of Home's five fixed shelves, in the order the page draws them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    /// The cards of what was saved most recently.
    Dated,
    /// The stations built from the library's own seeds.
    Stations,
    /// The artists related to one followed artist.
    Because,
    /// The artists the library holds the most of.
    Counted,
    /// The newest episodes of the followed shows.
    Episodes,
}

impl Slot {
    /// The page's fixed order: the dated shelf first, the four built ones under
    /// it.
    pub const ORDER: [Self; 5] = [
        Self::Dated,
        Self::Stations,
        Self::Because,
        Self::Counted,
        Self::Episodes,
    ];
}

/// What one shelf is doing, as the page's own rules read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// A read the shelf draws from has not answered.
    Waiting,
    /// A read the shelf draws from failed and may be retried.
    Failed,
    /// The shelf has nothing to draw from.
    Absent,
    /// The shelf has something to draw.
    Ready,
}

/// One shelf, from the phases of the reads it draws from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Section<T> {
    /// A read the shelf draws from has not answered.
    #[default]
    Waiting,
    /// A read the shelf draws from failed and may be retried.
    Failed(Failure),
    /// The shelf has nothing to draw from.
    Absent,
    /// What the shelf draws.
    Ready(T),
}

impl<T> Section<T> {
    /// What the page reads off this shelf.
    pub fn presence(&self) -> Presence {
        match self {
            Self::Waiting => Presence::Waiting,
            Self::Failed(_) => Presence::Failed,
            Self::Absent => Presence::Absent,
            Self::Ready(_) => Presence::Ready,
        }
    }
}

/// `own`, unless the read it waits on failed.
///
/// A failed rootlist leaves the reads behind it unasked, so they are failed too:
/// the page must say what happened rather than wait for answers no read will
/// ever ask for.
pub fn inherit(prerequisite: Phase, own: Phase) -> Phase {
    match prerequisite {
        Phase::Failed(failure) => Phase::Failed(failure),
        Phase::Pending | Phase::Arrived => own,
    }
}

/// One shelf's state from `prerequisite`, the phases in `feeds`, and `held`.
pub fn settle<T>(prerequisite: Phase, feeds: &[Phase], held: Option<T>) -> Section<T> {
    if let Phase::Failed(failure) = prerequisite {
        return Section::Failed(failure);
    }
    if prerequisite == Phase::Pending || feeds.contains(&Phase::Pending) {
        return Section::Waiting;
    }
    if let Some(held) = held {
        return Section::Ready(held);
    }
    if let Some(Phase::Failed(failure)) =
        feeds.iter().find(|phase| matches!(phase, Phase::Failed(_)))
    {
        return Section::Failed(*failure);
    }
    Section::Absent
}

/// How many reads one open of Home has left, per shelf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    counted: usize,
    because: usize,
    episodes: usize,
    stations: usize,
}

impl Budget {
    /// How many reads one open allows `slot`.
    pub const fn per_open(slot: Slot) -> usize {
        match slot {
            Slot::Dated => 0,
            // Four stations, then the one read that lengths their rows.
            Slot::Stations => 5,
            Slot::Because => 4,
            Slot::Counted => 1,
            Slot::Episodes => 8,
        }
    }

    /// How many reads one open allows every shelf together.
    pub const fn allowance() -> usize {
        Self::per_open(Slot::Dated)
            + Self::per_open(Slot::Stations)
            + Self::per_open(Slot::Because)
            + Self::per_open(Slot::Counted)
            + Self::per_open(Slot::Episodes)
    }

    /// The reads `slot` has left.
    pub fn left(&self, slot: Slot) -> usize {
        match slot {
            Slot::Dated => 0,
            Slot::Stations => self.stations,
            Slot::Because => self.because,
            Slot::Counted => self.counted,
            Slot::Episodes => self.episodes,
        }
    }

    /// The reads every shelf has left together.
    pub fn total(&self) -> usize {
        self.counted + self.because + self.episodes + self.stations
    }

    /// Whether `slot` may send one more read, spending it.
    pub fn take(&mut self, slot: Slot) -> bool {
        let left = match slot {
            Slot::Dated => return false,
            Slot::Stations => &mut self.stations,
            Slot::Because => &mut self.because,
            Slot::Counted => &mut self.counted,
            Slot::Episodes => &mut self.episodes,
        };
        if *left == 0 {
            return false;
        }
        *left -= 1;
        true
    }

    /// Gives `slot` its allowance back, for a retry that reads again.
    pub fn refill(&mut self, slot: Slot) {
        match slot {
            Slot::Dated => {}
            Slot::Stations => self.stations = Self::per_open(slot),
            Slot::Because => self.because = Self::per_open(slot),
            Slot::Counted => self.counted = Self::per_open(slot),
            Slot::Episodes => self.episodes = Self::per_open(slot),
        }
    }
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            counted: Self::per_open(Slot::Counted),
            because: Self::per_open(Slot::Because),
            episodes: Self::per_open(Slot::Episodes),
            stations: Self::per_open(Slot::Stations),
        }
    }
}

#[cfg(test)]
mod tests;
