//! The label pass's chunking: chunks in flight, retries in their own order.

use super::super::*;
use super::test_support::owned;

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// What one fake request answers: the values it named, and the statuses it saw.
type Answer = Option<(HashMap<String, String>, StatusTally)>;

/// A metadata request whose answers a test decides, recording what it was asked
/// and how many requests were in flight at once.
struct Probe<'a> {
    rule: &'a dyn Fn(&[String]) -> Answer,
    asked: Mutex<Vec<Vec<String>>>,
    live: AtomicUsize,
    peak: AtomicUsize,
}

impl<'a> Probe<'a> {
    fn new(rule: &'a dyn Fn(&[String]) -> Answer) -> Self {
        Self {
            rule,
            asked: Mutex::new(Vec::new()),
            live: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        }
    }

    async fn request(&self, chunk: Vec<String>) -> Answer {
        let live = self.live.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(live, Ordering::SeqCst);
        tokio::task::yield_now().await;
        let answer = (self.rule)(&chunk);
        self.asked.lock().unwrap().push(chunk);
        self.live.fetch_sub(1, Ordering::SeqCst);
        answer
    }

    fn asked(&self) -> Vec<Vec<String>> {
        self.asked.lock().unwrap().clone()
    }

    fn peak(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }
}

/// One answer naming every URI in `chunk` by itself.
fn names(chunk: &[String]) -> (HashMap<String, String>, StatusTally) {
    let found = chunk.iter().map(|uri| (uri.clone(), uri.clone())).collect();
    (found, StatusTally::default())
}

#[tokio::test]
async fn a_short_answer_is_retried_as_split_halves() {
    let rule = |chunk: &[String]| {
        if chunk.len() == 4 {
            // One answer out of four is a truncation, not a refusal.
            let mut found = HashMap::new();
            found.insert(chunk[0].clone(), chunk[0].clone());
            return Some((found, StatusTally::default()));
        }
        Some(names(chunk))
    };
    let probe = Probe::new(&rule);
    let uris = owned(&["a", "b", "c", "d"]);

    let (found, _) = fetch_chunked_with(&uris, |chunk| probe.request(chunk)).await;

    assert_eq!(
        probe.asked(),
        vec![
            owned(&["a", "b", "c", "d"]),
            owned(&["b"]),
            owned(&["c", "d"])
        ],
        "the unanswered rest is split in half and asked in order"
    );
    assert_eq!(found.len(), 4);
}

#[tokio::test]
async fn a_refused_chunk_is_not_retried() {
    let rule = |_: &[String]| {
        let mut tally = StatusTally::default();
        tally.record_provider(500);
        Some((HashMap::new(), tally))
    };
    let probe = Probe::new(&rule);
    let uris = owned(&["a", "b"]);

    let (found, tally) = fetch_chunked_with(&uris, |chunk| probe.request(chunk)).await;

    assert_eq!(
        probe.asked(),
        vec![owned(&["a", "b"])],
        "nothing arrived, so asking for less would not help"
    );
    assert!(found.is_empty());
    assert_eq!(
        tally.summary().as_deref(),
        Some("metadata statuses: provider 500 x1")
    );
}

#[tokio::test]
async fn one_unanswered_uri_is_left_unknown() {
    let rule = |chunk: &[String]| {
        let mut found = HashMap::new();
        found.insert(chunk[1].clone(), chunk[1].clone());
        found.insert(chunk[2].clone(), chunk[2].clone());
        Some((found, StatusTally::default()))
    };
    let probe = Probe::new(&rule);
    let uris = owned(&["a", "b", "c"]);

    let (found, _) = fetch_chunked_with(&uris, |chunk| probe.request(chunk)).await;

    assert_eq!(
        probe.asked(),
        vec![owned(&["a", "b", "c"])],
        "one miss is an entity that is unknown, not a truncated answer"
    );
    assert!(!found.contains_key("a"));
    assert!(found.contains_key("b") && found.contains_key("c"));
}

/// The rule the equivalence test asks through: a full chunk refuses, another
/// answers all but two URIs, and a retry answers everything.
fn partly_refusing(chunk: &[String]) -> Option<(HashMap<String, String>, StatusTally)> {
    if chunk.len() == LABEL_CHUNK && chunk[0] == "u500" {
        let mut tally = StatusTally::default();
        tally.record_provider(500);
        return Some((HashMap::new(), tally));
    }
    if chunk.len() == LABEL_CHUNK && chunk[0] == "u250" {
        let found = chunk[2..]
            .iter()
            .map(|uri| (uri.clone(), uri.clone()))
            .collect();
        return Some((found, StatusTally::default()));
    }
    Some(names(chunk))
}

/// The same pass, one chunk at a time: the answer the fan-out must reproduce.
fn sequential(
    uris: &[String],
    request: &dyn Fn(&[String]) -> Answer,
) -> (HashMap<String, String>, StatusTally) {
    let mut found = HashMap::new();
    let mut tally = StatusTally::default();
    let mut queue: Vec<Vec<String>> = uris.chunks(LABEL_CHUNK).map(<[String]>::to_vec).collect();
    queue.reverse();
    while let Some(chunk) = queue.pop() {
        let Some((answers, chunk_tally)) = request(&chunk) else {
            continue;
        };
        tally.merge(chunk_tally);
        let answered = !answers.is_empty();
        let mut missing: Vec<String> = chunk
            .iter()
            .filter(|uri| !answers.contains_key(*uri))
            .cloned()
            .collect();
        found.extend(answers);
        if missing.is_empty() || !answered || missing.len() == 1 {
            continue;
        }
        let right = missing.split_off(missing.len() / 2);
        queue.push(right);
        queue.push(missing);
    }
    (found, tally)
}

#[tokio::test]
async fn seven_chunks_keep_three_in_flight_and_merge_as_one_pass() {
    let uris: Vec<String> = (0..LABEL_CHUNK * 7).map(|at| format!("u{at}")).collect();
    let probe = Probe::new(&partly_refusing);

    let (found, tally) = fetch_chunked_with(&uris, |chunk| probe.request(chunk)).await;

    assert_eq!(probe.peak(), CHUNK_FANOUT, "exactly one chunk per permit");
    let (expected, expected_tally) = sequential(&uris, &partly_refusing);
    assert_eq!(found.len(), expected.len());
    assert_eq!(
        sorted(&found),
        sorted(&expected),
        "the fan-out names exactly what the serial pass named"
    );
    assert_eq!(tally.summary(), expected_tally.summary());
    assert!(
        found.contains_key("u250") && found.contains_key("u251"),
        "a short first-wave answer still retries the URIs it left unanswered"
    );
}

/// A map's pairs in key order, so two maps compare as one value.
fn sorted(map: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = map
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    pairs.sort();
    pairs
}
