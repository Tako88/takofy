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
mod tests;
