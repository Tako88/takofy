//! Context page parsing and the queue-based walker.
//!
//! A resolved context may hold several pages, and any page may carry its own
//! `next`, so the walker queues work instead of following one linear chain.

use std::collections::{HashSet, VecDeque};

use librespot::core::SpotifyUri;
use librespot::protocol::context_page::ContextPage;
use protobuf_json_mapping::ParseOptions;
use takofy_model::{AddedAt, CoverUrl};

/// One position in a page: the URI the row draws and, for a playlist, when the
/// track joined it.
///
/// The date belongs to the position rather than to the track, so two rows of the
/// same track can carry two dates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageTrack {
    pub uri: String,
    pub added_at: Option<AddedAt>,
}

/// One page of a context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// The playable positions kept from this page.
    pub tracks: Vec<PageTrack>,
    /// The next page URL, when present and non-empty.
    pub next: Option<String>,
}

/// Why a context read failed.
#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    /// The source itself failed; the message is the source's.
    #[error("{0}")]
    Source(String),
    /// The body could not be decoded; the message names the parser's complaint.
    #[error("could not parse a context page: {0}")]
    Parse(String),
    /// A `next` URL was discovered twice.
    #[error("context page cycle at {url}")]
    Cycle { url: String },
    /// More than [`MAX_PAGES`] pages were returned.
    #[error("context has more than {MAX_PAGES} pages")]
    TooManyPages,
    /// The read did not answer inside the budget it was given.
    #[error("context read timed out")]
    TimedOut,
}

/// A resolved context: its pages and the image the context itself carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resolved {
    /// The inline pages, in order.
    pub pages: Vec<Page>,
    /// The context's own image, when the response carried a usable URL.
    pub cover: Option<CoverUrl>,
}

/// Reads pages of a context. Implemented over librespot in `remote`.
pub(crate) trait PageSource {
    async fn resolve(&self, uri: &str) -> Result<Resolved, ContextError>;
    async fn next(&self, url: &str) -> Result<Page, ContextError>;
}

/// Hard cap on pages, so a source that keeps serving `next` cannot loop forever.
const MAX_PAGES: usize = 500;

/// Keeps only playable positions, with their membership dates, and drops an
/// empty `next`.
pub fn page_from_proto(page: &ContextPage) -> Page {
    let tracks = page
        .tracks
        .iter()
        .filter_map(|track| {
            let uri = track.uri.as_deref()?;
            match SpotifyUri::from_uri(uri) {
                Ok(parsed) if parsed.is_playable() => Some(PageTrack {
                    uri: uri.to_owned(),
                    added_at: track
                        .metadata
                        .get("added_at")
                        .and_then(|wire| AddedAt::from_wire(wire)),
                }),
                _ => None,
            }
        })
        .collect();
    let next = page
        .next_page_url
        .as_deref()
        .filter(|url| !url.is_empty())
        .map(str::to_owned);
    Page { tracks, next }
}

/// Decodes a JSON context page. The parser error carries no JSON body.
pub fn parse_page_json(bytes: &[u8]) -> Result<Page, ContextError> {
    let json = std::str::from_utf8(bytes)
        .map_err(|_| ContextError::Parse("body is not utf-8".to_owned()))?;
    let options = ParseOptions {
        ignore_unknown_fields: true,
        ..Default::default()
    };
    let page = protobuf_json_mapping::parse_from_str_with_options::<ContextPage>(json, &options)
        .map_err(|error| ContextError::Parse(error.to_string()))?;
    let page = page_from_proto(&page);
    if page.tracks.is_empty()
        && page.next.is_none()
        && ![
            "tracks",
            "next_page_url",
            "nextPageUrl",
            "page_url",
            "pageUrl",
        ]
        .iter()
        .any(|field| json.contains(field))
    {
        return Err(ContextError::Parse(
            "missing 'tracks' or 'next_page_url' in context page".to_owned(),
        ));
    }
    Ok(page)
}

/// Every track URI a context holds, in the order the walker produces them.
///
/// Duplicates are kept: a playlist that lists a track twice must stay
/// resolvable at both rows, so this is a concatenation and not a set.
pub(crate) async fn resolve_uris<S: PageSource>(
    source: S,
    uri: &str,
) -> Result<Vec<String>, ContextError> {
    let mut walker = Walker::new(source, uri);
    let mut uris = Vec::new();
    while let Some(page) = walker.next_page().await? {
        uris.extend(page.tracks.into_iter().map(|track| track.uri));
    }
    Ok(uris)
}

/// Walks a context: all inline pages first, then pending URLs in discovery order.
pub(crate) struct Walker<S> {
    source: S,
    uri: String,
    resolved: bool,
    ready: VecDeque<Page>,
    pending: VecDeque<String>,
    seen: HashSet<String>,
    count: usize,
    cover: Option<CoverUrl>,
}

impl<S: PageSource> Walker<S> {
    pub(crate) fn new(source: S, uri: impl Into<String>) -> Self {
        Self {
            source,
            uri: uri.into(),
            resolved: false,
            ready: VecDeque::new(),
            pending: VecDeque::new(),
            seen: HashSet::new(),
            count: 0,
            cover: None,
        }
    }

    /// The context's own image, once it has been resolved.
    pub(crate) fn cover(&self) -> Option<CoverUrl> {
        self.cover.clone()
    }

    pub(crate) async fn next_page(&mut self) -> Result<Option<Page>, ContextError> {
        if !self.resolved {
            self.resolved = true;
            let resolved = self.source.resolve(&self.uri).await?;
            self.cover = resolved.cover;
            self.ready.extend(resolved.pages);
        }
        loop {
            if let Some(page) = self.ready.pop_front() {
                self.count += 1;
                if self.count > MAX_PAGES {
                    return Err(ContextError::TooManyPages);
                }
                if let Some(url) = &page.next {
                    if !self.seen.insert(url.clone()) {
                        return Err(ContextError::Cycle { url: url.clone() });
                    }
                    self.pending.push_back(url.clone());
                }
                return Ok(Some(page));
            }
            match self.pending.pop_front() {
                Some(url) => self.ready.push_back(self.source.next(&url).await?),
                None => return Ok(None),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use librespot::protocol::context_page::ContextPage;
    use librespot::protocol::context_track::ContextTrack;

    use super::{
        ContextError, MAX_PAGES, Page, PageSource, PageTrack, Resolved, Walker, page_from_proto,
        parse_page_json, resolve_uris,
    };
    use takofy_model::{AddedAt, CoverUrl};

    const TRACK_A: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQC";
    const TRACK_B: &str = "spotify:track:6rqhFgbbKwnb9MLmUQDhG6";

    fn cover() -> CoverUrl {
        CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url")
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
        cover: Option<CoverUrl>,
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

        fn with_cover(mut self, cover: CoverUrl) -> Self {
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
        let fake =
            Fake::new(vec![page(&["a", "a"], Some("u1"))]).with_next("u1", page(&["a"], None));
        assert_eq!(
            resolve_uris(fake, "context").await.expect("walk"),
            vec!["a", "a", "a"],
            "a row repeated in the context must resolve at every row"
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
        let fake =
            Fake::new(vec![page(&["a"], Some("u1"))]).with_next("u1", page(&["b"], Some("u1")));
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
}
