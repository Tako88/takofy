//! Choosing a cover URL from an album's image group.
//!
//! Pure: the wire's shapes in, a checked [`CoverUrl`] or nothing out. The
//! single `cover` field on an album is never populated, so only the group is
//! read.

use librespot::protocol::metadata::image::Size;
use librespot::protocol::metadata::{Image, ImageGroup};
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
    let mut candidates: Vec<&Image> = images
        .iter()
        .filter(|image| image.size() == Size::DEFAULT)
        .collect();
    if let Some(nearest) = nearest_width(images) {
        candidates.push(nearest);
    }
    candidates.extend(images.iter());
    candidates
        .into_iter()
        .find_map(|image| CoverUrl::from_file_id(image.file_id()))
}

/// The image whose width is nearest [`TARGET_WIDTH`].
fn nearest_width(images: &[Image]) -> Option<&Image> {
    images
        .iter()
        .min_by_key(|image| image.width().abs_diff(TARGET_WIDTH))
}

#[cfg(test)]
mod tests {
    use librespot::protocol::metadata::image::Size;
    use librespot::protocol::metadata::{Album, Image, ImageGroup};

    use super::{TARGET_WIDTH, pick_cover};

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
}
