//! Constructed lists only: every title, artist and URI below is invented, so
//! the thresholds are pinned by hand rather than by a captured payload.

use takofy_model::TrackEntry;

use super::{OVERLAP_MIN, SMALL_LIST, confirm_hit, normalise, overlaps};

fn track(title: &str, names: &[&str], uris: &[&str]) -> TrackEntry {
    TrackEntry {
        uri: "spotify:track:synthetic".to_owned(),
        title: title.to_owned(),
        artists: names.join(", "),
        album: None,
        album_uri: None,
        artist_uris: uris.iter().map(|uri| (*uri).to_owned()).collect(),
        artist_names: names.iter().map(|name| (*name).to_owned()).collect(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

fn titles(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn the_right_artist_is_accepted() {
    let entry = track("Signal Fire", &["Vela"], &["spotify:artist:vela"]);
    assert_eq!(confirm_hit(&entry, "Signal Fire", "Vela"), Some(0));
}

#[test]
fn the_zip_returns_the_index_of_the_matching_uri() {
    let entry = track(
        "Signal Fire",
        &["Someone Else", "Vela"],
        &["spotify:artist:someone", "spotify:artist:vela"],
    );
    let index = confirm_hit(&entry, "Signal Fire", "Vela").expect("the second credit");
    assert_eq!(index, 1);
    assert_eq!(entry.artist_uris[index], "spotify:artist:vela");
}

#[test]
fn a_same_name_homonym_is_rejected() {
    let entry = track("Different Song", &["Vela"], &["spotify:artist:vela"]);
    assert_eq!(confirm_hit(&entry, "Signal Fire", "Vela"), None);
}

#[test]
fn an_unrelated_act_is_rejected() {
    let entry = track("Signal Fire", &["Grouplove"], &["spotify:artist:grouplove"]);
    assert_eq!(confirm_hit(&entry, "Signal Fire", "Grouper"), None);
}

#[test]
fn a_name_match_with_the_wrong_title_is_rejected() {
    let entry = track("Signal Fire", &["Vela"], &["spotify:artist:vela"]);
    assert_eq!(confirm_hit(&entry, "Night Watch", "Vela"), None);
}

#[test]
fn empty_credit_lists_are_rejected() {
    let entry = track("Signal Fire", &[], &[]);
    assert_eq!(confirm_hit(&entry, "Signal Fire", "Vela"), None);
}

#[test]
fn a_blank_wanted_name_is_rejected() {
    let entry = track("Signal Fire", &[""], &["spotify:artist:vela"]);
    assert_eq!(confirm_hit(&entry, "Signal Fire", ""), None);
}

#[test]
fn remaster_and_punctuation_variants_are_accepted() {
    assert_eq!(normalise("Signal Fire (Remastered 2011)"), "signalfire");
    assert_eq!(normalise("Signal Fire - Remastered 2011"), "signalfire");
    assert_eq!(normalise("Signal Fire!"), "signalfire");
    assert_eq!(normalise("Signal  Fire"), "signalfire");
    let entry = track("Signal Fire [Live]", &["Vela!"], &["spotify:artist:vela"]);
    assert_eq!(confirm_hit(&entry, "Signal Fire", "Vela"), Some(0));
}

#[test]
fn a_brace_group_is_stripped_like_any_other_bracket() {
    assert_eq!(normalise("Signal Fire {Live}"), "signalfire");
    assert_eq!(
        normalise("Signal Fire {unclosed"),
        "signalfireunclosed",
        "an unclosed group is part of the title"
    );
}

#[test]
fn a_title_that_normalises_to_nothing_matches_nothing() {
    let entry = track("(Intro)", &["Vela"], &["spotify:artist:vela"]);
    assert_eq!(normalise("(Intro)"), "");
    assert_eq!(
        confirm_hit(&entry, "(Intro)", "Vela"),
        None,
        "two titles that both normalise to nothing are no evidence"
    );
}

#[test]
fn diacritics_are_not_folded() {
    assert_ne!(normalise("Björk"), normalise("Bjork"));
}

#[test]
fn overlap_needs_two_titles_once_both_lists_are_large_enough() {
    let spotify = titles(&["One", "Two", "Three", "Four"]);
    assert!(!overlaps(
        &spotify,
        &titles(&["One", "Five", "Six", "Seven"])
    ));
    assert!(overlaps(&spotify, &titles(&["One", "Two", "Six", "Seven"])));
}

#[test]
fn overlap_needs_one_title_when_a_list_is_small() {
    let spotify = titles(&["One", "Two", "Three"]);
    assert!(overlaps(&spotify, &titles(&["One", "Four", "Five"])));
    assert!(!overlaps(&spotify, &titles(&["Four", "Five", "Six"])));
}

#[test]
fn repeated_titles_do_not_count_twice() {
    let spotify = titles(&["One", "One", "One", "One"]);
    let deezer = titles(&["One", "One", "Two", "Three"]);
    assert!(!overlaps(&spotify, &deezer));
    assert_eq!(OVERLAP_MIN, 2);
    assert_eq!(SMALL_LIST, 4);
}
