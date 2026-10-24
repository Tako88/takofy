//! Which playlist covers must be asked for again after a row edit.
//!
//! A derived cover is a picture the server took from the playlist's first track,
//! so removing, moving or appending that track can make it wrong while the row
//! edit itself says nothing about it. A cover is *due* once such an edit lands,
//! and a playlist is *awaiting* while one or more reads that can answer it are
//! in flight. Reads come back in the order they were issued, so only the last
//! reply for an awaiting playlist is allowed to overwrite the cover.
//!
//! It imports no I/O. Two maps, keyed by playlist URI.

use std::collections::{HashMap, HashSet};

use takofy_model::TrackEntry;

/// The derived covers that must be asked for again, and the asks in flight.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoverRefresh {
    /// Playlists whose derived cover a row edit may have invalidated.
    due: HashSet<String>,
    /// Playlists with reads in flight, and how many are still unanswered.
    awaiting: HashMap<String, u32>,
}

impl CoverRefresh {
    /// Marks the derived cover of `uri` due, so it is asked for again.
    ///
    /// This only queues the ask. Clearing the entry's cover and the cached
    /// image belongs to the caller that drops them.
    pub fn drop_for(&mut self, uri: &str) {
        self.due.insert(uri.to_owned());
    }

    /// Takes one due playlist that is listed and is not the kept page.
    ///
    /// The returned URI gains an ask in [`Self::awaiting`], so the reply to the
    /// read that follows may write its cover. A due playlist that is not listed
    /// stays due until [`Self::retain`] prunes it.
    ///
    /// `keep` is for the page whose read is already in flight: the caller passes
    /// it only while that page's tracks are loading, because that read answers
    /// its cover. A page that is merely on screen and settled is not kept, or its
    /// cover would never be asked for again.
    pub fn take_due(&mut self, present: &[String], keep: Option<&str>) -> Option<String> {
        if self.due.is_empty() {
            return None;
        }
        let uri = present
            .iter()
            .find(|uri| Some(uri.as_str()) != keep && self.due.contains(*uri))?
            .clone();
        self.due.remove(&uri);
        self.count_ask(&uri);
        Some(uri)
    }

    /// Marks `uri`'s re-read as issued, if it was due.
    pub fn asked(&mut self, uri: &str) {
        if self.due.remove(uri) {
            self.count_ask(uri);
        }
    }

    /// Whether `uri`'s reply is the last one in flight, consuming that ask.
    ///
    /// A playlist asked twice is answered twice: the earlier reply is
    /// superseded, and only the one that empties the count is authoritative.
    pub fn answer(&mut self, uri: &str) -> bool {
        let Some(count) = self.awaiting.get_mut(uri) else {
            return false;
        };
        *count -= 1;
        let last = *count == 0;
        if last {
            self.awaiting.remove(uri);
        }
        last
    }

    /// Forgets every playlist that is no longer listed.
    pub fn retain(&mut self, present: &[String]) {
        self.due.retain(|uri| present.contains(uri));
        self.awaiting.retain(|uri, _| present.contains(uri));
    }

    /// Forgets one playlist entirely, so a gone page is never asked for.
    pub fn forget(&mut self, uri: &str) {
        self.due.remove(uri);
        self.awaiting.remove(uri);
    }

    /// Records one more read for `uri` in flight.
    fn count_ask(&mut self, uri: &str) {
        *self.awaiting.entry(uri.to_owned()).or_insert(0) += 1;
    }
}

