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
mod tests;
