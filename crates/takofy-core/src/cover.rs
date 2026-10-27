//! Reading the sizes a cover group offers.
//!
//! Pure: the wire's shapes in, a checked [`CoverSet`] or nothing out. The
//! single `cover` field on an album is never populated, so only the group is
//! read.
//!
//! Two wrappers carry images: the protocol `ImageGroup` a context names, and the
//! metadata `Images` an album's `cover_group` converts to. They differ only in
//! how a file id is reached — a protocol `Image` hands out its raw bytes, a
//! metadata `Image` holds a [`FileId`] — so the adapters only unpack their own
//! shape and hand the rest to [`CoverSet`].

use librespot::metadata::image::Images;
use librespot::protocol::metadata::ImageGroup;
use takofy_model::{CoverSet, CoverUrl};

/// Every usable size a protocol group offers, smallest first.
///
/// A variant whose file id is malformed or all-zero is skipped; the rest keep
/// the size the wire gave them.
pub(crate) fn covers_in_group(group: Option<&ImageGroup>) -> Option<CoverSet> {
    let images = group?.image.as_slice();
    CoverSet::from_sizes(images.iter().filter_map(|image| {
        let url = CoverUrl::from_file_id(image.file_id())?;
        Some((size_of(image.width(), image.height()), url))
    }))
}

/// Every usable size an album's or an artist's `Images` offers, smallest first.
///
/// The same rule [`covers_in_group`] applies, reading the metadata wrapper
/// instead of the protocol one.
///
/// A caveat the wrapper cannot recover from: `FileId::from_raw` zero-pads an id
/// shorter than twenty bytes, so a padded id looks real here and its CDN URL
/// answers 404. The band then draws no art, which is the outcome skipping it
/// would have had.
pub(crate) fn covers_in_images(images: &Images) -> Option<CoverSet> {
    CoverSet::from_sizes(images.iter().filter_map(|image| {
        let url = CoverUrl::from_file_id(&image.id.0[..])?;
        Some((size_of(image.width, image.height), url))
    }))
}

/// The drawn size of one wire image, when the wire gave a usable one.
///
/// A zero side is no measurement: the known side fills the square box. A size
/// with no known side at all is no size, because a pixel count that is not
/// positive says nothing about how the image fills one.
fn size_of(width: i32, height: i32) -> Option<(u32, u32)> {
    let width = u32::try_from(width).ok().filter(|width| *width > 0);
    let height = u32::try_from(height).ok().filter(|height| *height > 0);
    match (width, height) {
        (Some(width), Some(height)) => Some((width, height)),
        (Some(width), None) => Some((width, width)),
        (None, Some(height)) => Some((height, height)),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests;
