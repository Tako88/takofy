//! What a repeat mode does with a finished track, and how the bar names it.
//!
//! Pure: the mode, why the queue is moving, and the one table between them. The
//! queue holds the mode and applies the hop; nothing here draws or moves.

/// The queue's repeat mode, cycled in this order by the playbar's button.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Repeat {
    /// Stop after the last row.
    #[default]
    Off,
    /// Wrap to the first row.
    All,
    /// Play the current row again.
    One,
}

/// Why the queue is moving off the current row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// The track ran out.
    Ended,
    /// The track could not be loaded.
    Unavailable,
    /// The user asked for the next one.
    Manual,
}

/// Where the queue goes from the row it is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hop {
    /// The row after the current one.
    Next,
    /// The first row.
    Wrap,
    /// The current row again.
    Replay,
}

impl Repeat {
    /// The mode a pair of flags names, with the one-track loop winning because
    /// both cannot hold at once.
    pub fn from_flags(context: bool, track: bool) -> Self {
        match (track, context) {
            (true, _) => Self::One,
            (false, true) => Self::All,
            (false, false) => Self::Off,
        }
    }

    /// The two halves this mode holds, as the flags name them.
    pub fn flags(self) -> (bool, bool) {
        (self == Self::All, self == Self::One)
    }

    /// The next mode in the cycle.
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }

    /// What the button is called in this state, which names the state and not
    /// the action the next click takes.
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Repeat off",
            Self::All => "Repeat all",
            Self::One => "Repeat one",
        }
    }

    /// Whether the glyph takes the accent: two of the three states are on.
    pub fn lit(self) -> bool {
        self != Self::Off
    }

    /// The spelling a preferences file stores, which is not the display text.
    pub fn key(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::All => "all",
            Self::One => "one",
        }
    }

    /// The mode `key` names, or `None` for any other spelling.
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "off" => Some(Self::Off),
            "all" => Some(Self::All),
            "one" => Some(Self::One),
            _ => None,
        }
    }

    /// The badge the glyph carries, which is the only thing that tells `One`
    /// from `All` at a glance.
    pub fn badge(self) -> Option<&'static str> {
        (self == Self::One).then_some("1")
    }
}

/// The row a `cause` moves to, for `len` rows with the current row at `index`.
///
/// `One` governs only a natural end: an unplayable row must not be replayed, and
/// a manual next must go forward.
pub fn advance(repeat: Repeat, index: Option<usize>, len: usize, cause: Cause) -> Option<Hop> {
    let index = index.filter(|index| *index < len)?;
    if repeat == Repeat::One && cause == Cause::Ended {
        return Some(Hop::Replay);
    }
    if index + 1 < len {
        Some(Hop::Next)
    } else if repeat == Repeat::All {
        Some(Hop::Wrap)
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
