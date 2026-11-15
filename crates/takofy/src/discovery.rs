//! The outside lookup's seam and the state every request shares.
//!
//! The lookup reaches the outside service only through [`Fetch`], so it runs
//! against a fake with no socket. The gate is read before any other work, the
//! limiter is locked only to take a decision and never across an `await`, and a
//! genre table that failed to load is retried rather than cached.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::OnceCell;
use tokio::time::Instant;

pub mod genre;
pub mod http;
pub mod limiter;
pub mod lookup;
pub mod verify;
pub mod wire;

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests;

// Worst case per row: 14 outside calls (1 identify, 3 verify, 1 related, 8
// neighbour tops, 1 albums) and about 25 Spotify calls, so three rows stay
// under the limiter's 40 calls per five seconds. The genre table adds one
// outside call once per process.

/// How long one lookup may run before the bridge cancels it.
pub(crate) const LOOKUP_DEADLINE: Duration = Duration::from_secs(30);
/// The longest one request may wait on the limiter or a backoff before the
/// lookup answers unreachable instead of sleeping.
pub(crate) const MAX_WAIT: Duration = Duration::from_secs(10);
/// One request's own budget, below the lookup's so a slow body fails alone.
pub(crate) const HTTP_TIMEOUT: Duration = Duration::from_secs(8);
/// The longest a failure may pause the outside, whatever it asks for.
pub(crate) const BACKOFF_CEILING: Duration = Duration::from_secs(30);
/// The most bytes one response may carry.
pub(crate) const MAX_BODY: usize = 1024 * 1024;
/// The most lookups three rows may run at once.
pub(crate) const MAX_CONCURRENT_LOOKUPS: usize = 3;

const _: () = assert!(
    MAX_WAIT.as_millis() < LOOKUP_DEADLINE.as_millis(),
    "a wait must leave time for the request it waits for"
);
const _: () = assert!(
    HTTP_TIMEOUT.as_millis() < LOOKUP_DEADLINE.as_millis(),
    "one request must fit inside the lookup it belongs to"
);

/// Why a fetch did not answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchError {
    /// A non-success status, with the `Retry-After` the response named.
    Status(u16, Option<Duration>),
    /// The request could not be made or its body read.
    Transport,
    /// The body exceeded [`MAX_BODY`].
    TooLarge,
}

/// One request to the outside service.
pub trait Fetch {
    fn get(&self, path_and_query: &str)
    -> impl Future<Output = Result<Vec<u8>, FetchError>> + Send;
}

/// Why a lookup's request did not answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutsideError {
    /// The setting is off, so no request was made.
    Off,
    /// The service asked for a pause, so this lookup stops rather than retrying.
    Backoff,
    /// The request failed without asking for a pause, so one may be dropped.
    Failed,
    /// The service answered, and holds no such record.
    Absent,
}

/// The gate, the pacing and the genre table one lookup reaches the outside
/// through.
pub(crate) struct Outside<F: Fetch> {
    gate: Arc<AtomicBool>,
    limiter: Mutex<limiter::Limiter>,
    genres: OnceCell<wire::GenreTable>,
    fetcher: F,
}

impl<F: Fetch + Sync> Outside<F> {
    pub(crate) fn new(fetcher: F, gate: Arc<AtomicBool>) -> Self {
        Self {
            gate,
            limiter: Mutex::new(limiter::Limiter::new()),
            genres: OnceCell::new(),
            fetcher,
        }
    }

    pub(crate) fn is_open(&self) -> bool {
        self.gate.load(Ordering::Relaxed)
    }

    fn decide(&self, now: Instant) -> limiter::Decision {
        match self.limiter.lock() {
            Ok(mut limiter) => limiter.admit(now),
            Err(_) => limiter::Decision::Refuse,
        }
    }

    fn record(&self, result: &Result<Vec<u8>, FetchError>) {
        let Ok(mut limiter) = self.limiter.lock() else {
            return;
        };
        match result {
            Ok(_) => limiter.ok(),
            Err(FetchError::Status(429, retry_after)) => {
                limiter.backoff(Instant::now(), *retry_after);
            }
            _ => {}
        }
    }

    /// Checks the gate, waits for the limiter, then fetches.
    ///
    /// An error body answers HTTP 200, so it is read here: the quota pauses the
    /// outside, and any other error code is a record the service does not hold.
    /// Neither reaches the caller as an empty list.
    pub(crate) async fn get(&self, path_and_query: &str) -> Result<Vec<u8>, OutsideError> {
        loop {
            if !self.is_open() {
                return Err(OutsideError::Off);
            }
            match self.decide(Instant::now()) {
                limiter::Decision::Go => break,
                limiter::Decision::Wait(wait) => tokio::time::sleep(wait).await,
                limiter::Decision::Refuse => return Err(OutsideError::Backoff),
            }
        }
        if !self.is_open() {
            return Err(OutsideError::Off);
        }
        let result = self.fetcher.get(path_and_query).await;
        if let Ok(body) = &result
            && let Some(error) = wire::body_error(body)
        {
            if error == wire::BodyError::Quota {
                if let Ok(mut limiter) = self.limiter.lock() {
                    limiter.backoff(Instant::now(), None);
                }
                return Err(OutsideError::Backoff);
            }
            return Err(OutsideError::Absent);
        }
        self.record(&result);
        match result {
            Ok(body) => Ok(body),
            Err(FetchError::Status(429, _)) => Err(OutsideError::Backoff),
            Err(_) => Err(OutsideError::Failed),
        }
    }

    /// The genre table, attempted again after a failed load.
    pub(crate) async fn genres(&self) -> Result<wire::GenreTable, OutsideError> {
        self.genres
            .get_or_try_init(|| async {
                let body = self.get("/genre").await?;
                wire::genre_table(&body).map_err(|_| OutsideError::Failed)
            })
            .await
            .cloned()
    }
}
