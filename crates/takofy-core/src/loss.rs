//! Whether this window still holds the account's sound.
//!
//! The cluster is the only authority: a push naming another device is a loss,
//! and the two baselines name the pushes that predate our activation. Nothing
//! here decides to stop playing; it only names the verdict.

use std::time::Instant;

use crate::connect::ClusterView;

/// Which push would be ours to ignore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Baseline {
    /// A push stamped at or before this instant predates our activation.
    After(i64),
    /// A push arriving before this instant raced our activation's reply.
    Until(Instant),
}

/// What one pushed view means for our hold on the sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Judgement {
    /// The cluster names us.
    Held,
    /// It names another device, but the push may predate our activation.
    Stale,
    /// It names another device, and the push is ours to obey.
    Lost,
}

/// Reads one pushed view against the baseline chosen at activation.
pub(crate) fn judge(view: &ClusterView, baseline: Baseline, now: Instant) -> Judgement {
    if view.held_here {
        return Judgement::Held;
    }
    match baseline {
        // A zero stamp is no stamp: playing over the owner is not recoverable,
        // so an unstamped push counts as fresh.
        Baseline::After(ms) => {
            if view.changed_ms != 0 && view.changed_ms <= ms {
                Judgement::Stale
            } else {
                Judgement::Lost
            }
        }
        Baseline::Until(until) => {
            if now < until {
                Judgement::Stale
            } else {
                Judgement::Lost
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRACE_MS: u64 = 2_000;
    const BASELINE_MS: i64 = 5_000;

    fn view(held_here: bool, changed_ms: i64) -> ClusterView {
        ClusterView {
            devices: None,
            holder: (!held_here).then(|| "phone".to_owned()),
            held_here,
            changed_ms,
            playback: None,
        }
    }

    #[test]
    fn a_view_naming_us_is_held() {
        let now = Instant::now();

        assert_eq!(
            judge(&view(true, 0), Baseline::After(BASELINE_MS), now),
            Judgement::Held
        );
        assert_eq!(
            judge(&view(true, 9_000), Baseline::Until(now), now),
            Judgement::Held
        );
    }

    #[test]
    fn after_a_stale_push_is_ignored() {
        let now = Instant::now();

        assert_eq!(
            judge(
                &view(false, BASELINE_MS - 1_000),
                Baseline::After(BASELINE_MS),
                now
            ),
            Judgement::Stale
        );
        assert_eq!(
            judge(&view(false, BASELINE_MS), Baseline::After(BASELINE_MS), now),
            Judgement::Stale,
            "a push stamped at the activation instant is still ours to ignore"
        );
    }

    #[test]
    fn after_a_zero_changed_time_counts_as_fresh() {
        assert_eq!(
            judge(
                &view(false, 0),
                Baseline::After(BASELINE_MS),
                Instant::now()
            ),
            Judgement::Lost
        );
    }

    #[test]
    fn after_a_fresh_push_naming_another_device_is_lost() {
        assert_eq!(
            judge(
                &view(false, BASELINE_MS + 1),
                Baseline::After(BASELINE_MS),
                Instant::now()
            ),
            Judgement::Lost
        );
    }

    #[test]
    fn until_a_push_inside_the_grace_is_stale() {
        let now = Instant::now();
        let until = now + std::time::Duration::from_millis(GRACE_MS);

        assert_eq!(
            judge(&view(false, 9_000), Baseline::Until(until), now),
            Judgement::Stale
        );
    }

    #[test]
    fn until_a_push_after_the_grace_is_lost() {
        let now = Instant::now();
        let until = now + std::time::Duration::from_millis(GRACE_MS);

        assert_eq!(
            judge(
                &view(false, 9_000),
                Baseline::Until(until),
                until + std::time::Duration::from_millis(1)
            ),
            Judgement::Lost
        );
        assert_eq!(
            judge(&view(false, 9_000), Baseline::Until(now), now),
            Judgement::Lost,
            "the grace ends at its own instant"
        );
    }
}
