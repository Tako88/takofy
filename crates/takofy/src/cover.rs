//! Fetching and decoding cover art off the drawing thread.
//!
//! The fetch is a plain HTTPS GET of a URL the model already allowlisted:
//! there is no session and no credential, and the request carries a timeout so
//! it is cancellable. Decode runs on a detached `std::thread`, so a slow or
//! hung decode cannot hold up the runtime drop. The drawing thread never sees
//! bytes, only a decoded [`takofy_model::CoverImage`].

use std::collections::HashSet;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use takofy_model::{CoverImage, CoverUrl};
use takofy_ui::message::Update;
use tokio::sync::{Semaphore, oneshot};
use tokio::task::JoinSet;

use crate::bridge::Notifier;

/// How long one fetch may take before it is cancelled.
pub(crate) const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
/// How long the worker waits for a detached decode before giving up on it.
pub(crate) const DECODE_TIMEOUT: Duration = Duration::from_secs(5);
/// The most bytes a cover body may have.
pub(crate) const MAX_COVER_BYTES: usize = 2 * 1024 * 1024;
/// The most cover fetches that may run at once.
pub(crate) const MAX_CONCURRENT_COVERS: usize = 6;

/// The window waits longer for a cover than the worker takes to fetch and
/// decode it, and both sides bound the same traffic with the same number.
///
/// Ties [`takofy_ui::cover::cache::IN_FLIGHT_TIMEOUT`] to [`FETCH_TIMEOUT`] plus
/// [`DECODE_TIMEOUT`], and [`takofy_ui::cover::MAX_IN_FLIGHT`] to
/// [`MAX_CONCURRENT_COVERS`]. The build fails when the worker's fetch plus
/// decode would outlast the window's in-flight timeout — that is, when
/// [`FETCH_TIMEOUT`] or [`DECODE_TIMEOUT`] is raised in this file, or when
/// [`takofy_ui::cover::cache::IN_FLIGHT_TIMEOUT`] is lowered in
/// `takofy-ui/src/cover/cache.rs` — and on any change to either concurrency
/// limit that leaves the two unequal.
const _: () = assert!(
    takofy_ui::cover::cache::IN_FLIGHT_TIMEOUT.as_millis()
        > FETCH_TIMEOUT.as_millis() + DECODE_TIMEOUT.as_millis(),
    "the window's in-flight timeout must outlast the worker's fetch and decode: raise IN_FLIGHT_TIMEOUT in takofy-ui/src/cover/cache.rs, or lower FETCH_TIMEOUT or DECODE_TIMEOUT in this file"
);
const _: () = assert!(
    takofy_ui::cover::MAX_IN_FLIGHT == MAX_CONCURRENT_COVERS,
    "the window's in-flight cap and the worker's permits must agree: change MAX_IN_FLIGHT in takofy-ui/src/cover.rs, or MAX_CONCURRENT_COVERS in this file, so the two are equal"
);

/// Why a cover could not be produced. Kind only: no body, no URL, so nothing
/// here can name a user's library when it is logged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoverError {
    /// The response body was empty.
    Empty,
    /// The response body exceeded [`MAX_COVER_BYTES`].
    TooLarge,
    /// The response was not an image.
    NotImage,
    /// The request failed, timed out, or carried a non-success status.
    Fetch,
    /// The bytes were not a decodable JPEG within the limits.
    Decode,
}

impl CoverError {
    /// A fixed name for the log.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::TooLarge => "too large",
            Self::NotImage => "not an image",
            Self::Fetch => "request",
            Self::Decode => "decode",
        }
    }
}

