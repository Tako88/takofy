//! The discography filter: which chips an artist can show, and what they show.
//!
//! The row is built from the artist's own releases, so a chip always has
//! something under it: `All` plus one chip per kind the releases carry, in the
//! reference's order rather than the order the releases happen to arrive in.
//! `Other` gets no chip — an artist's discography effectively never carries one,
//! and those releases stay reachable under `All`.
//!
//! The choice belongs to the page, not to the frame: it is kept while the same
//! artist stays open, and another artist starts on `All`. A choice that no
//! longer has a chip falls back to `All` for the frame without being forgotten,
//! so a reload that brings its kind back restores it.

use takofy_model::{Release, ReleaseKind};

/// A chip in the discography's filter row.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Chip {
    /// Every block, whatever its kind.
    #[default]
    All,
    /// Only the blocks of one kind.
    Kind(ReleaseKind),
}

/// The kinds the row can offer a chip for, in the reference's order.
///
/// A fixed order, because a release's kind is independent of its group: the
/// discography is not sorted by kind, so the row cannot follow the releases.
pub const KINDS: [ReleaseKind; 4] = [
    ReleaseKind::Album,
    ReleaseKind::Ep,
    ReleaseKind::Single,
    ReleaseKind::Compilation,
];

/// The row an artist's releases can show: `All`, then every kind present.
pub fn chips(releases: &[Release]) -> Vec<Chip> {
    let mut chips = vec![Chip::All];
    for kind in KINDS {
        if releases.iter().any(|release| release.kind == kind) {
            chips.push(Chip::Kind(kind));
        }
    }
    chips
}

impl Chip {
    /// Whether a block of `kind` is drawn under this chip.
    pub fn matches(self, kind: ReleaseKind) -> bool {
        match self {
            Self::All => true,
            Self::Kind(chosen) => chosen == kind,
        }
    }
}

/// Which of the two axes left the discography empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emptied {
    /// The name filter emptied the list; the sentence names what was typed.
    Query,
    /// No release of the chosen kind exists here; the sentence is the format's.
    Format,
}

/// What emptied a list, if it is empty.
///
/// The query wins whenever one is typed, because "nothing matches" is the more
/// specific fact and the reference shows only one of the two sentences at a time.
/// With no query, a list that lost its rows to the format says so; a list that
/// has not lost anything says nothing.
pub fn emptied(shown: usize, queried: bool) -> Option<Emptied> {
    if shown > 0 {
        return None;
    }
    Some(if queried {
        Emptied::Query
    } else {
        Emptied::Format
    })
}

/// The true indices whose kind matches `chip` and whose name contains `needle`.
///
/// The needle is trimmed and folded here, so every caller gets the same
/// case-insensitive substring rule. A blank needle asks only about the chip.
pub fn narrow(releases: &[Release], chip: Chip, needle: &str) -> Vec<usize> {
    let needle = needle.trim().to_lowercase();
    releases
        .iter()
        .enumerate()
        .filter(|(_, release)| {
            chip.matches(release.kind)
                && (needle.is_empty() || release.name.to_lowercase().contains(&needle))
        })
        .map(|(index, _)| index)
        .collect()
}

/// The page's filter state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Filter {
    /// The artist this choice concerns; `None` before any artist page opened.
    artist: Option<String>,
    /// The chip last pressed.
    chosen: Chip,
    /// The name the field holds, as the user typed it.
    query: String,
}

impl Filter {
    /// Points the state at an artist.
    ///
    /// Opening another artist resets to `All`, because a choice made for one
    /// artist's discs says nothing about another's. Re-opening the same artist
    /// keeps the choice, so Back onto it draws what the user left.
    pub fn open(&mut self, artist: &str) {
        if self.artist.as_deref() == Some(artist) {
            return;
        }
        *self = Self {
            artist: Some(artist.to_owned()),
            chosen: Chip::All,
            query: String::new(),
        };
    }

    /// The artist this state concerns.
    pub fn artist(&self) -> Option<&str> {
        self.artist.as_deref()
    }

    /// The chip last pressed.
    pub fn chosen(&self) -> Chip {
        self.chosen
    }

    /// Presses a chip. There is no toggle: pressing the chosen chip is a no-op.
    pub fn choose(&mut self, chip: Chip) {
        self.chosen = chip;
    }

    /// The text the field edits.
    ///
    /// The field owns the query across frames, so a keystroke, a clear click and
    /// an Escape all leave this page's state rather than a one-frame copy.
    pub fn edit(&mut self) -> &mut String {
        &mut self.query
    }

    /// The needle the releases match, when there is a non-blank query.
    pub fn needle(&self) -> Option<String> {
        let trimmed = self.query.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_lowercase())
    }

    /// Drops the query, so every release the chip keeps shows again.
    pub fn clear_query(&mut self) {
        self.query.clear();
    }

    /// The chip the row draws as pressed and the blocks are filtered by.
    ///
    /// A choice with no chip in the current row falls back to `All` without
    /// overwriting the choice, so a reload that restores the kind restores it.
    pub fn effective(&self, chips: &[Chip]) -> Chip {
        if chips.contains(&self.chosen) {
            self.chosen
        } else {
            Chip::All
        }
    }
}

#[cfg(test)]
mod tests;
