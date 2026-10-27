//! A bounded in-memory cache for per-URI labels.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Mutex, PoisonError};

/// How many URIs one session keeps named.
const LABEL_CAP: usize = 20_000;

/// One value per URI, held for the life of the session that owns it.
///
/// No clock: the cache is empty when a session starts and gone when it ends,
/// and the cap plus the eviction order is the whole bound.
pub(crate) struct UriCache<V> {
    cap: usize,
    state: Mutex<State<V>>,
}

struct State<V> {
    values: HashMap<String, V>,
    /// Every cached key, oldest first.
    inserted: VecDeque<String>,
}

impl<V> UriCache<V> {
    pub(crate) fn new() -> Self {
        Self::with_cap(LABEL_CAP)
    }

    fn with_cap(cap: usize) -> Self {
        Self {
            cap,
            state: Mutex::new(State {
                values: HashMap::new(),
                inserted: VecDeque::new(),
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values
            .len()
    }
}

impl<V: Clone> UriCache<V> {
    /// The cached values for `uris`; a hit stays cached.
    pub(crate) fn take(&self, uris: &[String]) -> HashMap<String, V> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        uris.iter()
            .filter_map(|uri| {
                state
                    .values
                    .get(uri)
                    .map(|value| (uri.clone(), value.clone()))
            })
            .collect()
    }

    /// Caches `values`, dropping the oldest keys above the cap.
    ///
    /// Overwriting a key keeps its place in the eviction order, so a re-read of
    /// the same URI cannot grow the map. Only the order of the calls decides
    /// which key is oldest: within one call the input's order is a map's.
    pub(crate) fn store(&self, values: &HashMap<String, V>) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        for (uri, value) in values {
            if state.values.insert(uri.clone(), value.clone()).is_none() {
                state.inserted.push_back(uri.clone());
            }
        }
        while state.values.len() > self.cap {
            let Some(oldest) = state.inserted.pop_front() else {
                break;
            };
            state.values.remove(&oldest);
        }
    }

    /// The values for `uris`, from the cache plus one fetch of whatever it lacks.
    ///
    /// Hits are copied out before the fetch and the fetched map is merged back,
    /// so an answer larger than the cap still names every URI it was asked for.
    /// No lock is held across the fetch.
    pub(crate) async fn through_cache<F>(&self, uris: Vec<String>, fetch: F) -> HashMap<String, V>
    where
        F: AsyncFnOnce(Vec<String>) -> HashMap<String, V>,
    {
        let mut found = self.take(&uris);
        let mut missing = Vec::new();
        let mut seen = HashSet::new();
        for uri in uris {
            if !found.contains_key(&uri) && seen.insert(uri.clone()) {
                missing.push(uri);
            }
        }
        if missing.is_empty() {
            return found;
        }
        let fetched = fetch(missing).await;
        self.store(&fetched);
        found.extend(fetched);
        found
    }
}

#[cfg(test)]
mod tests;