/// The URI of the first row that is a track a playlist can hold.
///
/// An episode or a local file in front is not a track, so it cannot be the one a
/// derived cover is taken from.
pub fn first_track_uri(rows: &[TrackEntry]) -> Option<&str> {
    rows.iter()
        .find(|row| row.is_track())
        .map(|row| row.uri.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "spotify:playlist:a";
    const B: &str = "spotify:playlist:b";
    const C: &str = "spotify:playlist:c";

    fn listed(uris: &[&str]) -> Vec<String> {
        uris.iter().map(|uri| (*uri).to_owned()).collect()
    }

    fn track(uri: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: "A Title".to_owned(),
            artists: String::new(),
            album: None,
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
    fn take_due_returns_a_listed_uri_and_moves_it_to_awaiting() {
        let mut refresh = CoverRefresh::default();
        refresh.drop_for(A);
        assert_eq!(
            refresh.take_due(&listed(&[A, B]), None),
            Some(A.to_owned()),
            "a due playlist that is listed must be taken"
        );
        assert!(
            refresh.answer(A),
            "a taken playlist must be awaiting its reply"
        );
    }

    #[test]
    fn take_due_skips_the_kept_uri_and_leaves_it_due() {
        let mut refresh = CoverRefresh::default();
        refresh.drop_for(A);
        assert_eq!(
            refresh.take_due(&listed(&[A]), Some(A)),
            None,
            "the kept page's own read answers its cover, so it is not due"
        );
        assert_eq!(
            refresh.take_due(&listed(&[A]), None),
            Some(A.to_owned()),
            "a skipped uri must stay due, not be dropped"
        );
    }

    #[test]
    fn take_due_leaves_an_unlisted_uri_due() {
        let mut refresh = CoverRefresh::default();
        refresh.drop_for(B);
        assert_eq!(
            refresh.take_due(&listed(&[A]), None),
            None,
            "an unlisted playlist must not be asked for"
        );
        assert_eq!(
            refresh.take_due(&listed(&[A, B]), None),
            Some(B.to_owned()),
            "an unlisted playlist must still be due once it is listed"
        );
    }

    #[test]
    fn take_due_pops_each_uri_once() {
        let mut refresh = CoverRefresh::default();
        refresh.drop_for(A);
        refresh.drop_for(B);
        let present = listed(&[A, B]);
        let mut taken = Vec::new();
        while let Some(uri) = refresh.take_due(&present, None) {
            taken.push(uri);
        }
        taken.sort();
        assert_eq!(
            taken,
            listed(&[A, B]),
            "draining must pop each due uri once, so a drain loop terminates"
        );
        assert_eq!(
            refresh.take_due(&present, None),
            None,
            "a drained set has nothing left to take"
        );
    }

    #[test]
    fn asked_moves_a_due_uri_to_awaiting() {
        let mut refresh = CoverRefresh::default();
        refresh.drop_for(A);
        refresh.asked(A);
        assert!(
            refresh.answer(A),
            "a due uri whose read was issued must be awaiting its reply"
        );
        assert_eq!(
            refresh.take_due(&listed(&[A]), None),
            None,
            "an asked uri must no longer be due"
        );
    }

    #[test]
    fn asked_is_a_no_op_for_a_uri_not_due() {
        let mut refresh = CoverRefresh::default();
        refresh.asked(A);
        assert!(
            !refresh.answer(A),
            "an ask for a uri that was not due must not await a reply"
        );
    }

    #[test]
    fn answer_is_true_once_for_an_awaited_uri_and_false_after() {
        let mut refresh = CoverRefresh::default();
        refresh.drop_for(A);
        refresh.asked(A);
        assert!(refresh.answer(A), "the first reply is the awaited one");
        assert!(
            !refresh.answer(A),
            "a second reply for one ask is not awaited"
        );
    }

    #[test]
    fn answer_is_false_for_a_uri_never_asked() {
        let mut refresh = CoverRefresh::default();
        assert!(
            !refresh.answer(A),
            "a reply for a uri never asked must not be taken as awaited"
        );
    }

    #[test]
    fn two_asks_for_one_playlist_answer_once() {
        let mut refresh = CoverRefresh::default();
        refresh.drop_for(A);
        refresh.asked(A);
        refresh.drop_for(A);
        refresh.asked(A);
        assert!(
            !refresh.answer(A),
            "the first of two replies for one playlist is superseded, not the answer"
        );
        assert!(
            refresh.answer(A),
            "the last reply for one playlist is the answer"
        );
        assert!(
            !refresh.answer(A),
            "a reply past the last ask is not awaited"
        );
    }

    #[test]
    fn take_due_is_none_when_nothing_is_due() {
        let mut refresh = CoverRefresh::default();
        refresh.drop_for(A);
        refresh.asked(A);
        assert_eq!(
            refresh.take_due(&listed(&[A, B]), None),
            None,
            "an awaiting uri is not due, so there is nothing to take"
        );
        assert!(
            refresh.answer(A),
            "taking nothing must leave the ask in flight"
        );
    }

    #[test]
    fn take_due_takes_the_second_due_uri_when_the_first_is_kept() {
        let mut refresh = CoverRefresh::default();
        refresh.drop_for(A);
        refresh.drop_for(B);
        assert_eq!(
            refresh.take_due(&listed(&[A, B]), Some(A)),
            Some(B.to_owned()),
            "the kept page's own read answers its cover, so the next due uri is taken"
        );
    }

    #[test]
    fn take_due_follows_listing_order() {
        let mut refresh = CoverRefresh::default();
        refresh.drop_for(A);
        refresh.drop_for(C);
        assert_eq!(
            refresh.take_due(&listed(&[C, A]), None),
            Some(C.to_owned()),
            "the listing decides which due uri is taken first"
        );
    }

    #[test]
    fn retain_prunes_both_sets() {
        let mut refresh = CoverRefresh::default();
        refresh.drop_for(A);
        refresh.drop_for(C);
        refresh.drop_for(B);
        refresh.asked(B);
        refresh.retain(&listed(&[A]));
        assert_eq!(
            refresh.take_due(&listed(&[A, B, C]), None),
            Some(A.to_owned()),
            "a listed due uri must survive the prune"
        );
        assert_eq!(
            refresh.take_due(&listed(&[A, B, C]), None),
            None,
            "an unlisted due uri must be pruned"
        );
        assert!(
            !refresh.answer(B),
            "an unlisted awaiting uri must be pruned"
        );
    }

    #[test]
    fn forget_removes_from_both() {
        let mut refresh = CoverRefresh::default();
        refresh.drop_for(A);
        refresh.drop_for(B);
        refresh.asked(B);
        refresh.forget(A);
        refresh.forget(B);
        assert_eq!(
            refresh.take_due(&listed(&[A, B]), None),
            None,
            "a forgotten uri must not be due"
        );
        assert!(
            !refresh.answer(B),
            "a forgotten uri must not be awaiting a reply"
        );
    }

    #[test]
    fn first_track_uri_skips_episodes_and_locals() {
        let rows = [
            track("spotify:episode:e"),
            track("spotify:local:artist:album:title:100"),
            track("spotify:track:t1"),
            track("spotify:track:t2"),
        ];
        assert_eq!(
            first_track_uri(&rows),
            Some("spotify:track:t1"),
            "the first row a playlist can hold is the one a cover comes from"
        );
    }

    #[test]
    fn first_track_uri_is_none_for_an_empty_slice() {
        assert_eq!(
            first_track_uri(&[]),
            None,
            "an empty list has no first track, so no derived cover"
        );
    }

    #[test]
    fn first_track_uri_is_none_when_every_row_is_not_a_track() {
        let rows = [
            track("spotify:episode:e"),
            track("spotify:local:artist:album:title:100"),
        ];
        assert_eq!(
            first_track_uri(&rows),
            None,
            "a playlist of episodes and local files has no track a cover comes from"
        );
    }
}
