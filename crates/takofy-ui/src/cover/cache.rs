//! The pure cover cache: what is held, what is wanted, and what to fetch next.
//!
//! The cache is keyed by [`CoverUrl`] and holds at most [`MAX_HELD`] entries,
//! evicting the least recently used first. An entry wanted in the current or the
//! previous frame is never evicted, so the bound is soft by at most the tiles
//! drawn in two frames.
//!
//! Everything time-related is passed in as an [`Instant`]; the cache never reads
//! a clock, so its retry and timeout behaviour is pinned by advancing a base
//! instant rather than by sleeping.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use takofy_model::CoverUrl;

/// How many entries the cache holds. About 46 MB at 300×300×4.
pub const MAX_HELD: usize = 128;

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
    /// The decoded image is held.
    Ready,
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

/// A bounded, least-recently-used set of covers with retry state.
#[derive(Debug, Default)]
pub struct Cache {
    entries: HashMap<CoverUrl, Entry>,
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
            Some(Entry::Ready) => Look::Ready,
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

    /// Records that a decoded image is held for `url`.
    pub fn insert(&mut self, url: &CoverUrl) {
        self.entries.insert(url.clone(), Entry::Ready);
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
            Entry::Ready => return,
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
            Entry::Ready => return,
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

    /// Drops the least recently used entries until the bound holds.
    ///
    /// An entry in the current or the previous frame is pinned, so when every
    /// entry is pinned the bound is exceeded rather than dropping a tile that is
    /// on screen.
    fn evict(&mut self) {
        while self.entries.len() > MAX_HELD {
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
            self.entries.remove(&url);
            self.removed.push(url);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cover_url(tail: &str) -> CoverUrl {
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
    }

    /// A base instant the tests advance by a `Duration`; nothing sleeps.
    fn base() -> Instant {
        Instant::now()
    }

    #[test]
    fn a_new_entry_is_loading_and_ready_after_insert() {
        let now = base();
        let mut cache = Cache::new();
        assert_eq!(cache.look(&cover_url("a"), now), Look::Loading);
        cache.insert(&cover_url("a"));
        assert_eq!(cache.look(&cover_url("a"), now), Look::Ready);
    }

    #[test]
    fn distinct_urls_are_distinct_entries() {
        let now = base();
        let mut cache = Cache::new();
        cache.insert(&cover_url("a"));
        cache.insert(&cover_url("b"));
        assert_eq!(cache.look(&cover_url("a"), now), Look::Ready);
        assert_eq!(cache.look(&cover_url("b"), now), Look::Ready);
        assert_eq!(cache.entries.len(), 2);
    }

    #[test]
    fn eviction_takes_the_least_recently_used_first() {
        let now = base();
        let mut cache = Cache::new();
        for index in 0..MAX_HELD {
            cache.insert(&cover_url(&format!("u{index:03}")));
        }
        // Touch the oldest so it is no longer the least recently used.
        assert_eq!(cache.look(&cover_url("u000"), now), Look::Ready);
        cache.insert(&cover_url("overflow"));

        assert!(
            !cache.entries.contains_key(&cover_url("u001")),
            "u001 is evicted"
        );
        assert!(
            cache.entries.contains_key(&cover_url("u000")),
            "u000 was touched"
        );
        assert_eq!(cache.entries.len(), MAX_HELD);
    }

    /// Browsing is many distinct URLs arriving a screenful at a time, not one
    /// entry overflowing a full cache. The bound has to hold under that, and what
    /// goes must be the least recently used: the frames furthest back.
    #[test]
    fn browsing_holds_the_bound_and_evicts_the_oldest_frames() {
        const PER_FRAME: usize = 32;
        const FRAMES: usize = 10;

        let now = base();
        let mut cache = Cache::new();
        for frame in 0..FRAMES {
            cache.begin_frame();
            for slot in 0..PER_FRAME {
                // Looked at, then fetched: a tile that appears and resolves.
                let url = cover_url(&format!("f{frame:02}-t{slot:02}"));
                cache.look(&url, now);
                cache.insert(&url);
            }
        }

        assert_eq!(
            cache.entries.len(),
            MAX_HELD,
            "the cache grew past its bound while browsing"
        );

        // The newest frames are exactly what still fits; every older frame went.
        let held_frames = MAX_HELD / PER_FRAME;
        assert!(held_frames < FRAMES, "the test never passes the bound");
        for frame in 0..FRAMES {
            let held = frame >= FRAMES - held_frames;
            for slot in 0..PER_FRAME {
                let tail = format!("f{frame:02}-t{slot:02}");
                let url = cover_url(&tail);
                assert_eq!(
                    cache.entries.contains_key(&url),
                    held,
                    "{tail} should {} have been evicted",
                    if held { "not" } else { "" }
                );
            }
        }
    }

    #[test]
    fn a_wanted_entry_survives_a_full_cache() {
        let now = base();
        let mut cache = Cache::new();
        for index in 0..MAX_HELD {
            cache.insert(&cover_url(&format!("u{index:03}")));
        }
        assert_eq!(cache.look(&cover_url("u000"), now), Look::Ready);
        for index in 0..MAX_HELD {
            cache.insert(&cover_url(&format!("v{index:03}")));
        }
        assert_eq!(cache.look(&cover_url("u000"), now), Look::Ready);
    }

    #[test]
    fn an_entry_wanted_in_the_previous_frame_survives() {
        let now = base();
        let mut cache = Cache::new();
        for index in 0..MAX_HELD {
            cache.insert(&cover_url(&format!("u{index:03}")));
        }
        assert_eq!(cache.look(&cover_url("u000"), now), Look::Ready);
        // The next frame wants nothing: u000 moves from current to previous.
        cache.begin_frame();
        for index in 0..MAX_HELD {
            cache.insert(&cover_url(&format!("v{index:03}")));
        }
        assert!(cache.entries.contains_key(&cover_url("u000")));
    }

    #[test]
    fn a_failure_retries_at_five_then_thirty_seconds_and_then_stops() {
        let now = base();
        let mut cache = Cache::new();
        let url = cover_url("a");

        assert_eq!(cache.look(&url, now), Look::Loading);
        cache.mark_failed(&url, now);
        // Waiting for the first retry.
        assert_eq!(cache.look(&url, now), Look::Failed);
        assert_eq!(cache.look(&url, now + Duration::from_secs(4)), Look::Failed);
        // The retry is taken, and it fails again.
        assert_eq!(
            cache.look(&url, now + Duration::from_secs(5)),
            Look::Loading
        );
        cache.mark_failed(&url, now + Duration::from_secs(5));
        assert_eq!(cache.look(&url, now + Duration::from_secs(6)), Look::Failed);
        assert_eq!(
            cache.look(&url, now + Duration::from_secs(35)),
            Look::Loading
        );
        cache.mark_failed(&url, now + Duration::from_secs(35));
        // Three attempts are spent; the failure is permanent.
        assert_eq!(
            cache.look(&url, now + Duration::from_secs(1_000)),
            Look::Failed
        );
    }

    #[test]
    fn eviction_resets_a_failure() {
        let now = base();
        let mut cache = Cache::new();
        let url = cover_url("a");
        cache.look(&url, now);
        cache.mark_failed(&url, now);
        assert_eq!(cache.look(&url, now), Look::Failed);

        // Push the failed entry out by filling the cache without it.
        for index in 0..MAX_HELD {
            cache.begin_frame();
            cache.insert(&cover_url(&format!("u{index:03}")));
        }
        assert!(!cache.entries.contains_key(&url));
        assert_eq!(cache.look(&url, now), Look::Loading, "a fresh entry");
    }

    #[test]
    fn next_wake_is_the_exact_remaining_delay() {
        let now = base();
        let mut cache = Cache::new();
        let url = cover_url("a");
        cache.look(&url, now);
        cache.mark_failed(&url, now);
        cache.begin_frame();
        cache.look(&url, now);
        assert_eq!(
            cache.next_wake(now + Duration::from_secs(1)),
            Some(Duration::from_secs(4))
        );
    }

    #[test]
    fn next_wake_counts_an_in_flight_deadline() {
        let now = base();
        let mut cache = Cache::new();
        let url = cover_url("a");
        cache.look(&url, now);
        cache.mark_in_flight(&url, now);
        assert_eq!(
            cache.next_wake(now + Duration::from_secs(1)),
            Some(IN_FLIGHT_TIMEOUT - Duration::from_secs(1))
        );
    }

    #[test]
    fn next_wake_ignores_a_failed_entry_not_wanted_this_frame() {
        let now = base();
        let mut cache = Cache::new();
        let url = cover_url("a");
        cache.look(&url, now);
        cache.mark_failed(&url, now);
        cache.begin_frame();
        assert_eq!(cache.next_wake(now), None);
    }

    #[test]
    fn next_wake_is_none_when_nothing_is_pending() {
        let now = base();
        let mut cache = Cache::new();
        let url = cover_url("a");
        cache.insert(&url);
        cache.begin_frame();
        cache.look(&url, now);
        assert_eq!(cache.next_wake(now), None);
    }

    #[test]
    fn wanted_lists_entries_in_the_order_they_were_looked_at() {
        let now = base();
        let mut cache = Cache::new();
        // The alphabetically last URL is drawn first, so it must be fetched
        // first: the cap has to favour the top of the visible region.
        let drawn_first = cover_url("z");
        let drawn_second = cover_url("a");
        cache.look(&drawn_first, now);
        cache.look(&drawn_second, now);
        assert_eq!(cache.wanted(), vec![drawn_first, drawn_second]);
    }

    #[test]
    fn wanted_lists_only_the_entries_needing_a_fetch() {
        let now = base();
        let mut cache = Cache::new();
        let fresh = cover_url("a");
        let ready = cover_url("b");
        cache.insert(&ready);
        cache.look(&fresh, now);
        cache.look(&ready, now);
        assert_eq!(cache.wanted(), vec![fresh]);
    }

    #[test]
    fn a_repeated_look_in_one_frame_is_listed_once() {
        let now = base();
        let mut cache = Cache::new();
        let url = cover_url("a");
        cache.look(&url, now);
        cache.look(&url, now);
        assert_eq!(cache.wanted(), vec![url]);
    }

    #[test]
    fn eviction_never_takes_an_in_flight_entry() {
        let now = base();
        let mut cache = Cache::new();
        let flying = cover_url("flying");
        cache.look(&flying, now);
        cache.mark_in_flight(&flying, now);

        // Fill the cache past its bound while `flying`, touched first, is the
        // least recently used candidate for eviction. `insert` does not pin an
        // entry, so it falls out of the wanted sets and only the in-flight pin
        // keeps it.
        for index in 0..MAX_HELD {
            cache.begin_frame();
            cache.insert(&cover_url(&format!("u{index:03}")));
        }

        assert!(
            cache.entries.contains_key(&flying),
            "a fetch still running pins its entry"
        );
        assert_eq!(cache.look(&flying, now), Look::Loading);
        assert!(
            !cache.wanted().contains(&flying),
            "the pinned entry is still in flight, not wanted again"
        );
    }
}
