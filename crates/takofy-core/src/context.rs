//! Context page parsing and the queue-based walker.
//!
//! A resolved context may hold several pages, and any page may carry its own
//! `next`, so the walker queues work instead of following one linear chain.

use std::collections::{HashSet, VecDeque};

use librespot::core::SpotifyUri;
use librespot::protocol::context_page::ContextPage;
use protobuf_json_mapping::ParseOptions;
use takofy_model::{AddedAt, CoverSet};

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
    pub cover: Option<CoverSet>,
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

/// The positions of a context's first `limit` pages, in the walker's order.
///
/// A short read is a shorter context rather than a failed one: a page that
/// fails after the first is logged and ends the walk, keeping the rows already
/// read. Only a failure on the first page leaves nothing to return.
pub(crate) async fn first_pages<S: PageSource>(
    source: S,
    uri: &str,
    limit: usize,
) -> Result<Vec<PageTrack>, ContextError> {
    let mut walker = Walker::new(source, uri);
    let mut positions = Vec::new();
    for page in 0..limit {
        match walker.next_page().await {
            Ok(Some(read)) => positions.extend(read.tracks),
            Ok(None) => break,
            Err(error) if page == 0 => return Err(error),
            Err(error) => {
                log::warn!("context page {page} failed: {error}");
                break;
            }
        }
    }
    Ok(positions)
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
    cover: Option<CoverSet>,
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
    pub(crate) fn cover(&self) -> Option<CoverSet> {
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
mod tests;