/// Fetches the bytes of an allowlisted cover URL.
///
/// `client` carries the timeout and the bounded pool. The body is read in
/// chunks under [`MAX_COVER_BYTES`], never buffered whole first.
pub(crate) async fn fetch_cover(
    client: &reqwest::Client,
    url: &CoverUrl,
) -> Result<Vec<u8>, CoverError> {
    let response = client
        .get(url.as_str())
        .send()
        .await
        .map_err(|_| CoverError::Fetch)?;
    if !response.status().is_success() {
        return Err(CoverError::Fetch);
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let mut response = response;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| CoverError::Fetch)? {
        if body.len() + chunk.len() > MAX_COVER_BYTES {
            return Err(CoverError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    check_body(&body, content_type.as_deref())?;
    Ok(body)
}

/// Checks a fully read body. Pure: no I/O, no allocation.
pub(crate) fn check_body(bytes: &[u8], content_type: Option<&str>) -> Result<(), CoverError> {
    if bytes.is_empty() {
        return Err(CoverError::Empty);
    }
    if bytes.len() > MAX_COVER_BYTES {
        return Err(CoverError::TooLarge);
    }
    if let Some(content_type) = content_type
        && !content_type.starts_with("image/")
    {
        return Err(CoverError::NotImage);
    }
    Ok(())
}

/// Removes a URL from the in-flight set once its fetch is resolved, or when
/// the task ends before that: success, failure, or `JoinSet::abort_all` on
/// shutdown. Releasing before the reply lets the window retry at once.
struct InFlight {
    url: CoverUrl,
    set: Arc<Mutex<HashSet<CoverUrl>>>,
}

impl Drop for InFlight {
    fn drop(&mut self) {
        if let Ok(mut set) = self.set.lock() {
            set.remove(&self.url);
        }
    }
}

/// Spawns one fetch-and-decode onto `tasks`.
///
/// `fetch` is the seam: production passes [`fetch_cover`]'s future, a test
/// passes one that answers without a network. `decode` runs on a detached
/// thread. A URL already in flight is dropped rather than fetched twice, and
/// nothing completed is retained, so the worker has no unbounded cache.
pub(crate) fn spawn_cover<F, D>(
    tasks: &mut JoinSet<()>,
    permits: Arc<Semaphore>,
    in_flight: Arc<Mutex<HashSet<CoverUrl>>>,
    url: CoverUrl,
    fetch: F,
    decode: D,
    notifier: Notifier,
) where
    F: Future<Output = Result<Vec<u8>, CoverError>> + Send + 'static,
    D: FnOnce(Vec<u8>) -> Result<CoverImage, CoverError> + Send + 'static,
{
    spawn_cover_with_timeout(
        tasks,
        permits,
        in_flight,
        url,
        fetch,
        decode,
        notifier,
        DECODE_TIMEOUT,
    );
}

/// [`spawn_cover`] with an explicit decode budget, so a test can reach the
/// timeout path without waiting the production five seconds.
#[allow(
    clippy::too_many_arguments,
    reason = "mirrors spawn_cover plus the test-only budget"
)]
pub(crate) fn spawn_cover_with_timeout<F, D>(
    tasks: &mut JoinSet<()>,
    permits: Arc<Semaphore>,
    in_flight: Arc<Mutex<HashSet<CoverUrl>>>,
    url: CoverUrl,
    fetch: F,
    decode: D,
    notifier: Notifier,
    decode_timeout: Duration,
) where
    F: Future<Output = Result<Vec<u8>, CoverError>> + Send + 'static,
    D: FnOnce(Vec<u8>) -> Result<CoverImage, CoverError> + Send + 'static,
{
    match in_flight.lock() {
        Ok(mut set) => {
            if !set.insert(url.clone()) {
                return;
            }
        }
        Err(_) => return,
    }
    let guard = InFlight {
        url: url.clone(),
        set: Arc::clone(&in_flight),
    };
    tasks.spawn(async move {
        let guard = guard;
        // The permit is held across the fetch and the wait for the decode, so
        // a storm of requests cannot exceed the CDN budget. It is released
        // when the task ends, which can be before a timed-out decode thread
        // finishes.
        let _permit = match permits.acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => {
                drop(guard);
                notifier.send(Update::CoverFailed { url });
                return;
            }
        };
        log::debug!("cover fetch start {}", tail(&url));
        let bytes = match fetch.await {
            Ok(bytes) => bytes,
            Err(error) => {
                log::debug!("cover fetch failed {}", error.kind());
                // Drop the in-flight claim before the reply: a window that
                // retries on the same frame must not be deduped away.
                drop(guard);
                notifier.send(Update::CoverFailed { url });
                return;
            }
        };
        let (tx, rx) = oneshot::channel();
        std::thread::spawn(move || {
            let _ = tx.send(decode(bytes));
        });
        match tokio::time::timeout(decode_timeout, rx).await {
            Ok(Ok(Ok(image))) => {
                drop(guard);
                notifier.send(Update::Cover { url, image });
            }
            Ok(Ok(Err(error))) => {
                log::debug!("cover decode failed {}", error.kind());
                drop(guard);
                notifier.send(Update::CoverFailed { url });
            }
            // The decode thread panicked, or the decode outran its budget.
            Ok(Err(_)) | Err(_) => {
                drop(guard);
                notifier.send(Update::CoverFailed { url });
            }
        }
    });
}

/// The last few characters of a URL, enough to identify it in a log line
/// without printing the whole address.
fn tail(url: &CoverUrl) -> &str {
    let text = url.as_str();
    match text.char_indices().rev().nth(11) {
        Some((index, _)) => &text[index..],
        None => text,
    }
}

#[cfg(test)]
mod tests;
