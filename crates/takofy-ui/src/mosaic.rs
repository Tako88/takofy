//! Which covers a playlist band draws as its mosaic, and when.
//!
//! The band shows a mosaic only when the list has two or more distinct album
//! covers and every one of them is already held. A mosaic that swapped in one
//! quadrant at a time would flicker while the fetches land, so it swaps whole and
//! falls back to the playlist's own cover until then.
//!
//! Nothing here touches the window: the selection is a function of the track
//! list, and the arrangement a function of the covers the store answered with.

use takofy_model::{CoverOrigin, CoverSet, TrackEntry, composed_from};

use crate::cover::Cover;

/// How many quadrants a mosaic has.
pub const QUADRANTS: usize = 4;

/// The first four distinct covers in track order, by the model's one rule.
///
/// The band's own origin is checked by [`wanted`]: a chosen picture is never
/// replaced, so picking covers does not need to know where they came from.
pub fn urls(tracks: &[TrackEntry]) -> Vec<CoverSet> {
    composed_from(None, tracks.iter().map(|track| track.cover.as_ref()))
}

/// The covers worth fetching for a mosaic.
///
/// Empty when the playlist carries a cover a user chose, because that picture is
/// the list's own and a mosaic would replace it. A list with fewer than two
/// covers has already composed nothing.
pub fn wanted(origin: Option<CoverOrigin>, urls: &[CoverSet]) -> Vec<CoverSet> {
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
    urls: Vec<CoverSet>,
}

impl Kept {
    /// Remembers `urls` as `playlist`'s mosaic.
    pub fn new(playlist: String, urls: Vec<CoverSet>) -> Self {
        Self { playlist, urls }
    }

    /// The URLs this memo holds for `playlist`.
    ///
    /// Another list's covers are none of this page's business, so they answer
    /// nothing rather than leaking across a page change.
    pub fn urls_for(&self, playlist: &str) -> Option<&[CoverSet]> {
        (self.playlist == playlist).then_some(self.urls.as_slice())
    }
}

#[cfg(test)]
mod tests;
