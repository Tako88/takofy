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
mod tests {
    use super::*;

    fn track(title: &str, artists: &str, album: Option<&str>) -> TrackEntry {
        TrackEntry {
            uri: format!("spotify:track:{title}"),
            title: title.to_owned(),
            artists: artists.to_owned(),
            album: album.map(str::to_owned),
            album_uri: None,
            artist_uris: Vec::new(),
            artist_names: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
        }
    }

    #[test]
    fn title_artist_or_album_match_case_insensitively() {
        let entries = vec![
            track("Midnight City", "M83", Some("Hurry Up, We're Dreaming")),
            track("Outro", "M83", Some("Hurry Up, We're Dreaming")),
        ];

        assert_eq!(hits(&entries, "midnight"), vec![0]);
        assert_eq!(hits(&entries, "m83"), vec![0, 1]);
        assert_eq!(hits(&entries, "dreaming"), vec![0, 1]);
    }

    #[test]
    fn non_ascii_case_folds() {
        let entries = vec![track("Été", "Hana Sō", None)];

        assert_eq!(hits(&entries, "é"), vec![0]);
        assert_eq!(hits(&entries, "ō"), vec![0]);
    }

    #[test]
    fn the_query_is_trimmed() {
        let mut filter = TrackFilter::default();
        filter.edit("spotify:playlist:a").push_str("  midnight  ");

        assert_eq!(
            filter.needle("spotify:playlist:a").as_deref(),
            Some("midnight")
        );
    }

    #[test]
    fn blank_or_whitespace_is_no_filter() {
        let mut filter = TrackFilter::default();
        assert_eq!(filter.needle("spotify:playlist:a"), None);

        filter.edit("spotify:playlist:a").push_str("   ");
        assert_eq!(filter.needle("spotify:playlist:a"), None);
    }

    #[test]
    fn a_missing_album_is_not_a_match_and_not_a_panic() {
        let entries = vec![track("Outro", "M83", None)];

        assert_eq!(hits(&entries, "dreaming"), Vec::<usize>::new());
        assert_eq!(hits(&entries, "outro"), vec![0]);
    }

    #[test]
    fn hits_are_ascending_true_indices() {
        let matching = [1, 4, 9, 12, 20, 23, 31, 38, 44, 51, 57, 60];
        let entries: Vec<TrackEntry> = (0..62)
            .map(|index| {
                if matching.contains(&index) {
                    track(&format!("midnight {index}"), "M83", None)
                } else {
                    track(&format!("other {index}"), "Vela Moreno", None)
                }
            })
            .collect();

        let found = hits(&entries, "midnight");

        assert_eq!(found, matching.to_vec());
        assert!(found.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn another_playlist_resets_the_text() {
        let mut filter = TrackFilter::default();
        filter.edit("spotify:playlist:a").push_str("midnight");

        let text = filter.edit("spotify:playlist:b");
        assert!(text.is_empty(), "another playlist starts unfiltered");
    }

    #[test]
    fn needle_for_another_playlist_is_none() {
        let mut filter = TrackFilter::default();
        filter.edit("spotify:playlist:a").push_str("midnight");

        assert_eq!(filter.needle("spotify:playlist:b"), None);
    }
}
