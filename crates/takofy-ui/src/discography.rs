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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    /// Every block, whatever its kind.
    All,
    /// Only the blocks of one kind.
    Kind(ReleaseKind),
}

impl Default for Chip {
    fn default() -> Self {
        Self::All
    }
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

/// The page's filter state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Filter {
    /// The artist this choice concerns; `None` before any artist page opened.
    artist: Option<String>,
    /// The chip last pressed.
    chosen: Chip,
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
mod tests {
    use super::*;

    fn release(kind: ReleaseKind) -> Release {
        Release {
            uri: format!("spotify:release:{kind:?}"),
            name: "A release".to_owned(),
            kind,
            year: None,
            cover: None,
            tracks: Vec::new(),
        }
    }

    fn releases(kinds: &[ReleaseKind]) -> Vec<Release> {
        kinds.iter().copied().map(release).collect()
    }

    #[test]
    fn chips_for_no_releases_is_just_all() {
        assert_eq!(chips(&[]), vec![Chip::All]);
    }

    #[test]
    fn chips_follow_the_fixed_kind_order_not_the_release_order() {
        let releases = releases(&[
            ReleaseKind::Compilation,
            ReleaseKind::Single,
            ReleaseKind::Ep,
            ReleaseKind::Album,
        ]);
        assert_eq!(
            chips(&releases),
            vec![
                Chip::All,
                Chip::Kind(ReleaseKind::Album),
                Chip::Kind(ReleaseKind::Ep),
                Chip::Kind(ReleaseKind::Single),
                Chip::Kind(ReleaseKind::Compilation),
            ]
        );
    }

    #[test]
    fn a_kind_with_no_release_gets_no_chip() {
        let releases = releases(&[ReleaseKind::Album, ReleaseKind::Single]);
        assert_eq!(
            chips(&releases),
            vec![
                Chip::All,
                Chip::Kind(ReleaseKind::Album),
                Chip::Kind(ReleaseKind::Single),
            ]
        );
    }

    #[test]
    fn other_gets_no_chip_even_when_it_is_the_only_kind() {
        assert_eq!(chips(&releases(&[ReleaseKind::Other])), vec![Chip::All]);
    }

    #[test]
    fn duplicates_of_a_kind_give_one_chip() {
        let releases = releases(&[ReleaseKind::Album, ReleaseKind::Album]);
        assert_eq!(
            chips(&releases),
            vec![Chip::All, Chip::Kind(ReleaseKind::Album)]
        );
    }

    #[test]
    fn all_matches_every_kind_including_other() {
        for kind in [
            ReleaseKind::Album,
            ReleaseKind::Ep,
            ReleaseKind::Single,
            ReleaseKind::Compilation,
            ReleaseKind::Other,
        ] {
            assert!(Chip::All.matches(kind), "{kind:?}");
        }
    }

    #[test]
    fn a_kind_chip_matches_only_its_kind() {
        let chip = Chip::Kind(ReleaseKind::Ep);
        assert!(chip.matches(ReleaseKind::Ep));
        for kind in [
            ReleaseKind::Album,
            ReleaseKind::Single,
            ReleaseKind::Compilation,
            ReleaseKind::Other,
        ] {
            assert!(!chip.matches(kind), "{kind:?}");
        }
    }

    #[test]
    fn a_new_filter_is_all() {
        assert_eq!(Filter::default().chosen(), Chip::All);
    }

    #[test]
    fn choose_sets_the_chip() {
        let mut filter = Filter::default();
        filter.choose(Chip::Kind(ReleaseKind::Single));
        assert_eq!(filter.chosen(), Chip::Kind(ReleaseKind::Single));
    }

    #[test]
    fn pressing_the_chosen_chip_again_keeps_it() {
        let mut filter = Filter::default();
        filter.choose(Chip::Kind(ReleaseKind::Ep));
        filter.choose(Chip::Kind(ReleaseKind::Ep));
        assert_eq!(
            filter.chosen(),
            Chip::Kind(ReleaseKind::Ep),
            "the row is exclusive and there is no toggle path"
        );
    }

    #[test]
    fn all_is_the_way_back_out() {
        let mut filter = Filter::default();
        filter.choose(Chip::Kind(ReleaseKind::Compilation));
        filter.choose(Chip::All);
        assert_eq!(filter.chosen(), Chip::All);
    }

    #[test]
    fn open_the_same_artist_keeps_the_choice() {
        let mut filter = Filter::default();
        filter.open("a");
        filter.choose(Chip::Kind(ReleaseKind::Album));
        filter.open("a");
        assert_eq!(filter.chosen(), Chip::Kind(ReleaseKind::Album));
    }

    #[test]
    fn open_another_artist_resets_to_all() {
        let mut filter = Filter::default();
        filter.open("a");
        filter.choose(Chip::Kind(ReleaseKind::Album));
        filter.open("b");
        assert_eq!(filter.chosen(), Chip::All);
        assert_eq!(filter.artist(), Some("b"));
    }

    #[test]
    fn effective_falls_back_to_all_when_the_choice_has_no_chip() {
        let mut filter = Filter::default();
        filter.open("a");
        filter.choose(Chip::Kind(ReleaseKind::Ep));
        let row = vec![Chip::All, Chip::Kind(ReleaseKind::Album)];
        assert_eq!(filter.effective(&row), Chip::All);
    }

    #[test]
    fn a_fallback_does_not_overwrite_the_choice() {
        let mut filter = Filter::default();
        filter.open("a");
        filter.choose(Chip::Kind(ReleaseKind::Ep));
        let without = vec![Chip::All, Chip::Kind(ReleaseKind::Album)];
        assert_eq!(filter.effective(&without), Chip::All);

        let with = vec![
            Chip::All,
            Chip::Kind(ReleaseKind::Album),
            Chip::Kind(ReleaseKind::Ep),
        ];
        assert_eq!(
            filter.effective(&with),
            Chip::Kind(ReleaseKind::Ep),
            "the choice is remembered, not replaced"
        );
        assert_eq!(filter.chosen(), Chip::Kind(ReleaseKind::Ep));
    }
}
