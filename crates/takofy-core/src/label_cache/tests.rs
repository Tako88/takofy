use std::collections::HashMap;
use std::sync::Mutex;

use super::UriCache;

/// The batches a test's fetcher was asked for, in order.
#[derive(Default)]
struct Recorder {
    batches: Mutex<Vec<Vec<String>>>,
}

impl Recorder {
    fn record(&self, batch: &[String]) {
        self.batches
            .lock()
            .expect("the recorder lock")
            .push(batch.to_vec());
    }

    /// Every URI asked for, across all batches.
    fn asked(&self) -> Vec<String> {
        self.batches
            .lock()
            .expect("the recorder lock")
            .iter()
            .flatten()
            .cloned()
            .collect()
    }

    fn calls(&self) -> usize {
        self.batches.lock().expect("the recorder lock").len()
    }
}

fn uris(list: &[&str]) -> Vec<String> {
    list.iter().map(|uri| (*uri).to_owned()).collect()
}

fn label(uri: &str) -> String {
    format!("label {uri}")
}

fn answers(batch: &[String]) -> HashMap<String, String> {
    batch.iter().map(|uri| (uri.clone(), label(uri))).collect()
}

fn entries(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(uri, value)| ((*uri).to_owned(), (*value).to_owned()))
        .collect()
}

#[tokio::test]
async fn an_empty_cache_asks_the_fetch_for_every_uri() {
    let recorder = Recorder::default();
    let cache: UriCache<String> = UriCache::new();
    let asked = uris(&["a", "b"]);

    let labels = cache
        .through_cache(asked.clone(), |batch: Vec<String>| {
            recorder.record(&batch);
            async move { answers(&batch) }
        })
        .await;

    assert_eq!(recorder.asked(), asked);
    assert_eq!(labels, answers(&asked));
}

#[tokio::test]
async fn a_second_read_asks_the_fetch_for_nothing() {
    let recorder = Recorder::default();
    let cache: UriCache<String> = UriCache::new();
    let asked = uris(&["a", "b"]);

    let first = cache
        .through_cache(asked.clone(), |batch: Vec<String>| {
            recorder.record(&batch);
            async move { answers(&batch) }
        })
        .await;
    let second = cache
        .through_cache(asked, |batch: Vec<String>| {
            recorder.record(&batch);
            async move { answers(&batch) }
        })
        .await;

    assert_eq!(recorder.calls(), 1);
    assert_eq!(second, first);
    assert_eq!(cache.len(), 2);
}

#[tokio::test]
async fn a_partial_hit_asks_only_for_the_misses_and_merges_the_values() {
    let recorder = Recorder::default();
    let cache: UriCache<String> = UriCache::new();

    cache
        .through_cache(uris(&["a"]), |batch: Vec<String>| {
            recorder.record(&batch);
            async move { answers(&batch) }
        })
        .await;
    let labels = cache
        .through_cache(uris(&["a", "b"]), |batch: Vec<String>| {
            recorder.record(&batch);
            async move { answers(&batch) }
        })
        .await;

    assert_eq!(recorder.asked(), uris(&["a", "b"]));
    assert_eq!(labels, answers(&uris(&["a", "b"])));
    assert_eq!(cache.len(), 2);
}

#[tokio::test]
async fn an_unanswered_miss_is_not_stored_and_is_asked_for_again() {
    let recorder = Recorder::default();
    let cache: UriCache<String> = UriCache::new();
    let answer = |batch: &[String]| {
        let mut found = answers(batch);
        found.remove("b");
        found
    };

    let first = cache
        .through_cache(uris(&["a", "b"]), |batch: Vec<String>| {
            recorder.record(&batch);
            async move { answer(&batch) }
        })
        .await;
    assert_eq!(first.get("a").map(String::as_str), Some("label a"));
    assert!(!first.contains_key("b"));
    assert_eq!(cache.len(), 1);

    let second = cache
        .through_cache(uris(&["a", "b"]), |batch: Vec<String>| {
            recorder.record(&batch);
            async move { answer(&batch) }
        })
        .await;

    assert_eq!(recorder.asked(), uris(&["a", "b", "b"]));
    assert_eq!(second, first);
}

