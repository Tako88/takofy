use std::collections::HashMap;
use std::sync::Mutex;

use librespot::protocol::context_page::ContextPage;
use librespot::protocol::context_track::ContextTrack;

use super::{
    ContextError, MAX_PAGES, Page, PageSource, PageTrack, Resolved, Walker, first_pages,
    page_from_proto, parse_page_json, resolve_uris,
};
use takofy_model::{AddedAt, CoverSet, CoverUrl};

const TRACK_A: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQC";
const TRACK_B: &str = "spotify:track:6rqhFgbbKwnb9MLmUQDhG6";

fn cover() -> CoverSet {
    CoverSet::single(CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url"))
}

fn date(wire: &str) -> AddedAt {
    AddedAt::from_wire(wire).expect("a date on the wire")
}

fn page(tracks: &[&str], next: Option<&str>) -> Page {
    Page {
        tracks: tracks
            .iter()
            .map(|track| PageTrack {
                uri: (*track).to_owned(),
                added_at: None,
            })
            .collect(),
        next: next.map(str::to_owned),
    }
}

struct Fake {
    resolve: Vec<Page>,
    cover: Option<CoverSet>,
    nexts: Mutex<HashMap<String, Page>>,
    fail_resolve: bool,
    fail_next: bool,
}

impl Fake {
    fn new(resolve: Vec<Page>) -> Self {
        Self {
            resolve,
            cover: None,
            nexts: Mutex::new(HashMap::new()),
            fail_resolve: false,
            fail_next: false,
        }
    }

    fn with_cover(mut self, cover: CoverSet) -> Self {
        self.cover = Some(cover);
        self
    }

    fn with_next(mut self, url: &str, page: Page) -> Self {
        self.nexts
            .get_mut()
            .expect("lock")
            .insert(url.to_owned(), page);
        self
    }
}

impl PageSource for Fake {
    async fn resolve(&self, _uri: &str) -> Result<Resolved, ContextError> {
        if self.fail_resolve {
            return Err(ContextError::Source("resolve failed".to_owned()));
        }
        Ok(Resolved {
            pages: self.resolve.clone(),
            cover: self.cover.clone(),
        })
    }

    async fn next(&self, url: &str) -> Result<Page, ContextError> {
        if self.fail_next {
            return Err(ContextError::Source("next failed".to_owned()));
        }
        self.nexts
            .lock()
            .expect("lock")
            .get(url)
            .cloned()
            .ok_or_else(|| ContextError::Source(format!("no page for {url}")))
    }
}

async fn drain(walker: &mut Walker<Fake>) -> Result<Vec<Page>, ContextError> {
    let mut pages = Vec::new();
    while let Some(page) = walker.next_page().await? {
        pages.push(page);
    }
    Ok(pages)
}

#[tokio::test]
async fn inline_pages_come_out_in_order() {
    let fake = Fake::new(vec![page(&["a"], None), page(&["b"], None)]);
    let mut walker = Walker::new(fake, "context");
    let pages = drain(&mut walker).await.expect("walk");
    assert_eq!(pages, vec![page(&["a"], None), page(&["b"], None)]);
}

#[tokio::test]
async fn follows_next_after_inline_pages() {
    let fake = Fake::new(vec![page(&["a"], Some("u1"))]).with_next("u1", page(&["b"], None));
    let mut walker = Walker::new(fake, "context");
    let pages = drain(&mut walker).await.expect("walk");
    assert_eq!(pages, vec![page(&["a"], Some("u1")), page(&["b"], None)]);
}

#[tokio::test]
async fn multiple_inline_pages_each_with_next_are_all_followed() {
    let fake = Fake::new(vec![page(&["a"], Some("u1")), page(&["b"], Some("u2"))])
        .with_next("u1", page(&["c"], None))
        .with_next("u2", page(&["d"], None));
    let mut walker = Walker::new(fake, "context");
    let pages = drain(&mut walker).await.expect("walk");
    assert_eq!(
        pages,
        vec![
            page(&["a"], Some("u1")),
            page(&["b"], Some("u2")),
            page(&["c"], None),
            page(&["d"], None),
        ]
    );
}

#[tokio::test]
async fn resolve_uris_walks_every_page_in_order() {
    let fake = Fake::new(vec![page(&["a", "b"], Some("u1")), page(&["c"], None)])
        .with_next("u1", page(&["d"], None));
    assert_eq!(
        resolve_uris(fake, "context").await.expect("walk"),
        vec!["a", "b", "c", "d"]
    );
}

#[tokio::test]
async fn resolve_uris_keeps_duplicates() {
    let fake = Fake::new(vec![page(&["a", "a"], Some("u1"))]).with_next("u1", page(&["a"], None));
    assert_eq!(
        resolve_uris(fake, "context").await.expect("walk"),
        vec!["a", "a", "a"],
        "a row repeated in the context must resolve at every row"
    );
}

#[tokio::test]
async fn first_pages_reads_a_page_and_its_next() {
    let fake = Fake::new(vec![page(&["a", "b"], Some("u1"))]).with_next("u1", page(&["c"], None));
    assert_eq!(
        first_pages(fake, "station", 2).await.expect("read"),
        page(&["a", "b", "c"], None).tracks
    );
}

#[tokio::test]
async fn first_pages_keeps_the_rows_it_read_when_the_next_page_fails() {
    let mut fake = Fake::new(vec![page(&["a"], Some("u1"))]);
    fake.fail_next = true;
    assert_eq!(
        first_pages(fake, "station", 2)
            .await
            .expect("the first page is a shorter read, not a failed one"),
        page(&["a"], None).tracks
    );
}

#[tokio::test]
async fn first_pages_propagates_a_resolve_error() {
    let mut fake = Fake::new(vec![page(&["a"], None)]);
    fake.fail_resolve = true;
    let error = first_pages(fake, "station", 2)
        .await
        .expect_err("nothing was read at all");
    assert!(matches!(error, ContextError::Source(_)));
}

#[tokio::test]
async fn first_pages_never_fetches_a_next_off_the_last_page_it_reads() {
    let fake = Fake::new(vec![page(&["a"], Some("u1"))])
        .with_next("u1", page(&["b"], Some("u2")))
        .with_next("u2", page(&["c"], None));
    assert_eq!(
        first_pages(fake, "station", 2).await.expect("read"),
        page(&["a", "b"], None).tracks,
        "the limit is reached, so u2 must not be fetched"
    );
}

#[tokio::test]
async fn first_pages_of_one_page_fetches_no_next() {
    // `u1` is unregistered, so a fetch would be an error rather than a page.
    let fake = Fake::new(vec![page(&["a"], Some("u1"))]);
    assert_eq!(
        first_pages(fake, "station", 1).await.expect("one page"),
        page(&["a"], None).tracks
    );
}

#[tokio::test]
async fn first_pages_counts_inline_pages_toward_the_limit() {
    // Two inline pages fill a limit of two, so the next is never fetched:
    // `u1` is unregistered, and a fetch would be an error.
    let fake = Fake::new(vec![page(&["a"], None), page(&["b"], Some("u1"))]);
    assert_eq!(
        first_pages(fake, "station", 2).await.expect("two pages"),
        page(&["a", "b"], None).tracks
    );
}

#[tokio::test]
async fn resolve_uris_propagates_a_cycle() {
    let fake = Fake::new(vec![page(&["a"], Some("u1")), page(&["b"], Some("u1"))]);
    let error = resolve_uris(fake, "context")
        .await
        .expect_err("a repeated next is a cycle");
    assert!(matches!(error, ContextError::Cycle { url } if url == "u1"));
}

#[tokio::test]
async fn inline_pages_sharing_a_next_url_is_a_cycle() {
    let fake = Fake::new(vec![page(&["a"], Some("u1")), page(&["b"], Some("u1"))]);
    let mut walker = Walker::new(fake, "context");
    walker.next_page().await.expect("first page");
    let error = walker.next_page().await.expect_err("cycle");
    assert!(matches!(error, ContextError::Cycle { url } if url == "u1"));
}

#[tokio::test]
async fn stops_when_next_is_absent() {
    let fake = Fake::new(vec![page(&[], None)]);
    let mut walker = Walker::new(fake, "context");
    assert_eq!(
        walker.next_page().await.expect("page"),
        Some(page(&[], None))
    );
    assert_eq!(walker.next_page().await.expect("end"), None);
}

#[tokio::test]
async fn repeated_next_url_is_a_cycle() {
    let fake = Fake::new(vec![page(&["a"], Some("u1"))]).with_next("u1", page(&["b"], Some("u1")));
    let mut walker = Walker::new(fake, "context");
    walker.next_page().await.expect("inline");
    let error = walker.next_page().await.expect_err("cycle");
    assert!(matches!(error, ContextError::Cycle { url } if url == "u1"));
}

#[tokio::test]
async fn page_cap_trips() {
    let resolve = (0..=MAX_PAGES)
        .map(|_| page(&["a"], None))
        .collect::<Vec<_>>();
    let fake = Fake::new(resolve);
    let mut walker = Walker::new(fake, "context");
    let error = drain(&mut walker).await.expect_err("cap");
    assert!(matches!(error, ContextError::TooManyPages));
}

#[tokio::test]
async fn fetched_chain_page_cap_trips() {
    let mut fake = Fake::new(vec![page(&["a"], Some("u0"))]);
    for index in 0..=MAX_PAGES {
        let next = format!("u{}", index + 1);
        fake = fake.with_next(&format!("u{index}"), page(&["a"], Some(next.as_str())));
    }
    let mut walker = Walker::new(fake, "context");
    let error = drain(&mut walker).await.expect_err("cap");
    assert!(matches!(error, ContextError::TooManyPages));
}

#[tokio::test]
async fn empty_context_yields_none() {
    let fake = Fake::new(Vec::new());
    let mut walker = Walker::new(fake, "context");
    assert_eq!(walker.next_page().await.expect("empty"), None);
}

#[tokio::test]
async fn source_error_propagates() {
    let mut fake = Fake::new(Vec::new());
    fake.fail_resolve = true;
    let mut walker = Walker::new(fake, "context");
    let error = walker.next_page().await.expect_err("source");
    assert!(matches!(error, ContextError::Source(_)));

    let mut fake = Fake::new(vec![page(&["a"], Some("u1"))]);
    fake.fail_next = true;
    let mut walker = Walker::new(fake, "context");
    walker.next_page().await.expect("inline");
    let error = walker.next_page().await.expect_err("source");
    assert!(matches!(error, ContextError::Source(_)));
}

#[tokio::test]
async fn the_cover_is_none_before_the_first_resolve() {
    let fake = Fake::new(vec![page(&["a"], None)]).with_cover(cover());
    let walker = Walker::new(fake, "context");
    assert_eq!(walker.cover(), None);
}

#[tokio::test]
async fn the_cover_equals_the_sources_after_the_first_resolve() {
    let fake = Fake::new(vec![page(&["a"], None)]).with_cover(cover());
    let mut walker = Walker::new(fake, "context");
    walker.next_page().await.expect("page");
    assert_eq!(walker.cover(), Some(cover()));
}

#[tokio::test]
async fn the_cover_survives_an_empty_context() {
    let fake = Fake::new(Vec::new()).with_cover(cover());
    let mut walker = Walker::new(fake, "context");
    assert_eq!(walker.next_page().await.expect("empty"), None);
    assert_eq!(walker.cover(), Some(cover()));
}

#[tokio::test]
async fn a_source_error_leaves_the_cover_none() {
    let mut fake = Fake::new(Vec::new()).with_cover(cover());
    fake.fail_resolve = true;
    let mut walker = Walker::new(fake, "context");
    let error = walker.next_page().await.expect_err("source");
    assert!(matches!(error, ContextError::Source(_)));
    assert_eq!(walker.cover(), None);
}

#[test]
fn non_playable_items_are_dropped() {
    let context_page = ContextPage {
        tracks: vec![track(TRACK_A), track("spotify:album:abc"), track(TRACK_B)],
        ..Default::default()
    };
    assert_eq!(
        page_from_proto(&context_page).tracks,
        page(&[TRACK_A, TRACK_B], None).tracks
    );
}

#[test]
fn page_from_proto_reads_added_at_per_item() {
    let context_page = ContextPage {
        tracks: vec![
            track_with_meta(TRACK_A, Some("1792357200000")),
            track_with_meta(TRACK_B, Some("1700000000000")),
            track(TRACK_A),
        ],
        ..Default::default()
    };
    let dates: Vec<Option<u64>> = page_from_proto(&context_page)
        .tracks
        .iter()
        .map(|track| track.added_at.map(AddedAt::as_millis))
        .collect();
    assert_eq!(
        dates,
        vec![Some(1_792_357_200_000), Some(1_700_000_000_000), None]
    );
}

#[test]
fn a_junk_added_at_keeps_the_item_with_no_date() {
    let context_page = ContextPage {
        tracks: vec![
            track_with_meta(TRACK_A, Some("soon")),
            track_with_meta(TRACK_B, Some("")),
            track_with_meta(TRACK_A, Some("1792357200")),
        ],
        ..Default::default()
    };
    let page = page_from_proto(&context_page);
    assert_eq!(page.tracks.len(), 3, "a bad date must not drop the row");
    assert!(page.tracks.iter().all(|track| track.added_at.is_none()));
}

#[test]
fn a_dropped_item_does_not_shift_the_dates_of_the_ones_kept() {
    let context_page = ContextPage {
        tracks: vec![
            track_with_meta(TRACK_A, Some("1700000000000")),
            track_with_meta("spotify:album:abc", Some("1800000000000")),
            track_with_meta(TRACK_B, Some("1792357200000")),
        ],
        ..Default::default()
    };
    assert_eq!(
        page_from_proto(&context_page).tracks,
        vec![
            PageTrack {
                uri: TRACK_A.to_owned(),
                added_at: Some(date("1700000000000")),
            },
            PageTrack {
                uri: TRACK_B.to_owned(),
                added_at: Some(date("1792357200000")),
            },
        ]
    );
}

#[test]
fn a_local_item_keeps_its_added_at() {
    let local = "spotify:local:Artist:Album:Title:222";
    let context_page = ContextPage {
        tracks: vec![track_with_meta(local, Some("1792357200000"))],
        ..Default::default()
    };
    assert_eq!(
        page_from_proto(&context_page).tracks,
        vec![PageTrack {
            uri: local.to_owned(),
            added_at: Some(date("1792357200000")),
        }]
    );
}

#[test]
fn parse_page_json_carries_added_at() {
    let snake = format!(
        r#"{{"tracks":[{{"uri":"{TRACK_A}","metadata":{{"added_at":"1792357200000"}}}}]}}"#
    );
    let parsed = parse_page_json(snake.as_bytes()).expect("snake");
    assert_eq!(
        parsed.tracks,
        vec![PageTrack {
            uri: TRACK_A.to_owned(),
            added_at: Some(date("1792357200000")),
        }]
    );

    let camel = format!(
        r#"{{"nextPageUrl":"u1","tracks":[{{"uri":"{TRACK_A}","metadata":{{"added_at":"1792357200000"}}}}]}}"#
    );
    let parsed = parse_page_json(camel.as_bytes()).expect("camel");
    assert_eq!(parsed.next.as_deref(), Some("u1"));
    assert_eq!(parsed.tracks[0].added_at, Some(date("1792357200000")));
}

#[test]
fn empty_next_url_is_none() {
    let context_page = ContextPage {
        next_page_url: Some(String::new()),
        ..Default::default()
    };
    assert_eq!(page_from_proto(&context_page).next, None);
}

#[test]
fn parses_snake_and_camel_json_names() {
    let snake = format!(r#"{{"next_page_url":"u1","tracks":[{{"uri":"{TRACK_A}"}}]}}"#);
    let parsed = parse_page_json(snake.as_bytes()).expect("snake");
    assert_eq!(parsed, page(&[TRACK_A], Some("u1")));

    let camel = format!(r#"{{"nextPageUrl":"u1","tracks":[{{"uri":"{TRACK_A}"}}]}}"#);
    let parsed = parse_page_json(camel.as_bytes()).expect("camel");
    assert_eq!(parsed, page(&[TRACK_A], Some("u1")));
}

#[test]
fn empty_well_shaped_page_parses() {
    let parsed = parse_page_json(br#"{"tracks":[]}"#).expect("empty tracks");
    assert_eq!(parsed, page(&[], None));
}

#[test]
fn context_shaped_body_is_rejected() {
    let error = parse_page_json(br#"{"pages":[]}"#).expect_err("wrong shape");
    assert!(matches!(error, ContextError::Parse(_)));
    assert!(!error.to_string().contains("pages"));
}

#[test]
fn object_with_no_known_fields_is_rejected() {
    let error = parse_page_json(b"{}").expect_err("no fields");
    assert!(matches!(error, ContextError::Parse(_)));
    assert!(!error.to_string().contains("{}"));
}

#[test]
fn unknown_json_fields_are_ignored() {
    let json = format!(
        r#"{{"next_page_url":"u1","something_else":true,"tracks":[{{"uri":"{TRACK_A}"}}]}}"#
    );
    let parsed = parse_page_json(json.as_bytes()).expect("unknown ignored");
    assert_eq!(parsed, page(&[TRACK_A], Some("u1")));
}

fn track(uri: &str) -> ContextTrack {
    ContextTrack {
        uri: Some(uri.to_owned()),
        ..Default::default()
    }
}

fn track_with_meta(uri: &str, added_at: Option<&str>) -> ContextTrack {
    let mut metadata = HashMap::new();
    if let Some(added_at) = added_at {
        metadata.insert("added_at".to_owned(), added_at.to_owned());
    }
    ContextTrack {
        uri: Some(uri.to_owned()),
        metadata,
        ..Default::default()
    }
}
