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
