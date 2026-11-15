//! Pure query handling only: no socket is opened.

use super::split_query;

#[test]
fn a_path_without_a_query_has_no_pairs() {
    let (path, pairs) = split_query("/genre");
    assert_eq!(path, "/genre");
    assert!(pairs.is_empty());
}

#[test]
fn a_query_is_split_into_decoded_pairs() {
    let (path, pairs) = split_query("/artist/1/top?limit=10&q=Simon%20%26%20Garfunkel");
    assert_eq!(path, "/artist/1/top");
    assert_eq!(
        pairs,
        vec![
            ("limit".to_owned(), "10".to_owned()),
            ("q".to_owned(), "Simon & Garfunkel".to_owned()),
        ]
    );
}

#[test]
fn plus_decodes_to_a_space() {
    let (_, pairs) = split_query("/search/artist?q=two+words");
    assert_eq!(pairs, vec![("q".to_owned(), "two words".to_owned())]);
}

#[test]
fn a_percent_sign_decodes() {
    let (_, pairs) = split_query("/search/artist?q=100%25");
    assert_eq!(pairs, vec![("q".to_owned(), "100%".to_owned())]);
}

#[test]
fn a_broken_escape_is_kept_as_text() {
    let (_, pairs) = split_query("/search/artist?q=100%2");
    assert_eq!(pairs, vec![("q".to_owned(), "100%2".to_owned())]);
}
