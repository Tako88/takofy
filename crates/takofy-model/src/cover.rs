//! Sized covers: every image the wire offered for one cover, and the pick.
//!
//! A cover carries one URL per size the wire named, each with its width and
//! height (some albums ship a wide pair rather than a square). A draw site says
//! how many device pixels its box is and gets the smallest image that covers it.

use crate::CoverUrl;

/// The widest box, in device pixels, that still takes the smallest image.
///
/// Below this width the cheapest size is what a small tile is for, and the cost
/// is that a box in roughly 100–128 px draws the smallest image up to twice its
/// size. Above it the image has to cover the box.
pub const SMALL_MAX_PX: u32 = 128;

/// One size the wire offered for a cover, with its URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverVariant {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub url: CoverUrl,
}

impl CoverVariant {
    /// The square size this variant can fill: its shorter side.
    ///
    /// A square box takes a wide pair by its height rather than its width.
    pub fn effective(&self) -> Option<u32> {
        Some(self.width?.min(self.height?))
    }
}

/// The sizes offered for one cover, sorted by [`CoverVariant::effective`].
///
/// A set is never empty and holds at most one variant per effective size, so a
/// pick cannot flip between frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverSet(Vec<CoverVariant>);

impl From<CoverUrl> for CoverSet {
    fn from(url: CoverUrl) -> Self {
        Self::single(url)
    }
}

impl CoverSet {
    /// Builds the set the wire offered, dropping unusable entries.
    ///
    /// A variant with a zero side has no size and is dropped. The rest sort by
    /// effective size, an unknown size last, and the first of an equal size
    /// wins. `None` when nothing usable is left.
    pub fn from_sizes(
        sizes: impl IntoIterator<Item = (Option<(u32, u32)>, CoverUrl)>,
    ) -> Option<Self> {
        let mut variants: Vec<CoverVariant> = sizes
            .into_iter()
            .filter(|(size, _)| size.is_none_or(|(width, height)| width > 0 && height > 0))
            .map(|(size, url)| match size {
                Some((width, height)) => CoverVariant {
                    width: Some(width),
                    height: Some(height),
                    url,
                },
                None => CoverVariant {
                    width: None,
                    height: None,
                    url,
                },
            })
            .collect();
        // A stable sort keeps the wire's order among equal sizes, so the first
        // one offered is the one kept.
        variants.sort_by_key(|variant| match variant.effective() {
            Some(effective) => (0, effective),
            None => (1, 0),
        });
        variants.dedup_by_key(|variant| variant.effective());
        if variants.is_empty() {
            None
        } else {
            Some(Self(variants))
        }
    }

    /// A set of one URL with no sizes: a chosen picture the wire named bare.
    pub fn single(url: CoverUrl) -> Self {
        Self(vec![CoverVariant {
            width: None,
            height: None,
            url,
        }])
    }

    /// The variants, smallest first.
    pub fn variants(&self) -> &[CoverVariant] {
        &self.0
    }

    /// The URL to draw in a box `want_px` device pixels wide.
    ///
    /// A small box takes the smallest variant; anything larger takes the
    /// smallest variant that covers it, or the largest sized one when none does.
    /// A variant of unknown size is picked only when the set has no size at all.
    pub fn pick(&self, want_px: u32) -> Option<&CoverUrl> {
        let smallest = self.0.first()?;
        if want_px <= SMALL_MAX_PX {
            return Some(&smallest.url);
        }
        self.0
            .iter()
            .find(|variant| variant.effective().is_some_and(|size| size >= want_px))
            .or_else(|| {
                self.0
                    .iter()
                    .rev()
                    .find(|variant| variant.effective().is_some())
            })
            .or_else(|| self.0.last())
            .map(|variant| &variant.url)
    }
}

#[cfg(test)]
mod tests;
