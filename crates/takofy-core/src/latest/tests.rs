use std::time::Duration;

use takofy_model::{Status, TrackEntry};

use super::*;

const WAIT: Duration = Duration::from_millis(20);

fn report(uri: &str) -> Report {
    Report {
        status: Status::Playing,
        track: Some(TrackEntry {
            uri: uri.to_owned(),
            title: uri.to_owned(),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            artist_names: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
        }),
        prev: Vec::new(),
        next: Vec::new(),
        context: None,
        queue_revision: String::new(),
        options: None,
        position_ms: 1_000,
        duration_ms: Some(200_000),
    }
}

#[test]
fn a_report_is_kept_with_its_age() {
    let latest = Latest::default();
    assert!(latest.set(Some(report("a"))));
    std::thread::sleep(WAIT);

    let (kept, age) = latest.report().expect("the report is held");
    assert_eq!(kept, report("a"));
    assert!(
        age >= WAIT,
        "the age is the time since it was taken: {age:?}"
    );
    assert!(latest.has_report());
}

#[tokio::test]
async fn setting_the_same_report_wakes_nobody() {
    let latest = Latest::default();
    assert!(latest.set(Some(report("a"))));
    let _ = latest.changed().await;

    std::thread::sleep(WAIT);
    let (_, before) = latest.report().expect("the report is held");
    assert!(
        !latest.set(Some(report("a"))),
        "the same report is no change"
    );
    let (_, after) = latest.report().expect("the report is held");
    assert!(after >= before, "a repeated report keeps its stamp");

    assert!(
        tokio::time::timeout(Duration::from_millis(20), latest.changed())
            .await
            .is_err(),
        "a repeated report wakes nobody"
    );

    assert!(latest.set(Some(report("b"))));
    assert!(
        tokio::time::timeout(Duration::from_millis(20), latest.changed())
            .await
            .is_ok(),
        "a different report wakes the publisher"
    );
}

#[tokio::test]
async fn none_clears() {
    let latest = Latest::default();
    assert!(latest.set(Some(report("a"))));
    let _ = latest.changed().await;

    assert!(latest.set(None));
    assert!(latest.report().is_none());
    assert!(!latest.has_report());
    assert!(
        tokio::time::timeout(Duration::from_millis(20), latest.changed())
            .await
            .is_ok(),
        "clearing is a change"
    );
    assert!(!latest.set(None), "clearing what is empty is no change");
}
