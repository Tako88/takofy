//! The shared state: the gate, the retried genre table and the error body.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::test_support::FakeFetch;
use super::{FetchError, Outside, OutsideError};

const GENRE: &str = include_str!("fixtures/genre.json");

fn outside(fetch: &FakeFetch, gate: bool) -> Outside<FakeFetch> {
    Outside::new(fetch.clone(), Arc::new(AtomicBool::new(gate)))
}

#[tokio::test]
async fn the_gate_is_read_before_any_fetch() {
    let fetch = FakeFetch::new();
    fetch.ok("/genre", GENRE.as_bytes().to_vec());
    let outside = outside(&fetch, false);

    assert_eq!(outside.get("/genre").await, Err(OutsideError::Off));
    assert_eq!(fetch.count(), 0);
}

#[tokio::test(start_paused = true)]
async fn a_quota_error_body_pauses_the_outside() {
    let fetch = FakeFetch::new();
    fetch.ok(
        "/genre",
        br#"{"error":{"type":"Exception","code":4}}"#.to_vec(),
    );
    let outside = outside(&fetch, true);

    assert_eq!(outside.get("/genre").await, Err(OutsideError::Backoff));
    assert_eq!(fetch.count(), 1);
}

#[tokio::test]
async fn a_no_data_error_body_is_not_a_pause() {
    let fetch = FakeFetch::new();
    fetch.ok(
        "/genre",
        br#"{"error":{"type":"DataException","code":800}}"#.to_vec(),
    );
    fetch.ok("/genre", GENRE.as_bytes().to_vec());
    let outside = outside(&fetch, true);

    assert_eq!(outside.get("/genre").await, Err(OutsideError::Absent));
    assert_eq!(
        outside.get("/genre").await,
        Ok(GENRE.as_bytes().to_vec()),
        "a record the service does not hold does not pause the next request"
    );
    assert_eq!(fetch.count(), 2);
}

#[tokio::test]
async fn a_failed_genre_table_is_not_cached() {
    let fetch = FakeFetch::new();
    fetch.push("/genre", Err(FetchError::Transport));
    fetch.ok("/genre", GENRE.as_bytes().to_vec());
    let outside = outside(&fetch, true);

    assert_eq!(outside.genres().await, Err(OutsideError::Failed));
    let table = outside.genres().await.expect("the retry answers");
    assert_eq!(table.entries().len(), 22);
    assert_eq!(fetch.count(), 2);
}

#[tokio::test]
async fn a_loaded_genre_table_is_cached() {
    let fetch = FakeFetch::new();
    fetch.ok("/genre", GENRE.as_bytes().to_vec());
    let outside = outside(&fetch, true);

    let first = outside.genres().await.expect("the first read");
    let second = outside.genres().await.expect("the cached read");

    assert_eq!(first, second);
    assert_eq!(fetch.count(), 1);
}
