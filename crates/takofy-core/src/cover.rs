//! Choosing a cover URL from an album's image group.
//!
//! Pure: the wire's shapes in, a checked [`CoverUrl`] or nothing out. The
//! single `cover` field on an album is never populated, so only the group is
//! read.
//!
//! Two wrappers carry images: the protocol `ImageGroup` a context names, and the
//! metadata `Images` an album's `cover_group` converts to. They differ only in
//! how a file id is reached — a protocol `Image` hands out its raw bytes, a
//! metadata `Image` holds a [`FileId`] — so the ranking rule lives once and the
//! two adapters only unpack their own shape.

use librespot::metadata::image::Images;
use librespot::protocol::metadata::ImageGroup;
use librespot::protocol::metadata::image::Size;
use takofy_model::CoverUrl;

/// The tile width the fallback chooser aims for, in pixels.
pub(crate) const TARGET_WIDTH: i32 = 300;

/// The cover a group's tiles should use.
///
/// `Size::DEFAULT` wins; otherwise the image nearest [`TARGET_WIDTH`], then the
/// first image. A candidate whose file id is malformed or zero is skipped and
/// the next one is tried.
pub(crate) fn pick_cover(group: Option<&ImageGroup>) -> Option<CoverUrl> {
    let images = group?.image.as_slice();
    let candidates = images
        .iter()
        .map(|image| {
            (
                image.size() == Size::DEFAULT,
                image.width(),
                image.file_id(),
            )
        })
        .collect::<Vec<_>>();
    CoverUrl::from_file_id(choose(ranked(&candidates))?)
}

/// The cover an album's or an artist's `Images` should use.
///
/// The same rule [`pick_cover`] applies, reading the metadata wrapper instead of
/// the protocol one: an album's `cover_group` and an artist's `portrait_group`
/// are both `Images`, and both want the default tile or the width nearest the
/// target.
///
/// A caveat the wrapper cannot recover from: `FileId::from_raw` zero-pads an id
/// shorter than twenty bytes, so a padded id looks real here and its CDN URL
/// answers 404. The band then draws no art, which is the outcome skipping it
/// would have had.
pub(crate) fn pick_images_cover(images: &Images) -> Option<CoverUrl> {
    let candidates = images
        .iter()
        .map(|image| (image.size == Size::DEFAULT, image.width, &image.id.0[..]))
        .collect::<Vec<_>>();
    CoverUrl::from_file_id(choose(ranked(&candidates))?)
}

/// One image as the chooser sees it: whether the wire called it the default, its
/// width, and its raw file id. Raw bytes rather than a URL, so both wrappers
/// feed one rule.
type Candidate<'a> = (bool, i32, &'a [u8]);

/// The candidates in preference order: the default size, then the image nearest
/// [`TARGET_WIDTH`], then the wire order.
///
/// Every default is kept rather than only the first, so a default whose id is
/// unusable falls through to another default before it falls back to the width
/// rule. The repeats are harmless: [`choose`] reads the list head first.
fn ranked<'a>(images: &'a [Candidate<'a>]) -> impl Iterator<Item = (i32, &'a [u8])> + 'a {
    let nearest = images
        .iter()
        .min_by_key(|(_, width, _)| width.abs_diff(TARGET_WIDTH));
    images
        .iter()
        .filter(|(is_default, _, _)| *is_default)
        .chain(nearest)
        .chain(images.iter())
        .map(|(_, width, id)| (*width, *id))
}

/// The first candidate id [`CoverUrl::from_file_id`] accepts, as raw bytes.
///
/// The one rule both adapters share: a short, empty or all-zero id is not an
/// image, so it is skipped rather than drawn.
fn choose<'a>(candidates: impl Iterator<Item = (i32, &'a [u8])>) -> Option<&'a [u8]> {
    candidates
        .map(|(_, id)| id)
        .find(|id| CoverUrl::from_file_id(id).is_some())
}

#[cfg(test)]
mod tests {
    use librespot::metadata::image::Images;
    use librespot::protocol::metadata::image::Size;
    use librespot::protocol::metadata::{Album, Image, ImageGroup};

    use super::{TARGET_WIDTH, pick_images_cover, pick_cover};

    fn image(size: Size, width: i32, id: &[u8]) -> Image {
        let mut image = Image::new();
        image.set_size(size);
        image.set_width(width);
        image.set_file_id(id.to_vec());
        image
    }

    fn group(images: Vec<Image>) -> ImageGroup {
        ImageGroup {
            image: images,
            ..Default::default()
        }
    }

    fn id(byte: u8) -> Vec<u8> {
        vec![byte; 20]
    }

    #[test]
    fn a_group_with_only_default_picks_it() {
        let group = group(vec![image(Size::DEFAULT, TARGET_WIDTH, &id(1))]);
        let cover = pick_cover(Some(&group)).expect("a cover");
        assert!(
            cover
                .as_str()
                .ends_with("0101010101010101010101010101010101010101")
        );
    }

