//! The playlist page's filter: the query it keeps and the rows it leaves.
//!
//! The query belongs to one playlist at a time, so a reply for a different
//! playlist cannot inherit it. Matching is trimmed and case-insensitive over a
//! row's title, artists and album; the answer is the rows' true indices, so a
//! filtered row keeps its number and the selection keeps its index space.

use takofy_model::TrackEntry;

/// The longest query the field accepts.
pub const QUERY_MAX: usize = 200;

/// One page's query, and the playlist it was typed on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrackFilter {
    playlist: String,
    text: String,
}

impl TrackFilter {
    /// The text the field edits, reset when the playlist it belongs to changes.
    pub fn edit(&mut self, playlist: &str) -> &mut String {
        if self.playlist != playlist {
            self.playlist = playlist.to_owned();
            self.text.clear();
        }
        &mut self.text
    }

    /// The needle the rows match, when this playlist owns a non-blank query.
    pub fn needle(&self, playlist: &str) -> Option<String> {
        if self.playlist != playlist {
            return None;
        }
        let trimmed = self.text.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_lowercase())
    }

    /// Drops the query, so the whole list shows again.
    pub fn clear(&mut self) {
        self.text.clear();
    }

    /// Drops the query and hands back the text it held.
    ///
    /// A write that empties the page keeps the words, so a failed one can put
    /// them back beside the rows it restores.
    pub fn take(&mut self) -> String {
        std::mem::take(&mut self.text)
    }
}

/// The ascending true indices whose title, artists or album contain `needle`.
pub fn hits(entries: &[TrackEntry], needle: &str) -> Vec<usize> {
    entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            entry.title.to_lowercase().contains(needle)
                || entry.artists.to_lowercase().contains(needle)
                || entry
                    .album
                    .as_deref()
                    .is_some_and(|album| album.to_lowercase().contains(needle))
        })
        .map(|(index, _)| index)
        .collect()
}

#[cfg(test)]
mod tests;
