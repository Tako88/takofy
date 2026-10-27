use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{Engine, page_cover, resolve_within, search_page};
use crate::context::{ContextError, Page, PageSource, PageTrack, Resolved};
use crate::remote::titled_label;
use takofy_model::{CoverSet, CoverUrl, Report, Status, TrackEntry};

/// A source that records every URI it is asked to resolve and answers one
/// page with one track.
#[derive(Clone, Default)]
struct Recording {
    resolved: Arc<Mutex<Vec<String>>>,
}

impl Recording {
    fn resolved(&self) -> Vec<String> {
        self.resolved.lock().expect("lock").clone()
    }
}

impl PageSource for Recording {
    async fn resolve(&self, uri: &str) -> Result<Resolved, ContextError> {
        self.resolved.lock().expect("lock").push(uri.to_owned());
        Ok(Resolved {
            pages: vec![Page {
                tracks: vec![PageTrack {
                    uri: "spotify:track:a".to_owned(),
                    added_at: None,
                }],
                next: None,
            }],
            cover: None,
        })
    }

    async fn next(&self, url: &str) -> Result<Page, ContextError> {
        panic!("a search resolves once and never pages (asked for {url})");
    }
}

#[tokio::test]
async fn a_search_walks_a_search_context_uri() {
    let source = Recording::default();
    let page = search_page(source.clone(), "never gonna")
        .await
        .expect("walk")
        .expect("one page");

    assert_eq!(
        source.resolved(),
        vec!["spotify:search:never+gonna".to_owned()],
        "the walker must be given a context URI, not a bare query"
    );
    assert_eq!(
        page,
        Page {
            tracks: vec![PageTrack {
                uri: "spotify:track:a".to_owned(),
                added_at: None,
            }],
            next: None,
        }
    );
}

#[tokio::test]
async fn a_blank_query_resolves_nothing() {
    let source = Recording::default();
    assert_eq!(
        search_page(source.clone(), "  \t ").await.expect("walk"),
        None
    );
    assert_eq!(source.resolved(), Vec::<String>::new());
}

/// A source that never answers, so the budget is the only thing that can
/// end the read.
struct Silent;

impl PageSource for Silent {
    async fn resolve(&self, _uri: &str) -> Result<Resolved, ContextError> {
        std::future::pending().await
    }

    async fn next(&self, _url: &str) -> Result<Page, ContextError> {
        std::future::pending().await
    }
}

#[tokio::test]
async fn a_context_that_never_answers_times_out() {
    let error = resolve_within(Silent, "spotify:playlist:abc", Duration::from_millis(20))
        .await
        .expect_err("a read with no answer must not hold the caller open");
    assert!(
        matches!(error, ContextError::TimedOut),
        "the budget names itself, so a caller can tell it from a source failure"
    );
}

fn cover(tail: &str) -> CoverSet {
    CoverSet::single(
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url"),
    )
}

