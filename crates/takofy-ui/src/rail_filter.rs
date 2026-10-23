//! The library rail's filter: the query the head's field keeps, and the rows
//! it leaves.
//!
//! The query belongs to the rail, not to a page: it is unkeyed, so it survives
//! every page change and every reload of any library list, and a redraw while a
//! list is loading changes nothing. Only the field's own clear control, an
//! Escape while the field owns the keyboard, and the create field's opening
//! clear it.
//!
//! Matching is trimmed and case-insensitive over a row's name alone, because the
//! kind line would make "art" match every artist. The answer is the rows' true
//! indices, so a narrowed row still names the playlist it stands for.

/// The rail's query: one string, with no page to belong to.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RailFilter {
    text: String,
}

impl RailFilter {
    /// The text the field edits.
    pub fn edit(&mut self) -> &mut String {
        &mut self.text
    }

    /// The trimmed query as it was typed, for the line that names it.
    pub fn query(&self) -> &str {
        self.text.trim()
    }

    /// The needle the rows match, when the query is not blank.
    pub fn needle(&self) -> Option<String> {
        let trimmed = self.text.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_lowercase())
    }

    /// Drops the query, so the whole list shows again.
    pub fn clear(&mut self) {
        self.text.clear();
    }
}

/// The ascending true indices whose name contains `needle`.
///
/// It takes names rather than rows, so it does not depend on any one list's
/// shape: the sidebar answers the four kinds with the same call.
pub fn keep<'a>(names: impl Iterator<Item = &'a str>, needle: &str) -> Vec<usize> {
    names
        .enumerate()
        .filter(|(_, name)| name.to_lowercase().contains(needle))
        .map(|(index, _)| index)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track_filter::QUERY_MAX;

    #[test]
    fn keep_matches_names_case_insensitively() {
        let names = ["Midnight City", "Outro", "midnight sun"];

        assert_eq!(keep(names.iter().copied(), "midnight"), vec![0, 2]);
        assert_eq!(keep(names.iter().copied(), "outro"), vec![1]);
    }

    #[test]
    fn keep_returns_ascending_true_indices() {
        let matching = [1, 4, 9, 12, 20, 23, 31, 38, 44, 51, 57, 60];
        let names: Vec<String> = (0..62)
            .map(|index| {
                if matching.contains(&index) {
                    format!("midnight {index}")
                } else {
                    format!("other {index}")
                }
            })
            .collect();

        let found = keep(names.iter().map(String::as_str), "midnight");

        assert_eq!(found, matching.to_vec());
        assert!(found.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn non_ascii_names_case_fold() {
        let names = ["Été", "Hana Sō"];

        assert_eq!(keep(names.iter().copied(), "é"), vec![0]);
        assert_eq!(keep(names.iter().copied(), "ō"), vec![1]);
    }

    #[test]
    fn a_query_is_trimmed_and_blank_is_none() {
        let mut filter = RailFilter::default();
        assert_eq!(filter.needle(), None);

        filter.edit().push_str("  Midnight  ");
        assert_eq!(filter.needle().as_deref(), Some("midnight"));

        filter.clear();
        filter.edit().push_str("   ");
        assert_eq!(filter.needle(), None, "whitespace is no filter at all");
    }

    #[test]
    fn query_returns_trimmed_original_case() {
        let mut filter = RailFilter::default();
        filter.edit().push_str("  Midnight City  ");

        assert_eq!(filter.query(), "Midnight City");

        filter.clear();
        assert_eq!(filter.query(), "", "a cleared field names nothing");
    }

    #[test]
    fn the_field_accepts_the_track_filter_bound() {
        let mut filter = RailFilter::default();
        filter.edit().push_str(&"a".repeat(QUERY_MAX));
        assert_eq!(filter.query().len(), QUERY_MAX);
    }
}
