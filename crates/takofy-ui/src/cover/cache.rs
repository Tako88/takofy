//! The pure cover cache: what is held, what is wanted, and what to fetch next.
//!
//! The cache is keyed by [`CoverUrl`] and bounded by both [`MAX_BYTES`] of held
//! pixels and [`MAX_HELD`] entries, evicting the least recently used first. An
//! entry wanted in the current or the previous frame is never evicted, so the
//! bounds are soft by at most the tiles drawn in two frames.
//!
//! Everything time-related is passed in as an [`Instant`]; the cache never reads
//! a clock, so its retry and timeout behaviour is pinned by advancing a base
//! instant rather than by sleeping.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use takofy_model::CoverUrl;

/// How much decoded cover data the cache holds, in RGBA bytes.
///
/// A 2× library grid is about twenty 640² tiles and the previous frame stays
/// pinned while scrolling, so the budget has to hold roughly forty of them.
pub const MAX_BYTES: usize = 96 * 1024 * 1024;

/// How many entries the cache holds, whatever their size.
///
/// The byte budget binds for every tile the wire actually offers; this cap
/// bounds a page of tiny ones.
pub const MAX_HELD: usize = 256;

/// How long a fetch may stay unanswered before the window calls it a failure.
///
/// Longer than the worker's fetch and decode timeouts together, so a real answer
/// always arrives first.
pub const IN_FLIGHT_TIMEOUT: Duration = Duration::from_secs(20);

/// The delay before each retry, indexed by the number of failures so far.
const RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(5), Duration::from_secs(30)];

/// The attempts a cover gets in total, the first try included.
const MAX_ATTEMPTS: u32 = 3;

/// What a [`Cache::look`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// A decoded image is held.
    Ready,
    /// A fetch is wanted or running, or the entry is waiting to retry.
    Loading,
    /// The cover could not be fetched, and either is waiting to retry or has
    /// exhausted its attempts.
    Failed,
}

/// What the cache knows about one URL.
#[derive(Debug, Clone, Copy)]
enum Entry {
    /// The decoded image is held, and its pixels cost `bytes`.
    Ready { bytes: usize },
    /// A fetch is wanted but has not been sent, because the in-flight cap was
    /// full when it was last looked at.
    Wanted { attempts: u32 },
    /// A fetch has been sent; `started` is when.
    InFlight { started: Instant, attempts: u32 },
    /// A fetch failed. `retry_at` is `None` once the attempts are exhausted.
    Failed {
        attempts: u32,
        retry_at: Option<Instant>,
    },
}

impl Entry {
    /// The bytes this entry holds; anything but a decoded image holds none.
    fn bytes(self) -> usize {
        match self {
            Entry::Ready { bytes } => bytes,
            _ => 0,
        }
    }
}

/// A bounded, least-recently-used set of covers with retry state.
#[derive(Debug, Default)]
pub struct Cache {
    entries: HashMap<CoverUrl, Entry>,
    /// The sum of the bytes held by the ready entries.
    bytes: usize,
    /// LRU order, least recently used at the front.
    order: VecDeque<CoverUrl>,
    /// The URLs looked at in the frame before this one.
    previous: HashSet<CoverUrl>,
    /// The URLs looked at in the current frame, in the order they were first
    /// looked at, which is the order the tiles draw in.
    current: Vec<CoverUrl>,
    /// Membership of [`Cache::current`], so a repeated look is not queued twice
    /// and eviction can test the frame's set cheaply.
    current_set: HashSet<CoverUrl>,
    /// URLs evicted since the last [`Cache::take_removed`].
    removed: Vec<CoverUrl>,
}