fn row(uri: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: "A Title".to_owned(),
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

/// Each engine builds its own cache, so a session never reads another
/// account's labels.
#[tokio::test]
async fn a_new_engine_starts_with_an_empty_label_cache() {
    let engine = Engine::new(librespot::core::Session::new(
        librespot::core::SessionConfig::default(),
        None,
    ));
    assert_eq!(engine.labels.len(), 0);
}

#[tokio::test]
async fn the_cache_belongs_to_the_engine_not_the_process() {
    let session = || librespot::core::Session::new(librespot::core::SessionConfig::default(), None);
    let first = Engine::new(session());
    let second = Engine::new(session());

    first.labels.store(&HashMap::from([(
        "spotify:track:a".to_owned(),
        titled_label("A Title"),
    )]));

    assert_eq!(first.labels.len(), 1);
    assert_eq!(second.labels.len(), 0);
}

/// The pages a playlist read labels through are handed the engine's own
/// cache, so a second visit is served by the first.
#[tokio::test]
async fn track_pages_borrow_the_engines_cache() {
    let engine = Engine::new(librespot::core::Session::new(
        librespot::core::SessionConfig::default(),
        None,
    ));
    let pages = engine.tracks("spotify:playlist:abc");
    assert!(std::ptr::eq(pages.labels, &engine.labels));
}

#[tokio::test]
async fn an_empty_portrait_request_answers_nothing_without_a_request() {
    // The empty case returns before the session is touched, which is what
    // makes a speculative ask free.
    let engine = Engine::new(librespot::core::Session::new(
        librespot::core::SessionConfig::default(),
        None,
    ));
    assert_eq!(engine.portraits(&[]).await, Some(Vec::new()));
}

#[test]
fn page_cover_prefers_the_context_image() {
    let rows = vec![TrackEntry {
        cover: Some(cover("row")),
        ..row("spotify:track:a")
    }];
    assert_eq!(
        page_cover(Some(cover("context")), &rows),
        Some(cover("context"))
    );
}

#[test]
fn page_cover_takes_the_first_track_rows_cover_when_the_context_names_none() {
    let rows = vec![
        TrackEntry {
            cover: Some(cover("first")),
            ..row("spotify:track:a")
        },
        TrackEntry {
            cover: Some(cover("second")),
            ..row("spotify:track:b")
        },
    ];
    assert_eq!(page_cover(None, &rows), Some(cover("first")));
}

#[test]
fn page_cover_skips_episodes_and_local_files_to_find_the_first_track() {
    let rows = vec![
        TrackEntry {
            cover: Some(cover("episode")),
            ..row("spotify:episode:1")
        },
        TrackEntry {
            cover: Some(cover("local")),
            ..row("spotify:local:a:b:c:1")
        },
        TrackEntry {
            cover: Some(cover("track")),
            ..row("spotify:track:a")
        },
    ];
    assert_eq!(page_cover(None, &rows), Some(cover("track")));
}

#[test]
fn page_cover_is_none_when_the_first_track_has_no_cover_even_if_the_second_has_one() {
    let rows = vec![
        row("spotify:track:a"),
        TrackEntry {
            cover: Some(cover("second")),
            ..row("spotify:track:b")
        },
    ];
    assert_eq!(page_cover(None, &rows), None);
}

#[test]
fn page_cover_is_none_for_an_empty_page() {
    assert_eq!(page_cover(None, &[]), None);
}

fn session() -> librespot::core::Session {
    librespot::core::Session::new(librespot::core::SessionConfig::default(), None)
}

fn report() -> Report {
    Report {
        status: Status::Playing,
        track: None,
        prev: Vec::new(),
        next: Vec::new(),
        context: None,
        queue_revision: String::new(),
        options: None,
        position_ms: 1_000,
        duration_ms: Some(200_000),
    }
}

#[tokio::test]
async fn a_fresh_session_is_not_down() {
    let engine = Engine::new(session());
    assert!(!engine.is_down());
}

#[tokio::test]
async fn a_shut_down_session_reads_as_down() {
    let held = session();
    let engine = Engine::new(held.clone());
    held.shutdown();
    assert!(engine.is_down());
}

#[tokio::test]
async fn closing_an_engine_invalidates_the_session_it_held() {
    let held = session();
    let engine = Engine::new(held.clone());
    engine.close();
    assert!(
        held.is_invalid(),
        "the session a close leaves behind must not look usable"
    );
}

#[tokio::test]
async fn restoring_reapplies_the_stage_and_the_last_report() {
    let engine = Engine::new(session());
    engine.restore(true, Some(report()));

    assert!(engine.presence.playable());
    assert!(engine.latest.has_report());
}

#[tokio::test]
async fn restoring_nothing_leaves_a_fresh_engine_empty() {
    let engine = Engine::new(session());
    engine.restore(false, None);

    assert!(!engine.presence.playable());
    assert!(!engine.latest.has_report());
}
