use super::{Anchor, Clock};
use std::time::Duration;
use tokio::time::Instant;

fn anchor(position_ms: u32, timestamp_ms: i64, speed: f64) -> Anchor {
    Anchor {
        position_ms,
        timestamp_ms,
        speed,
    }
}

#[test]
fn speed_zero_holds_still() {
    let start = Instant::now();
    let mut clock = Clock::default();
    clock.anchor(anchor(30_000, 1, 0.0), start);
    assert_eq!(clock.live_ms(start + Duration::from_secs(9)), 30_000);
}

#[test]
fn speed_one_advances_with_the_clock() {
    let start = Instant::now();
    let mut clock = Clock::default();
    clock.anchor(anchor(30_000, 1, 1.0), start);
    assert_eq!(clock.live_ms(start + Duration::from_secs(5)), 35_000);
}

#[test]
fn speed_two_advances_twice_as_fast() {
    let start = Instant::now();
    let mut clock = Clock::default();
    clock.anchor(anchor(10_000, 1, 2.0), start);
    assert_eq!(clock.live_ms(start + Duration::from_millis(1_500)), 13_000);
}

#[test]
fn an_older_anchor_is_ignored() {
    let start = Instant::now();
    let mut clock = Clock::default();
    clock.anchor(anchor(10_000, 500, 1.0), start);
    clock.anchor(anchor(90_000, 400, 1.0), start + Duration::from_secs(1));
    assert_eq!(clock.current().map(|a| a.position_ms), Some(10_000));
    assert_eq!(clock.live_ms(start + Duration::from_secs(1)), 11_000);
}

#[test]
fn an_equal_timestamp_wins() {
    let start = Instant::now();
    let mut clock = Clock::default();
    clock.anchor(anchor(10_000, 500, 1.0), start);
    clock.anchor(anchor(20_000, 500, 1.0), start);
    assert_eq!(clock.current().map(|a| a.position_ms), Some(20_000));
}

#[test]
fn never_negative_and_clamped() {
    let start = Instant::now();
    let mut clock = Clock::default();
    assert_eq!(clock.live_ms(start), 0);
    clock.anchor(anchor(u32::MAX - 10, 1, 1.0), start);
    assert_eq!(clock.live_ms(start + Duration::from_secs(1)), u32::MAX);
}