#[tokio::test]
async fn the_cap_holds_and_evicts_the_oldest_first() {
    let recorder = Recorder::default();
    let cache: UriCache<String> = UriCache::with_cap(2);

    // One store each, so the eviction order is the order they arrived in.
    cache.store(&entries(&[("a", "1")]));
    cache.store(&entries(&[("b", "2")]));
    cache.store(&entries(&[("c", "3")]));
    assert_eq!(cache.len(), 2);

    let kept = cache
        .through_cache(uris(&["b", "c"]), |batch: Vec<String>| {
            recorder.record(&batch);
            async move { answers(&batch) }
        })
        .await;
    assert_eq!(recorder.calls(), 0);
    assert_eq!(kept.get("b").map(String::as_str), Some("2"));
    assert_eq!(kept.get("c").map(String::as_str), Some("3"));

    cache
        .through_cache(uris(&["a"]), |batch: Vec<String>| {
            recorder.record(&batch);
            async move { answers(&batch) }
        })
        .await;
    assert_eq!(recorder.asked(), uris(&["a"]));
}

#[test]
fn a_read_does_not_refresh_the_eviction_order() {
    let cache: UriCache<String> = UriCache::with_cap(2);
    cache.store(&entries(&[("a", "1")]));
    cache.store(&entries(&[("b", "2")]));

    assert_eq!(
        cache.take(&uris(&["a"])).get("a").map(String::as_str),
        Some("1")
    );
    cache.store(&entries(&[("c", "3")]));

    assert!(!cache.take(&uris(&["a"])).contains_key("a"));
    assert!(cache.take(&uris(&["b"])).contains_key("b"));
}

#[test]
fn overwriting_a_key_adds_no_eviction_slot() {
    let cache: UriCache<String> = UriCache::with_cap(2);
    cache.store(&entries(&[("a", "1")]));
    cache.store(&entries(&[("a", "2")]));
    cache.store(&entries(&[("b", "3")]));

    assert_eq!(cache.len(), 2);
    assert_eq!(
        cache.take(&uris(&["a"])).get("a").map(String::as_str),
        Some("2")
    );
}

#[tokio::test]
async fn a_pass_larger_than_the_cap_returns_every_label() {
    let recorder = Recorder::default();
    let cache: UriCache<String> = UriCache::with_cap(1);
    let asked = uris(&["a", "b", "c"]);

    let labels = cache
        .through_cache(asked.clone(), |batch: Vec<String>| {
            recorder.record(&batch);
            async move { answers(&batch) }
        })
        .await;

    assert_eq!(labels, answers(&asked));
    assert_eq!(cache.len(), 1);
    assert_eq!(recorder.calls(), 1);
}

#[tokio::test]
async fn an_empty_request_asks_the_fetch_for_nothing() {
    let recorder = Recorder::default();
    let cache: UriCache<String> = UriCache::new();

    let labels = cache
        .through_cache(Vec::new(), |batch: Vec<String>| {
            recorder.record(&batch);
            async move { answers(&batch) }
        })
        .await;

    assert_eq!(recorder.calls(), 0);
    assert!(labels.is_empty());
}

#[tokio::test]
async fn a_poisoned_lock_still_serves() {
    let cache: UriCache<String> = UriCache::new();
    cache.store(&entries(&[("a", "1")]));

    let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _held = cache.state.lock().expect("the cache lock");
        panic!("poison the cache lock");
    }));
    assert!(poisoned.is_err());

    assert_eq!(
        cache.take(&uris(&["a"])).get("a").map(String::as_str),
        Some("1")
    );
    cache.store(&entries(&[("b", "2")]));
    assert_eq!(cache.len(), 2);
}
