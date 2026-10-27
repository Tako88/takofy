use super::super::*;

#[test]
fn an_update_id_carries_the_millisecond_and_a_counter() {
    assert_eq!(update_id(1_700_000_000_123, 0), "takofy-1700000000123-0");
    assert_ne!(
        update_id(1_700_000_000_123, 0),
        update_id(1_700_000_000_123, 1),
        "batches within one millisecond must still differ"
    );
}

#[test]
fn every_client_update_id_of_a_run_is_different() {
    let first = client_update_id();
    let second = client_update_id();
    assert_ne!(first, second);
    assert!(first.starts_with("takofy-"), "{first}");
}
