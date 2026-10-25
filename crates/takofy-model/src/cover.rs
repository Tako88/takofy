//! Sized covers: every image the wire offered for one cover, and the pick.
//!
//! A cover carries one URL per size the wire named, each with its width and
//! height (some albums ship a wide pair rather than a square). A draw site says
//! how many device pixels its box is and gets the smallest image that covers it.

use crate::CoverUrl;

/// The widest box, in device pixels, that still takes the smallest image.
///
/// A box no wider than twice the smallest size the wire offers cannot show the
/// difference, so it takes the cheapest image; past that the box is covered.
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
    /// smallest variant that covers it, or the largest when none does.
    pub fn pick(&self, want_px: u32) -> Option<&CoverUrl> {
        let smallest = self.0.first()?;
        if want_px <= SMALL_MAX_PX {
            return Some(&smallest.url);
        }
        self.0
            .iter()
            .find(|variant| variant.effective().is_some_and(|size| size >= want_px))
            .or_else(|| self.0.last())
            .map(|variant| &variant.url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(tail: &str) -> CoverUrl {
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
    }

    /// The album sizes: 64, 300 and 640 squares.
    fn album() -> CoverSet {
        CoverSet::from_sizes([
            (Some((64, 64)), url("small")),
            (Some((300, 300)), url("default")),
            (Some((640, 640)), url("large")),
        ])
        .expect("a cover set")
    }

    #[test]
    fn a_small_box_takes_the_smallest_variant() {
        let set = album();
        for want in [0, 36, 72, 80, 88, 112, SMALL_MAX_PX] {
            assert_eq!(set.pick(want), Some(&url("small")), "a {want}px box");
        }
    }

    #[test]
    fn a_box_past_the_threshold_takes_the_smallest_covering_variant() {
        let set = album();
        assert_eq!(set.pick(176), Some(&url("default")));
        assert_eq!(set.pick(224), Some(&url("default")));
        assert_eq!(set.pick(300), Some(&url("default")));
        assert_eq!(set.pick(352), Some(&url("large")));
        assert_eq!(set.pick(512), Some(&url("large")));
    }

    #[test]
    fn a_box_past_every_variant_takes_the_largest() {
        assert_eq!(album().pick(4000), Some(&url("large")));
    }

    #[test]
    fn a_single_variant_answers_a_large_box_too() {
        let set = CoverSet::from_sizes([(Some((64, 64)), url("small"))]).expect("a cover set");
        assert_eq!(set.pick(512), Some(&url("small")));
    }

    #[test]
    fn single_answers_any_box_with_its_one_url() {
        let set = CoverSet::single(url("picture"));
        assert_eq!(set.pick(36), Some(&url("picture")));
        assert_eq!(set.pick(4000), Some(&url("picture")));
        assert_eq!(set.variants().len(), 1);
        assert_eq!(set.variants()[0].effective(), None);
    }

    #[test]
    fn no_sizes_makes_no_set() {
        assert_eq!(CoverSet::from_sizes([]), None);
    }

    #[test]
    fn a_zero_dimension_is_dropped() {
        let set = CoverSet::from_sizes([
            (Some((0, 300)), url("zero-width")),
            (Some((300, 0)), url("zero-height")),
            (Some((300, 300)), url("default")),
        ])
        .expect("a cover set");
        assert_eq!(
            set.variants(),
            &[CoverVariant {
                width: Some(300),
                height: Some(300),
                url: url("default"),
            }]
        );
        assert_eq!(CoverSet::from_sizes([(Some((0, 0)), url("zero"))]), None);
    }

    #[test]
    fn duplicate_sizes_keep_the_first() {
        let set = CoverSet::from_sizes([
            (Some((300, 300)), url("first")),
            (Some((300, 300)), url("second")),
        ])
        .expect("a cover set");
        assert_eq!(set.variants().len(), 1);
        assert_eq!(set.variants()[0].url, url("first"));
        assert_eq!(set.pick(224), Some(&url("first")));
    }

    #[test]
    fn a_wide_pair_is_picked_by_its_height() {
        let set = CoverSet::from_sizes([
            (Some((1280, 720)), url("wide")),
            (Some((640, 360)), url("wide-small")),
        ])
        .expect("a cover set");
        assert_eq!(set.pick(352), Some(&url("wide-small")));
        assert_eq!(set.pick(720), Some(&url("wide")));
    }

    #[test]
    fn construction_order_does_not_change_the_pick() {
        let forward = CoverSet::from_sizes([
            (Some((64, 64)), url("small")),
            (Some((300, 300)), url("default")),
            (Some((640, 640)), url("large")),
        ])
        .expect("a cover set");
        let backward = CoverSet::from_sizes([
            (Some((640, 640)), url("large")),
            (Some((64, 64)), url("small")),
            (Some((300, 300)), url("default")),
        ])
        .expect("a cover set");

        for want in [36, SMALL_MAX_PX, 176, 224, 352, 512, 4000] {
            assert_eq!(forward.pick(want), backward.pick(want), "a {want}px box");
        }
        assert_eq!(forward, backward, "the same set, however it was built");
    }
}
