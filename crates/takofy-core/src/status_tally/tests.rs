use super::StatusTally;

#[test]
fn all_ok_summarises_to_nothing() {
    let mut tally = StatusTally::default();
    tally.record_provider(200);
    tally.record_entity("spotify:track:a", 200);
    tally.record_entity("spotify:track:b", 0);
    assert_eq!(tally.summary(), None);
}

#[test]
fn entity_codes_are_counted_by_distinct_uri() {
    let mut tally = StatusTally::default();
    tally.record_entity("spotify:track:a", 404);
    tally.record_entity("spotify:track:b", 404);
    tally.record_entity("spotify:track:c", 500);
    assert_eq!(
        tally.summary().as_deref(),
        Some("metadata statuses: entities 404 x2, 500 x1")
    );
}

#[test]
fn the_same_uri_recorded_twice_counts_once() {
    let mut tally = StatusTally::default();
    tally.record_entity("spotify:track:a", 404);
    tally.record_entity("spotify:track:a", 404);
    assert_eq!(
        tally.summary().as_deref(),
        Some("metadata statuses: entities 404 x1")
    );
}

#[test]
fn provider_and_entity_codes_are_reported_apart() {
    let mut tally = StatusTally::default();
    tally.record_provider(500);
    tally.record_provider(500);
    tally.record_entity("spotify:track:a", 404);
    assert_eq!(
        tally.summary().as_deref(),
        Some("metadata statuses: provider 500 x2; entities 404 x1")
    );
}

#[test]
fn merge_unions_the_uri_sets_and_adds_the_provider_counts() {
    let mut left = StatusTally::default();
    left.record_provider(500);
    left.record_entity("spotify:track:a", 404);
    let mut right = StatusTally::default();
    right.record_provider(500);
    right.record_entity("spotify:track:a", 404);
    right.record_entity("spotify:track:b", 404);

    left.merge(right);

    assert_eq!(
        left.summary().as_deref(),
        Some("metadata statuses: provider 500 x2; entities 404 x2")
    );
}

#[test]
fn a_zero_code_is_a_success() {
    let mut tally = StatusTally::default();
    // An absent header reads as the protobuf default, whose success code is zero.
    tally.record_entity("spotify:track:a", 0);
    assert_eq!(tally.summary(), None);
}

#[test]
fn the_summary_carries_codes_and_counts_but_no_uris() {
    let mut tally = StatusTally::default();
    tally.record_entity("spotify:track:secret", 404);
    let summary = tally.summary().expect("a failed entity is reported");
    assert!(!summary.contains("spotify:"), "{summary}");
    assert!(!summary.contains("secret"), "{summary}");
    assert_eq!(summary, "metadata statuses: entities 404 x1");
}
