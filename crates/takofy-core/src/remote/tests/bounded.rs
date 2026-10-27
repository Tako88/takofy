use super::super::*;

use std::time::Duration;

#[tokio::test]
async fn bounded_gives_none_for_a_future_that_never_completes() {
    let pending = std::future::pending::<u8>();
    assert_eq!(bounded(pending, Duration::from_millis(1)).await, None);
}

#[tokio::test]
async fn bounded_gives_some_for_a_future_that_does_complete() {
    assert_eq!(
        bounded(std::future::ready(7_u8), Duration::from_secs(30)).await,
        Some(7)
    );
}
