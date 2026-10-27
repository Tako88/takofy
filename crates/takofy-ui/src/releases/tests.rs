use super::*;

fn release(uri: &str, tracks: &[&str]) -> Release {
    Release {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        kind: takofy_model::ReleaseKind::Album,
        year: None,
        cover: None,
        tracks: tracks.iter().map(|track| (*track).to_owned()).collect(),
    }
}

fn track(uri: &str) -> TrackEntry {
    TrackEntry {
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
    }
}

fn load(release: &str, tracks: &[&str]) -> Command {
    Command::LoadReleaseTracks {
        release: release.to_owned(),
        tracks: tracks.iter().map(|track| (*track).to_owned()).collect(),
    }
}

fn store(releases: &[Release]) -> Releases {
    let mut store = Releases::default();
    store.retain(releases);
    store
}

#[test]
fn a_look_records_a_want_and_still_reads_idle() {
    let mut store = store(&[release("a", &["t1"])]);
    assert_eq!(store.look("a"), Labels::Idle);
    assert_eq!(store.wanted, vec!["a".to_owned()]);
}

#[test]
fn a_look_at_an_unknown_release_asks_for_nothing() {
    let mut store = store(&[release("a", &["t1"])]);
    assert_eq!(store.look("b"), Labels::Idle);
    assert!(store.requests().is_empty());
}

#[test]
fn the_drain_emits_one_request_and_marks_it_in_flight() {
    let mut store = store(&[release("a", &["t1", "t2"])]);
    store.look("a");
    assert_eq!(store.requests(), vec![load("a", &["t1", "t2"])]);
    assert_eq!(store.look("a"), Labels::Loading);
}

#[test]
fn a_release_in_flight_is_not_asked_for_again() {
    let mut store = store(&[release("a", &["t1"])]);
    store.look("a");
    let _ = store.requests();
    store.look("a");
    assert!(store.requests().is_empty());
}

#[test]
fn a_ready_release_is_never_asked_for_again() {
    let mut store = store(&[release("a", &["t1"])]);
    store.look("a");
    let _ = store.requests();
    store.apply(&Update::ReleaseTracks {
        release: "a".to_owned(),
        tracks: vec![track("t1")],
    });

    assert_eq!(store.look("a"), Labels::Ready(&[track("t1")]));
    assert!(store.requests().is_empty());
}

#[test]
fn a_reply_lands_only_on_the_release_it_names() {
    let mut store = store(&[release("a", &["t1"]), release("b", &["t2"])]);
    store.look("a");
    let _ = store.requests();

    store.apply(&Update::ReleaseTracks {
        release: "b".to_owned(),
        tracks: vec![track("t2")],
    });
    assert_eq!(store.look("b"), Labels::Idle, "B was never asked for");
    assert_eq!(store.look("a"), Labels::Loading);
}

#[test]
fn a_reply_for_a_release_that_is_not_in_flight_is_dropped() {
    let mut store = store(&[release("a", &["t1"])]);
    store.apply(&Update::ReleaseTracks {
        release: "a".to_owned(),
        tracks: vec![track("t1")],
    });
    assert_eq!(store.look("a"), Labels::Idle);

    store.look("a");
    let _ = store.requests();
    store.apply(&Update::ReleaseTracks {
        release: "gone".to_owned(),
        tracks: vec![track("x")],
    });
    assert_eq!(
        store.look("a"),
        Labels::Loading,
        "the stray reply is not A's"
    );
}

#[test]
fn a_failure_is_recorded_and_not_retried_until_the_page_reloads() {
    let mut store = store(&[release("a", &["t1"])]);
    store.look("a");
    let _ = store.requests();
    store.apply(&Update::ReleaseTracksFailed {
        release: "a".to_owned(),
        failure: Failure::Load,
    });

    assert_eq!(store.look("a"), Labels::Failed(Failure::Load));
    assert!(
        store.requests().is_empty(),
        "a failure is not retried by a look"
    );

    store.retain(&[release("a", &["t1"])]);
    assert_eq!(store.look("a"), Labels::Idle, "a reload re-arms it");
    assert_eq!(store.requests(), vec![load("a", &["t1"])]);
}

#[test]
fn the_drain_caps_at_two_and_frees_a_slot_per_answer() {
    let releases = [
        release("a", &["t1"]),
        release("b", &["t2"]),
        release("c", &["t3"]),
        release("d", &["t4"]),
    ];
    let mut store = store(&releases);
    for entry in &releases {
        store.look(&entry.uri);
    }
    assert_eq!(
        store.requests(),
        vec![load("a", &["t1"]), load("b", &["t2"])],
        "the cap holds at MAX_IN_FLIGHT"
    );
    assert!(
        store.requests().is_empty(),
        "the leftover wants are re-looked"
    );

    store.apply(&Update::ReleaseTracks {
        release: "a".to_owned(),
        tracks: vec![track("t1")],
    });
    for entry in &releases {
        store.look(&entry.uri);
    }
    assert_eq!(
        store.requests(),
        vec![load("c", &["t3"])],
        "one answer frees exactly one slot"
    );
}

#[test]
fn retain_drops_the_releases_the_page_no_longer_holds() {
    let mut store = store(&[release("a", &["t1"]), release("b", &["t2"])]);
    store.look("a");
    let _ = store.requests();

    store.retain(&[release("b", &["t2"])]);
    assert!(!store.entries.contains_key("a"));
    assert_eq!(store.look("a"), Labels::Idle);
    assert!(store.requests().is_empty());
}

#[test]
fn retain_refreshes_a_releases_tracks() {
    let mut store = store(&[release("a", &["t1"])]);
    store.look("a");
    let _ = store.requests();
    store.apply(&Update::ReleaseTracks {
        release: "a".to_owned(),
        tracks: vec![track("t1")],
    });

    store.retain(&[release("a", &["t1", "t9"])]);
    assert_eq!(
        store.look("a"),
        Labels::Ready(&[track("t1")]),
        "a ready release keeps its labels"
    );
    assert_eq!(store.entries["a"].tracks, vec!["t1", "t9"]);
}