impl Cache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks the start of a frame: the current wanted set becomes the previous
    /// one, and the current one starts empty.
    pub fn begin_frame(&mut self) {
        self.previous = std::mem::take(&mut self.current_set);
        self.current.clear();
    }

    /// Records that `url` is wanted this frame and reports what to draw.
    pub fn look(&mut self, url: &CoverUrl, now: Instant) -> Look {
        if self.current_set.insert(url.clone()) {
            self.current.push(url.clone());
        }
        self.touch(url);
        let look = match self.entries.get(url).copied() {
            None => {
                self.entries
                    .insert(url.clone(), Entry::Wanted { attempts: 0 });
                Look::Loading
            }
            Some(Entry::Ready { .. }) => Look::Ready,
            Some(Entry::Wanted { .. }) => Look::Loading,
            Some(Entry::InFlight { .. }) => Look::Loading,
            Some(Entry::Failed { attempts, retry_at }) => match retry_at {
                Some(at) if at <= now => {
                    self.entries.insert(url.clone(), Entry::Wanted { attempts });
                    Look::Loading
                }
                _ => Look::Failed,
            },
        };
        self.evict();
        look
    }

    /// Records that a decoded image of `bytes` is held for `url`.
    pub fn insert(&mut self, url: &CoverUrl, bytes: usize) {
        let replaced = self.entries.insert(url.clone(), Entry::Ready { bytes });
        if let Some(old) = replaced {
            self.bytes -= old.bytes();
        }
        self.bytes += bytes;
        self.touch(url);
        self.evict();
    }

    /// Records that a fetch was sent for `url`, starting its deadline.
    pub fn mark_in_flight(&mut self, url: &CoverUrl, now: Instant) {
        let Some(entry) = self.entries.get(url).copied() else {
            return;
        };
        let attempts = match entry {
            Entry::Wanted { attempts }
            | Entry::InFlight { attempts, .. }
            | Entry::Failed { attempts, .. } => attempts,
            Entry::Ready { .. } => return,
        };
        self.entries.insert(
            url.clone(),
            Entry::InFlight {
                started: now,
                attempts,
            },
        );
    }

    /// Records that the fetch for `url` failed, scheduling the next attempt.
    pub fn mark_failed(&mut self, url: &CoverUrl, now: Instant) {
        let Some(entry) = self.entries.get(url).copied() else {
            return;
        };
        let attempts = match entry {
            Entry::Wanted { attempts }
            | Entry::InFlight { attempts, .. }
            | Entry::Failed { attempts, .. } => attempts.saturating_add(1),
            Entry::Ready { .. } => return,
        };
        let retry_at = if attempts >= MAX_ATTEMPTS {
            None
        } else {
            Some(now + RETRY_DELAYS[(attempts - 1) as usize])
        };
        self.entries
            .insert(url.clone(), Entry::Failed { attempts, retry_at });
    }

    /// The URLs wanted this frame that still need a fetch, in the order they
    /// were first looked at.
    ///
    /// That order is the draw order, so when more tiles are on screen than the
    /// cap allows, the top of the visible region is fetched first.
    pub fn wanted(&self) -> Vec<CoverUrl> {
        self.current
            .iter()
            .filter(|url| matches!(self.entries.get(*url), Some(Entry::Wanted { .. })))
            .cloned()
            .collect()
    }

    /// The URLs evicted since the last call, so their textures can be freed.
    pub fn take_removed(&mut self) -> Vec<CoverUrl> {
        std::mem::take(&mut self.removed)
    }

    /// The soonest instant the cache needs a frame.
    ///
    /// It is the earlier of a wanted entry's retry time and an in-flight entry's
    /// deadline. `None` when nothing wanted is pending, so an idle window is not
    /// asked to wake.
    pub fn next_wake(&self, now: Instant) -> Option<Duration> {
        let mut soonest: Option<Duration> = None;
        for url in &self.current {
            let deadline = match self.entries.get(url) {
                Some(Entry::Failed {
                    retry_at: Some(at), ..
                }) => Some(*at),
                Some(Entry::InFlight { started, .. }) => Some(*started + IN_FLIGHT_TIMEOUT),
                _ => None,
            };
            if let Some(at) = deadline {
                let delay = at.saturating_duration_since(now);
                soonest = Some(soonest.map_or(delay, |current| current.min(delay)));
            }
        }
        soonest
    }

    /// Moves `url` to the back of the LRU order, adding it when it is new.
    fn touch(&mut self, url: &CoverUrl) {
        if let Some(index) = self.order.iter().position(|held| held == url) {
            let _ = self.order.remove(index);
        }
        self.order.push_back(url.clone());
    }

    /// Drops the least recently used entries until both bounds hold.
    ///
    /// An entry in the current or the previous frame is pinned, so when every
    /// entry is pinned the bounds are exceeded rather than dropping a tile that
    /// is on screen.
    fn evict(&mut self) {
        while self.bytes > MAX_BYTES || self.entries.len() > MAX_HELD {
            let Some(index) = self.order.iter().position(|url| {
                !self.current_set.contains(url)
                    && !self.previous.contains(url)
                    // An entry with a fetch running is left alone, so eviction
                    // and the store's in-flight set can never disagree about a
                    // URL that still has an answer coming.
                    && !matches!(self.entries.get(url), Some(Entry::InFlight { .. }))
            }) else {
                break;
            };
            let Some(url) = self.order.remove(index) else {
                break;
            };
            if let Some(entry) = self.entries.remove(&url) {
                self.bytes -= entry.bytes();
            }
            self.removed.push(url);
        }
    }
}

#[cfg(test)]
mod tests;
