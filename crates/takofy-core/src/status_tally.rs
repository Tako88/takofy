//! Per-pass counts of the status codes an extended-metadata read reports.

use std::collections::{BTreeMap, BTreeSet};

/// The codes one labelling pass saw, counted by code and by distinct entity.
#[derive(Debug, Default)]
pub(crate) struct StatusTally {
    provider: BTreeMap<i32, usize>,
    entities: BTreeMap<i32, BTreeSet<String>>,
}

impl StatusTally {
    /// Records one array-level provider status.
    ///
    /// Counts arrays per request, so a chunk that is split and retried records
    /// its provider code again. That is acceptable: the per-pass figure is the
    /// entity count, which dedupes by URI.
    pub(crate) fn record_provider(&mut self, code: i32) {
        if !success(code) {
            *self.provider.entry(code).or_default() += 1;
        }
    }

    /// Records one entity's status, once per distinct URI and code.
    pub(crate) fn record_entity(&mut self, uri: &str, code: i32) {
        if !success(code) {
            self.entities
                .entry(code)
                .or_default()
                .insert(uri.to_owned());
        }
    }

    /// Adds another pass's counts to this one.
    pub(crate) fn merge(&mut self, other: Self) {
        for (code, count) in other.provider {
            *self.provider.entry(code).or_default() += count;
        }
        for (code, uris) in other.entities {
            self.entities.entry(code).or_default().extend(uris);
        }
    }

    /// The codes and counts for one log line, or `None` when every code was a success.
    pub(crate) fn summary(&self) -> Option<String> {
        let mut sections = Vec::new();
        if !self.provider.is_empty() {
            let counts: Vec<String> = self
                .provider
                .iter()
                .map(|(code, count)| format!("{code} x{count}"))
                .collect();
            sections.push(format!("provider {}", counts.join(", ")));
        }
        if !self.entities.is_empty() {
            let counts: Vec<String> = self
                .entities
                .iter()
                .map(|(code, uris)| format!("{code} x{}", uris.len()))
                .collect();
            sections.push(format!("entities {}", counts.join(", ")));
        }
        if sections.is_empty() {
            None
        } else {
            Some(format!("metadata statuses: {}", sections.join("; ")))
        }
    }
}

/// Whether a status code means the read succeeded.
fn success(code: i32) -> bool {
    code == 200 || code == 0
}

#[cfg(test)]
mod tests {
    use super::StatusTally;

    #[test]
    fn all_ok_summarises_to_nothing() {
        let mut tally = StatusTally::default();
        tally.record_provider(200);
        tally.record_entity("spotify:track:a", 200);
        tally.record_entity("spotify:track:b", 0);
        assert_eq!(tally.summary(), None);
    }

    #[test]
    fn entity_codes_are_counted_by_distinct_uri() {
        let mut tally = StatusTally::default();
        tally.record_entity("spotify:track:a", 404);
        tally.record_entity("spotify:track:b", 404);
        tally.record_entity("spotify:track:c", 500);
        assert_eq!(
            tally.summary().as_deref(),
            Some("metadata statuses: entities 404 x2, 500 x1")
        );
    }

    #[test]
    fn the_same_uri_recorded_twice_counts_once() {
        let mut tally = StatusTally::default();
        tally.record_entity("spotify:track:a", 404);
        tally.record_entity("spotify:track:a", 404);
        assert_eq!(
            tally.summary().as_deref(),
            Some("metadata statuses: entities 404 x1")
        );
    }

    #[test]
    fn provider_and_entity_codes_are_reported_apart() {
        let mut tally = StatusTally::default();
        tally.record_provider(500);
        tally.record_provider(500);
        tally.record_entity("spotify:track:a", 404);
        assert_eq!(
            tally.summary().as_deref(),
            Some("metadata statuses: provider 500 x2; entities 404 x1")
        );
    }

    #[test]
    fn merge_unions_the_uri_sets_and_adds_the_provider_counts() {
        let mut left = StatusTally::default();
        left.record_provider(500);
        left.record_entity("spotify:track:a", 404);
        let mut right = StatusTally::default();
        right.record_provider(500);
        right.record_entity("spotify:track:a", 404);
        right.record_entity("spotify:track:b", 404);

        left.merge(right);

        assert_eq!(
            left.summary().as_deref(),
            Some("metadata statuses: provider 500 x2; entities 404 x2")
        );
    }

    #[test]
    fn a_zero_code_is_a_success() {
        let mut tally = StatusTally::default();
        // An absent header reads as the protobuf default, whose success code is zero.
        tally.record_entity("spotify:track:a", 0);
        assert_eq!(tally.summary(), None);
    }

    #[test]
    fn the_summary_carries_codes_and_counts_but_no_uris() {
        let mut tally = StatusTally::default();
        tally.record_entity("spotify:track:secret", 404);
        let summary = tally.summary().expect("a failed entity is reported");
        assert!(!summary.contains("spotify:"), "{summary}");
        assert!(!summary.contains("secret"), "{summary}");
        assert_eq!(summary, "metadata statuses: entities 404 x1");
    }
}
