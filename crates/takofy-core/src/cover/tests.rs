use librespot::metadata::image::Images;
use librespot::protocol::metadata::image::Size;
use librespot::protocol::metadata::{Album, Image, ImageGroup};

use super::{covers_in_group, covers_in_images};

fn image(size: Size, width: i32, height: i32, id: &[u8]) -> Image {
    let mut image = Image::new();
    image.set_size(size);
    image.set_width(width);
    image.set_height(height);
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

fn tail(url: &takofy_model::CoverUrl) -> String {
    url.as_str()["https://i.scdn.co/image/".len()..].to_owned()
}

/// The album sizes: 64, 300 and 640 squares.
fn album() -> ImageGroup {
    group(vec![
        image(Size::DEFAULT, 300, 300, &id(1)),
        image(Size::SMALL, 64, 64, &id(2)),
        image(Size::LARGE, 640, 640, &id(3)),
    ])
}

#[test]
fn a_group_keeps_every_size_it_offered() {
    let set = covers_in_group(Some(&album())).expect("a cover set");
    let kept: Vec<(Option<u32>, String)> = set
        .variants()
        .iter()
        .map(|variant| (variant.effective(), tail(&variant.url)))
        .collect();
    assert_eq!(
        kept,
        vec![
            (Some(64), "02".repeat(20)),
            (Some(300), "01".repeat(20)),
            (Some(640), "03".repeat(20)),
        ]
    );
    assert_eq!(set.pick(36), Some(&set.variants()[0].url));
    assert_eq!(set.pick(352), Some(&set.variants()[2].url));
}

#[test]
fn a_group_of_one_default_keeps_that_one() {
    let group = group(vec![image(Size::DEFAULT, 300, 300, &id(1))]);
    let set = covers_in_group(Some(&group)).expect("a cover set");
    assert_eq!(set.variants().len(), 1);
    assert_eq!(set.variants()[0].effective(), Some(300));
    assert!(
        tail(&set.variants()[0].url).ends_with(&"01".repeat(20)),
        "the default tile is kept"
    );
}

#[test]
fn a_half_known_size_keeps_its_known_side() {
    let group = group(vec![
        image(Size::DEFAULT, 300, 0, &id(1)),
        image(Size::LARGE, 0, 640, &id(2)),
    ]);
    let set = covers_in_group(Some(&group)).expect("a cover set");
    let effective: Vec<Option<u32>> = set.variants().iter().map(|v| v.effective()).collect();
    assert_eq!(
        effective,
        vec![Some(300), Some(640)],
        "width and height both count"
    );
    assert_eq!(set.pick(352), Some(&set.variants()[1].url));
}

#[test]
fn a_group_with_no_default_keeps_every_width() {
    let group = group(vec![
        image(Size::LARGE, 640, 640, &id(2)),
        image(Size::SMALL, 64, 64, &id(3)),
    ]);
    let set = covers_in_group(Some(&group)).expect("a cover set");
    let sizes: Vec<Option<u32>> = set.variants().iter().map(|v| v.effective()).collect();
    assert_eq!(sizes, vec![Some(64), Some(640)], "smallest first");
}

#[test]
fn a_wide_pair_is_kept_by_its_shorter_side() {
    let group = group(vec![
        image(Size::XLARGE, 1280, 720, &id(2)),
        image(Size::LARGE, 640, 360, &id(3)),
    ]);
    let set = covers_in_group(Some(&group)).expect("a cover set");
    let effective: Vec<Option<u32>> = set.variants().iter().map(|v| v.effective()).collect();
    assert_eq!(effective, vec![Some(360), Some(720)]);
}

#[test]
fn a_malformed_id_is_skipped_for_the_next_candidate() {
    let group = group(vec![
        image(Size::DEFAULT, 300, 300, &[1; 16]),
        image(Size::LARGE, 640, 640, &id(4)),
    ]);
    let set = covers_in_group(Some(&group)).expect("a cover set");
    assert_eq!(set.variants().len(), 1);
    assert!(tail(&set.variants()[0].url).ends_with(&"04".repeat(20)));
}

#[test]
fn a_zero_id_is_skipped_for_the_next_candidate() {
    let group = group(vec![
        image(Size::DEFAULT, 300, 300, &[0; 20]),
        image(Size::LARGE, 640, 640, &id(5)),
    ]);
    let set = covers_in_group(Some(&group)).expect("a cover set");
    assert_eq!(set.variants().len(), 1);
    assert!(tail(&set.variants()[0].url).ends_with(&"05".repeat(20)));
}

#[test]
fn no_group_is_no_cover() {
    assert_eq!(covers_in_group(None), None);
}

#[test]
fn an_empty_group_is_no_cover() {
    assert_eq!(covers_in_group(Some(&group(Vec::new()))), None);
}

#[test]
fn a_group_of_unusable_ids_is_no_cover() {
    let group = group(vec![
        image(Size::DEFAULT, 300, 300, &[0; 20]),
        image(Size::LARGE, 640, 640, &[1; 16]),
    ]);
    assert_eq!(covers_in_group(Some(&group)), None);
}

#[test]
fn a_present_but_empty_cover_field_is_ignored() {
    // Only `cover_group` is read; the `cover` field is dead on the wire.
    let album = Album {
        cover: vec![image(Size::DEFAULT, 300, 300, &id(6))],
        ..Default::default()
    };
    assert_eq!(covers_in_group(album.cover_group.as_ref()), None);
}

/// A metadata `Images` the way an album's `cover_group` converts one.
fn album_images(images: Vec<Image>) -> Images {
    Images::from(&group(images))
}

#[test]
fn an_album_keeps_every_size_it_offered() {
    let images = album_images(vec![
        image(Size::LARGE, 640, 640, &id(7)),
        image(Size::DEFAULT, 300, 300, &id(8)),
        image(Size::SMALL, 64, 64, &id(9)),
    ]);
    let set = covers_in_images(&images).expect("a cover set");
    let effective: Vec<Option<u32>> = set.variants().iter().map(|v| v.effective()).collect();
    assert_eq!(effective, vec![Some(64), Some(300), Some(640)]);
}

#[test]
fn an_album_of_one_default_keeps_that_one() {
    let images = album_images(vec![image(Size::DEFAULT, 64, 64, &id(8))]);
    let set = covers_in_images(&images).expect("a cover set");
    assert_eq!(set.variants().len(), 1);
    assert_eq!(set.variants()[0].effective(), Some(64));
    assert!(tail(&set.variants()[0].url).ends_with(&"08".repeat(20)));
}

#[test]
fn a_zero_album_id_is_skipped_for_the_next_candidate() {
    let images = album_images(vec![
        image(Size::DEFAULT, 300, 300, &[0; 20]),
        image(Size::LARGE, 640, 640, &id(13)),
    ]);
    let set = covers_in_images(&images).expect("a cover set");
    assert_eq!(set.variants().len(), 1);
    assert!(
        tail(&set.variants()[0].url).ends_with(&"0d".repeat(20)),
        "a zero id is padding, not an image"
    );
}

/// The documented wart: `FileId::from_raw` pads a short id, so a 16-byte wire
/// id becomes a 20-byte one that passes here and answers 404 at the CDN. The
/// wrapper cannot tell the padding from a real zero byte, so this pins the
/// behaviour rather than pretending to reject it.
#[test]
fn a_short_album_id_is_padded_and_accepted() {
    let images = album_images(vec![image(Size::DEFAULT, 300, 300, &[3; 16])]);
    let set = covers_in_images(&images).expect("a padded id still reads as an id");
    assert!(
        tail(&set.variants()[0].url).ends_with(&format!("{}{}", "03".repeat(16), "00".repeat(4))),
        "the padding is visible in the URL"
    );
}

#[test]
fn an_album_with_no_sizes_is_no_cover() {
    assert_eq!(covers_in_images(&album_images(Vec::new())), None);
}