    #[test]
    fn a_group_with_no_default_picks_the_nearest_width() {
        // 64 is nearer 300 than 640 is, so the small tile wins.
        let group = group(vec![
            image(Size::LARGE, 640, &id(2)),
            image(Size::SMALL, 64, &id(3)),
        ]);
        let cover = pick_cover(Some(&group)).expect("a cover");
        assert!(
            cover
                .as_str()
                .ends_with("0303030303030303030303030303030303030303"),
            "the nearest width must win: {cover:?}"
        );
    }

    #[test]
    fn a_malformed_default_is_skipped_for_the_next_candidate() {
        let group = group(vec![
            image(Size::DEFAULT, TARGET_WIDTH, &[1; 16]),
            image(Size::LARGE, 640, &id(4)),
        ]);
        let cover = pick_cover(Some(&group)).expect("a cover");
        assert!(
            cover
                .as_str()
                .ends_with("0404040404040404040404040404040404040404")
        );
    }

    #[test]
    fn a_zero_default_is_skipped_for_the_next_candidate() {
        let group = group(vec![
            image(Size::DEFAULT, TARGET_WIDTH, &[0; 20]),
            image(Size::LARGE, 640, &id(5)),
        ]);
        let cover = pick_cover(Some(&group)).expect("a cover");
        assert!(
            cover
                .as_str()
                .ends_with("0505050505050505050505050505050505050505")
        );
    }

    #[test]
    fn no_group_is_no_cover() {
        assert_eq!(pick_cover(None), None);
    }

    #[test]
    fn an_empty_group_is_no_cover() {
        assert_eq!(pick_cover(Some(&group(Vec::new()))), None);
    }

    #[test]
    fn a_present_but_empty_cover_field_is_ignored() {
        // Only `cover_group` is read; the `cover` field is dead on the wire.
        let album = Album {
            cover: vec![image(Size::DEFAULT, TARGET_WIDTH, &id(6))],
            ..Default::default()
        };
        assert_eq!(pick_cover(album.cover_group.as_ref()), None);
    }

    /// A metadata `Images` the way an album's `cover_group` converts one.
    fn album_images(images: Vec<Image>) -> Images {
        Images::from(&group(images))
    }

    #[test]
    fn a_default_album_tile_wins_over_a_nearer_width() {
        // 640 is nearer 300 than 64 is, but the wire called 64 the default.
        let images = album_images(vec![
            image(Size::LARGE, 640, &id(7)),
            image(Size::DEFAULT, 64, &id(8)),
        ]);
        let cover = pick_images_cover(&images).expect("a cover");
        assert!(
            cover
                .as_str()
                .ends_with("0808080808080808080808080808080808080808"),
            "the default tile must win: {cover:?}"
        );
    }

    #[test]
    fn an_album_with_no_default_picks_the_nearest_width() {
        let images = album_images(vec![
            image(Size::LARGE, 640, &id(9)),
            image(Size::SMALL, 64, &id(10)),
        ]);
        let cover = pick_images_cover(&images).expect("a cover");
        assert!(
            cover
                .as_str()
                .ends_with("0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a"),
            "the nearest width must win: {cover:?}"
        );
    }

    #[test]
    fn an_album_of_equidistant_tiles_picks_the_wires_first() {
        // 200 and 400 are both 100 from the target width, so the tie is broken by
        // the wire's own order.
        let images = album_images(vec![
            image(Size::LARGE, 400, &id(11)),
            image(Size::LARGE, 200, &id(12)),
        ]);
        let cover = pick_images_cover(&images).expect("a cover");
        assert!(
            cover
                .as_str()
                .ends_with("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b"),
            "a tie is broken by wire order: {cover:?}"
        );
    }

    #[test]
    fn a_zero_album_id_is_skipped_for_the_next_candidate() {
        let images = album_images(vec![
            image(Size::DEFAULT, TARGET_WIDTH, &[0; 20]),
            image(Size::LARGE, 640, &id(13)),
        ]);
        let cover = pick_images_cover(&images).expect("a cover");
        assert!(
            cover
                .as_str()
                .ends_with("0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d"),
            "a zero id is padding, not an image: {cover:?}"
        );
    }

    /// The documented wart: `FileId::from_raw` pads a short id, so a 16-byte wire
    /// id becomes a 20-byte one that passes here and answers 404 at the CDN. The
    /// wrapper cannot tell the padding from a real zero byte, so this pins the
    /// behaviour rather than pretending to reject it.
    #[test]
    fn a_short_album_id_is_padded_and_accepted() {
        let images = album_images(vec![image(Size::DEFAULT, TARGET_WIDTH, &[3; 16])]);
        let cover = pick_images_cover(&images).expect("a padded id still reads as an id");
        assert!(
            cover
                .as_str()
                .ends_with(&format!("{}{}", "03".repeat(16), "00".repeat(4))),
            "the padding is visible in the URL: {cover:?}"
        );
    }

    #[test]
    fn an_empty_album_group_is_no_cover() {
        assert_eq!(pick_images_cover(&album_images(Vec::new())), None);
    }
}
