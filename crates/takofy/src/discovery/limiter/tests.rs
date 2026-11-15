//! Injected instants only: no clock, no sleep, no socket.

use std::time::Duration;

use tokio::time::Instant;

use super::{BACKOFF_START, Decision, LIMIT, Limiter, WINDOW};
use crate::discovery::{BACKOFF_CEILING, MAX_WAIT};

fn after(base: Instant, seconds: u64) -> Instant {
    base + Duration::from_secs(seconds)
}

#[test]
fn forty_go_and_the_next_waits_for_the_window() {
    let base = Instant::now();
    let mut limiter = Limiter::new();
    for _ in 0..LIMIT {
        assert_eq!(limiter.admit(base), Decision::Go);
    }
    assert_eq!(limiter.admit(base), Decision::Wait(WINDOW));
}

#[test]
fn the_window_slides() {
    let base = Instant::now();
    let mut limiter = Limiter::new();
    for _ in 0..LIMIT {
        assert_eq!(limiter.admit(base), Decision::Go);
    }
    assert_eq!(limiter.admit(base), Decision::Wait(WINDOW));
    assert_eq!(limiter.admit(after(base, 5)), Decision::Go);
    for _ in 1..LIMIT {
        assert_eq!(limiter.admit(after(base, 5)), Decision::Go);
    }
    assert_eq!(limiter.admit(after(base, 5)), Decision::Wait(WINDOW));
}

#[test]
fn a_failure_doubles_from_two_seconds_and_stops_at_the_ceiling() {
    let base = Instant::now();
    let mut limiter = Limiter::new();
    let expected = [2, 4, 8, 16, 30, 30];
    for seconds in expected {
        limiter.backoff(base, None);
        assert_eq!(
            limiter.paused_until(),
            Some(after(base, seconds)),
            "the pause after a failure"
        );
    }
    assert_eq!(BACKOFF_START, Duration::from_secs(2));
}

#[test]
fn a_wait_over_the_lookup_budget_is_refused() {
    let base = Instant::now();
    let mut limiter = Limiter::new();
    limiter.backoff(base, Some(MAX_WAIT + Duration::from_secs(1)));
    assert_eq!(limiter.admit(base), Decision::Refuse);
}

#[test]
fn a_short_pause_is_a_wait() {
    let base = Instant::now();
    let mut limiter = Limiter::new();
    limiter.backoff(base, None);
    assert_eq!(limiter.admit(base), Decision::Wait(Duration::from_secs(2)));
}

#[test]
fn retry_after_wins_and_is_capped() {
    let base = Instant::now();
    let mut limiter = Limiter::new();
    limiter.backoff(base, None);
    assert_eq!(limiter.paused_until(), Some(after(base, 2)));

    limiter.backoff(base, Some(Duration::from_secs(8)));
    assert_eq!(limiter.paused_until(), Some(after(base, 8)));

    limiter.backoff(base, Some(Duration::from_secs(60)));
    assert_eq!(limiter.paused_until(), Some(base + BACKOFF_CEILING));
    assert_eq!(limiter.admit(base), Decision::Refuse);
}

#[test]
fn an_expired_pause_goes_again() {
    let base = Instant::now();
    let mut limiter = Limiter::new();
    limiter.backoff(base, None);
    assert_eq!(limiter.admit(after(base, 2)), Decision::Go);
}

#[test]
fn a_success_resets_the_pause_and_the_doubling() {
    let base = Instant::now();
    let mut limiter = Limiter::new();
    limiter.backoff(base, None);
    limiter.backoff(base, None);
    limiter.ok();
    assert_eq!(limiter.paused_until(), None);
    assert_eq!(limiter.admit(base), Decision::Go);

    limiter.backoff(base, None);
    assert_eq!(limiter.paused_until(), Some(after(base, 2)));
}

#[test]
fn a_blocked_limiter_never_goes() {
    let base = Instant::now();
    let mut limiter = Limiter::new();
    limiter.backoff(base, Some(Duration::from_secs(30)));
    for seconds in 0..30 {
        assert_ne!(limiter.admit(after(base, seconds)), Decision::Go);
    }
}
