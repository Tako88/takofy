//! Which covers a playlist band draws as its mosaic, and when.
//!
//! The band shows a mosaic only when the list has two or more distinct album
//! covers and every one of them is already held. A mosaic that swapped in one
//! quadrant at a time would flicker while the fetches land, so it swaps whole and
//! falls back to the playlist's own cover until then.
//!
//! Nothing here touches the window: the selection is a function of the track
//! list, and the arrangement a function of the covers the store answered with.

use takofy_model::{CoverOrigin, CoverUrl, TrackEntry, composed_from};

use crate::cover::Cover;

/// How many quadrants a mosaic has.
pub const QUADRANTS: usize = 4;

/// The first four distinct covers in track order, by the model's one rule.
///
/// The band's own origin is checked by [`wanted`]: a chosen picture is never
/// replaced, so picking covers does not need to know where they came from.
pub fn urls(tracks: &[TrackEntry]) -> Vec<CoverUrl> {
    composed_from(None, tracks.iter().map(|track| track.cover.as_ref()))
}

/// The covers worth fetching for a mosaic.
///
/// Empty when the playlist carries a cover a user chose, because that picture is
/// the list's own and a mosaic would replace it. A list with fewer than two
/// covers has already composed nothing.
pub fn wanted(origin: Option<CoverOrigin>, urls: &[CoverUrl]) -> Vec<CoverUrl> {
    if origin == Some(CoverOrigin::Chosen) {
        return Vec::new();
    }
    urls.to_vec()
}

/// The four quadrants the looked-at covers fill, when every one of them is ready.
///
/// Fewer than four covers still tile: the remaining quadrants are holes. A single
/// cover that is still loading holds the whole mosaic back, so the band keeps its
/// single picture until the swap can happen in one frame.
pub fn arrange(covers: &[Cover]) -> Option<[Cover; QUADRANTS]> {
    if covers.is_empty() || covers.iter().any(|cover| !matches!(cover, Cover::Ready(_))) {
        return None;
    }
    let mut quadrants = [Cover::Missing; QUADRANTS];
    for (quadrant, cover) in quadrants.iter_mut().zip(covers) {
        *quadrant = *cover;
    }
    Some(quadrants)
}

/// The covers the last ready list wanted, and which list they belonged to.
///
/// A reload puts the tracks back into `Loading`; this memo lets the band keep the
/// mosaic it already holds instead of falling back to one cover and fetching the
/// four again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    playlist: String,
    urls: Vec<CoverUrl>,
}

impl Kept {
    /// Remembers `urls` as `playlist`'s mosaic.
    pub fn new(playlist: String, urls: Vec<CoverUrl>) -> Self {
        Self { playlist, urls }
    }

    /// The URLs this memo holds for `playlist`.
    ///
    /// Another list's covers are none of this page's business, so they answer
    /// nothing rather than leaking across a page change.
    pub fn urls_for(&self, playlist: &str) -> Option<&[CoverUrl]> {
        (self.playlist == playlist).then_some(self.urls.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use egui::TextureId;
    use takofy_model::{CoverOrigin, CoverUrl, TrackEntry};

    use super::*;

    fn url(tail: &str) -> CoverUrl {
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
    }

    fn track(cover: &str) -> TrackEntry {
        TrackEntry {
            uri: format!("spotify:track:{cover}"),
            title: cover.to_owned(),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            artist_names: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: true,
            cover: Some(url(cover)),
        }
    }

    fn bare(tail: &str) -> TrackEntry {
        TrackEntry {
            cover: None,
            ..track(tail)
        }
    }

    fn ready() -> Cover {
        Cover::Ready(TextureId::Managed(1))
    }

    #[test]
    fn the_first_four_distinct_covers_in_track_order() {
        let tracks = [track("a"), track("b"), track("c"), track("d"), track("e")];
        assert_eq!(urls(&tracks), vec![url("a"), url("b"), url("c"), url("d")]);
    }

    #[test]
    fn a_repeated_album_counts_once() {
        let tracks = [track("a"), track("b"), track("a"), track("c")];
        assert_eq!(urls(&tracks), vec![url("a"), url("b"), url("c")]);
    }

    #[test]
    fn tracks_without_a_cover_are_skipped() {
        let tracks = [bare("x"), track("a"), bare("y"), track("b")];
        assert_eq!(urls(&tracks), vec![url("a"), url("b")]);
    }

    #[test]
    fn a_scan_stops_at_four() {
        let tracks: Vec<TrackEntry> = (0..8).map(|index| track(&format!("c{index}"))).collect();
        assert_eq!(urls(&tracks).len(), QUADRANTS);
    }

    #[test]
    fn one_cover_is_no_mosaic() {
        assert!(urls(&[track("a")]).is_empty());
        assert!(urls(&[track("a"), track("a")]).is_empty());
        assert!(wanted(Some(CoverOrigin::Derived), &urls(&[track("a")])).is_empty());
    }

    #[test]
    fn two_covers_are_a_mosaic() {
        assert_eq!(
            wanted(Some(CoverOrigin::Derived), &[url("a"), url("b")]),
            vec![url("a"), url("b")]
        );
    }

    #[test]
    fn a_chosen_cover_is_never_a_mosaic() {
        assert!(
            wanted(
                Some(CoverOrigin::Chosen),
                &[url("a"), url("b"), url("c"), url("d")]
            )
            .is_empty()
        );
    }

    #[test]
    fn a_derived_or_absent_origin_may_be_a_mosaic() {
        let found = vec![url("a"), url("b")];
        assert_eq!(wanted(Some(CoverOrigin::Derived), &found), found);
        assert_eq!(wanted(None, &found), found);
    }

    #[test]
    fn arrange_is_none_until_every_cover_is_ready() {
        assert_eq!(arrange(&[ready(), Cover::Loading]), None);
        assert_eq!(arrange(&[ready(), Cover::Failed]), None);
        assert_eq!(arrange(&[Cover::Loading, ready()]), None);
    }

    #[test]
    fn arrange_pads_missing_quadrants_with_holes() {
        let arranged = arrange(&[ready(), ready()]).expect("two ready covers tile");
        assert_eq!(arranged, [ready(), ready(), Cover::Missing, Cover::Missing]);
    }

    #[test]
    fn arrange_is_none_for_no_covers() {
        assert_eq!(arrange(&[]), None);
    }

    #[test]
    fn kept_answers_only_for_its_own_playlist() {
        let kept = Kept::new("spotify:playlist:a".to_owned(), vec![url("a"), url("b")]);
        assert_eq!(
            kept.urls_for("spotify:playlist:a"),
            Some([url("a"), url("b")].as_slice())
        );
        assert_eq!(kept.urls_for("spotify:playlist:b"), None);
    }
}
