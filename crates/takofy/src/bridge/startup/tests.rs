use std::time::Duration;

use super::{Ledger, Stage, line};

const EVERY_STAGE: [Stage; 9] = [
    Stage::SignIn,
    Stage::Connect,
    Stage::Walk,
    Stage::Labels,
    Stage::Likes,
    Stage::Albums,
    Stage::Artists,
    Stage::Shows,
    Stage::HomeFirst,
];

#[test]
fn line_carries_stage_word_milliseconds_and_count_only() {
    let text = line(
        Stage::Labels,
        Duration::from_millis(12),
        Duration::from_millis(340),
        3,
    );

    assert_eq!(text, "labels: took 12 ms, count 3, at 340 ms");
    assert!(!text.contains("spotify:"), "a line never names an address");
}

#[test]
fn a_stage_is_admitted_once_per_run() {
    let mut ledger = Ledger::default();

    assert!(ledger.admit(Stage::Walk), "the first walk logs");
    assert!(!ledger.admit(Stage::Walk), "a second walk stays quiet");
    assert!(ledger.admit(Stage::Labels), "another stage logs on its own");
    assert!(!ledger.admit(Stage::Labels));
}

#[test]
fn home_first_is_admitted_once_however_many_answers_follow() {
    let mut ledger = Ledger::default();

    assert!(ledger.admit(Stage::HomeFirst));
    for _ in 0..4 {
        assert!(
            !ledger.admit(Stage::HomeFirst),
            "only the first answered Home read is a stage"
        );
    }
}

#[test]
fn every_stage_has_a_distinct_word() {
    let mut words: Vec<&str> = EVERY_STAGE.iter().map(|stage| stage.word()).collect();
    words.sort_unstable();
    words.dedup();

    assert_eq!(words.len(), EVERY_STAGE.len(), "{words:?}");
    assert!(words.iter().all(|word| !word.is_empty()));
}
