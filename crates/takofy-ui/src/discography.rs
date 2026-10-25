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

    fn named(kind: ReleaseKind, name: &str, year: Option<u16>) -> Release {
        Release {
            uri: format!("spotify:release:{name}"),
            name: name.to_owned(),
            kind,
            year,
            cover: None,
            tracks: Vec::new(),
        }
    }

    #[test]
    fn a_name_matches_case_insensitively_by_substring() {
        let releases = vec![
            named(ReleaseKind::Album, "Midnight City", Some(2011)),
            named(ReleaseKind::Album, "Hurry Up, We're Dreaming", Some(2011)),
        ];
        assert_eq!(narrow(&releases, Chip::All, "midnight"), vec![0]);
        assert_eq!(narrow(&releases, Chip::All, "DREAMING"), vec![1]);
        assert_eq!(narrow(&releases, Chip::All, "city"), vec![0]);
    }

    #[test]
    fn non_ascii_names_fold() {
        let releases = vec![
            named(ReleaseKind::Album, "ÉTÉ", None),
            named(ReleaseKind::Album, "HANA SŌ", None),
        ];
        assert_eq!(narrow(&releases, Chip::All, "é"), vec![0]);
        assert_eq!(narrow(&releases, Chip::All, "Ō"), vec![1]);
    }

    #[test]
    fn the_query_is_trimmed_and_blank_is_no_filter() {
        let releases = vec![
            named(ReleaseKind::Album, "One", None),
            named(ReleaseKind::Ep, "Two", None),
        ];
        assert_eq!(narrow(&releases, Chip::All, "  one  "), vec![0]);
        assert_eq!(narrow(&releases, Chip::All, "   "), vec![0, 1]);
    }

    #[test]
    fn only_the_name_is_read() {
        let releases = vec![
            named(ReleaseKind::Album, "One", Some(2011)),
            named(ReleaseKind::Ep, "Two", Some(2024)),
        ];
        assert_eq!(
            narrow(&releases, Chip::All, "album"),
            Vec::<usize>::new(),
            "a kind word is not part of the release's name"
        );
        assert_eq!(
            narrow(&releases, Chip::All, "2024"),
            Vec::<usize>::new(),
            "the year is not part of the release's name"
        );
    }

    #[test]
    fn narrow_composes_the_chip_and_the_name_and_returns_true_ascending_indices() {
        let releases = vec![
            named(ReleaseKind::Album, "Album One", None),
            named(ReleaseKind::Ep, "An EP", None),
            named(ReleaseKind::Album, "Album Two", None),
            named(ReleaseKind::Ep, "Another EP", None),
        ];
        assert_eq!(narrow(&releases, Chip::All, "album"), vec![0, 2]);
        assert_eq!(
            narrow(&releases, Chip::Kind(ReleaseKind::Ep), "album"),
            Vec::<usize>::new(),
            "the two axes multiply"
        );
        let found = narrow(&releases, Chip::All, "e");
        assert_eq!(found, vec![0, 1, 3]);
        assert!(found.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn the_query_belongs_to_the_artist() {
        let mut filter = Filter::default();
        filter.open("a");
        filter.edit().push_str("  Midnight  ");
        assert_eq!(filter.needle().as_deref(), Some("midnight"));

        filter.open("b");
        assert_eq!(filter.needle(), None, "another artist starts unfiltered");
        assert!(filter.edit().is_empty());
    }

    #[test]
    fn clear_query_keeps_the_chip() {
        let mut filter = Filter::default();
        filter.open("a");
        filter.choose(Chip::Kind(ReleaseKind::Ep));
        filter.edit().push_str("midnight");
        filter.clear_query();
        assert_eq!(filter.needle(), None);
        assert!(filter.edit().is_empty());
        assert_eq!(filter.chosen(), Chip::Kind(ReleaseKind::Ep));
    }

    #[test]
    fn emptied_is_none_while_rows_are_shown() {
        assert_eq!(emptied(1, false), None);
        assert_eq!(emptied(1, true), None);
    }

    #[test]
    fn emptied_names_the_query_when_one_is_typed() {
        assert_eq!(emptied(0, true), Some(Emptied::Query));
    }

    #[test]
    fn emptied_names_the_format_with_no_query() {
        assert_eq!(emptied(0, false), Some(Emptied::Format));
    }

    #[test]
    fn the_query_wins_when_both_axes_empty_the_list() {
        assert_eq!(
            emptied(0, true),
            Some(Emptied::Query),
            "only one sentence is ever on screen, and it is the query's"
        );
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
