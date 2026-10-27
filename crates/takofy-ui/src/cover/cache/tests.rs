use super::*;

fn cover_url(tail: &str) -> CoverUrl {
    CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
}

/// A base instant the tests advance by a `Duration`; nothing sleeps.
fn base() -> Instant {
    Instant::now()
}

/// The bytes a decoded 300×300 tile costs, the wire's default size.
const DEFAULT_TILE: usize = 300 * 300 * 4;

/// The bytes a decoded 640×640 tile costs, the wire's large size.
const LARGE_TILE: usize = 640 * 640 * 4;

/// The bytes a decoded 64×64 tile costs, the wire's small size.
const ROW_TILE: usize = 64 * 64 * 4;

fn tile(index: usize) -> CoverUrl {
    cover_url(&format!("u{index:03}"))
}

#[test]
fn a_new_entry_is_loading_and_ready_after_insert() {
    let now = base();
    let mut cache = Cache::new();
    assert_eq!(cache.look(&cover_url("a"), now), Look::Loading);
    cache.insert(&cover_url("a"), DEFAULT_TILE);
    assert_eq!(cache.look(&cover_url("a"), now), Look::Ready);
}

#[test]
fn distinct_urls_are_distinct_entries() {
    let now = base();
    let mut cache = Cache::new();
    cache.insert(&cover_url("a"), DEFAULT_TILE);
    cache.insert(&cover_url("b"), DEFAULT_TILE);
    assert_eq!(cache.look(&cover_url("a"), now), Look::Ready);
    assert_eq!(cache.look(&cover_url("b"), now), Look::Ready);
    assert_eq!(cache.entries.len(), 2);
}

#[test]
fn eviction_takes_the_least_recently_used_first() {
    let now = base();
    let mut cache = Cache::new();
    for index in 0..MAX_HELD {
        cache.insert(&cover_url(&format!("u{index:03}")), DEFAULT_TILE);
    }
    // Touch the oldest so it is no longer the least recently used.
    assert_eq!(cache.look(&cover_url("u000"), now), Look::Ready);
    cache.insert(&cover_url("overflow"), DEFAULT_TILE);

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
            cache.insert(&url, DEFAULT_TILE);
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
        cache.insert(&cover_url(&format!("u{index:03}")), DEFAULT_TILE);
    }
    assert_eq!(cache.look(&cover_url("u000"), now), Look::Ready);
    for index in 0..MAX_HELD {
        cache.insert(&cover_url(&format!("v{index:03}")), DEFAULT_TILE);
    }
    assert_eq!(cache.look(&cover_url("u000"), now), Look::Ready);
}

#[test]
fn an_entry_wanted_in_the_previous_frame_survives() {
    let now = base();
    let mut cache = Cache::new();
    for index in 0..MAX_HELD {
        cache.insert(&cover_url(&format!("u{index:03}")), DEFAULT_TILE);
    }
    assert_eq!(cache.look(&cover_url("u000"), now), Look::Ready);
    // The next frame wants nothing: u000 moves from current to previous.
    cache.begin_frame();
    for index in 0..MAX_HELD {
        cache.insert(&cover_url(&format!("v{index:03}")), DEFAULT_TILE);
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
        cache.insert(&cover_url(&format!("u{index:03}")), DEFAULT_TILE);
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
    cache.insert(&url, DEFAULT_TILE);
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
    cache.insert(&ready, DEFAULT_TILE);
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

/// The count cap, not the budget, bounds the default tile: a 96 MiB budget
/// would hold 279 of them.
#[test]
fn a_default_tile_is_bounded_by_the_count_cap() {
    let mut cache = Cache::new();
    for index in 0..MAX_HELD {
        cache.insert(&tile(index), DEFAULT_TILE);
    }
    // The count cap is the bound for this tile, not the budget.
    const { assert!(MAX_BYTES / DEFAULT_TILE > MAX_HELD) };

    cache.insert(&tile(MAX_HELD), DEFAULT_TILE);
    assert_eq!(cache.entries.len(), MAX_HELD);
    assert!(!cache.entries.contains_key(&tile(0)), "the oldest went");
    assert!(cache.entries.contains_key(&tile(MAX_HELD)));
}

#[test]
fn a_large_tile_costs_its_pixels() {
    let mut cache = Cache::new();
    let held = MAX_BYTES / LARGE_TILE;
    assert!(held < MAX_HELD, "the budget is the bound");
    for index in 0..held {
        cache.insert(&tile(index), LARGE_TILE);
    }
    assert_eq!(cache.bytes, held * LARGE_TILE);

    cache.insert(&tile(held), LARGE_TILE);
    assert_eq!(cache.entries.len(), held);
    assert!(!cache.entries.contains_key(&tile(0)), "the oldest went");
    assert!(cache.bytes <= MAX_BYTES);
}

#[test]
fn a_row_tile_is_bounded_by_the_entry_cap() {
    let mut cache = Cache::new();
    for index in 0..MAX_HELD + 8 {
        cache.insert(&tile(index), ROW_TILE);
    }
    assert_eq!(cache.entries.len(), MAX_HELD);
    assert!(!cache.entries.contains_key(&tile(0)), "the oldest went");
    assert!(cache.bytes <= MAX_BYTES);
}

#[test]
fn replacing_an_entry_does_not_double_count_its_bytes() {
    let mut cache = Cache::new();
    let url = cover_url("a");
    cache.insert(&url, LARGE_TILE);
    cache.insert(&url, LARGE_TILE);
    assert_eq!(cache.bytes, LARGE_TILE);
    assert_eq!(cache.entries.len(), 1);
}

#[test]
fn an_evicted_entry_gives_its_bytes_back() {
    let mut cache = Cache::new();
    let held = MAX_BYTES / LARGE_TILE;
    for index in 0..held {
        cache.insert(&tile(index), LARGE_TILE);
    }
    assert_eq!(cache.bytes, held * LARGE_TILE);

    cache.insert(&cover_url("overflow"), LARGE_TILE);
    assert_eq!(cache.bytes, held * LARGE_TILE, "no byte leaked");
    let mut held_bytes = 0;
    for entry in cache.entries.values() {
        held_bytes += entry.bytes();
    }
    assert_eq!(held_bytes, cache.bytes, "the total matches the entries");
}

#[test]
fn a_wanted_large_tile_survives_a_full_budget() {
    let now = base();
    let mut cache = Cache::new();
    let wanted = cover_url("wanted");
    assert_eq!(cache.look(&wanted, now), Look::Loading);
    cache.insert(&wanted, LARGE_TILE);

    for index in 0..MAX_BYTES / LARGE_TILE {
        cache.insert(&tile(index), LARGE_TILE);
    }

    assert_eq!(cache.look(&wanted, now), Look::Ready);
    assert!(cache.bytes <= MAX_BYTES);
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
        cache.insert(&cover_url(&format!("u{index:03}")), DEFAULT_TILE);
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
