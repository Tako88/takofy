//! Constructed record lists over the synthetic genre table.

use super::vote;
use crate::discovery::wire::genre_table;

const GENRE: &str = include_str!("../fixtures/genre.json");

fn table() -> crate::discovery::wire::GenreTable {
    genre_table(GENRE.as_bytes()).expect("a synthetic table")
}

#[test]
fn a_clear_winner_names_the_genre() {
    assert_eq!(vote(&[3, 3, 3, 11], &table()), Some("Rock".to_owned()));
}

#[test]
fn exactly_half_is_enough() {
    assert_eq!(vote(&[3, 3, 11, 11], &table()), Some("Rock".to_owned()));
}

#[test]
fn one_fewer_than_half_is_silent() {
    // Four records, four different genres, so the first in table order holds
    // one of four and stays silent.
    assert_eq!(vote(&[3, 2, 4, 5], &table()), None);
}

#[test]
fn dropped_genres_do_not_vote_but_still_count() {
    // All, Pop and Jazz are dropped, so Rock is one of four and stays silent.
    assert_eq!(vote(&[0, 1, 10, 3], &table()), None);
    // The same records with Rock twice reach half once the denominator is four.
    assert_eq!(vote(&[0, 10, 3, 3], &table()), Some("Rock".to_owned()));
}

#[test]
fn all_is_dropped_even_when_it_is_the_majority() {
    assert_eq!(vote(&[0, 0, 0, 3], &table()), None);
}

#[test]
fn an_empty_list_is_silent() {
    assert_eq!(vote(&[], &table()), None);
}

#[test]
fn an_unknown_id_counts_in_the_denominator_but_votes_for_nothing() {
    assert_eq!(vote(&[3, 9999], &table()), Some("Rock".to_owned()));
    assert_eq!(vote(&[3, 9999, 9999], &table()), None);
}

#[test]
fn a_single_record_is_enough() {
    assert_eq!(vote(&[3], &table()), Some("Rock".to_owned()));
    assert_eq!(vote(&[0], &table()), None);
}

#[test]
fn a_tie_resolves_by_the_table_order() {
    // Rock is id 3, Alternative is id 6, and both have two votes of four.
    assert_eq!(vote(&[6, 3, 6, 3], &table()), Some("Rock".to_owned()));
}

#[test]
fn an_empty_table_is_silent() {
    assert_eq!(
        vote(&[3], &crate::discovery::wire::GenreTable::default()),
        None
    );
}
