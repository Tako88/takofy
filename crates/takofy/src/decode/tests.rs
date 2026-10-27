use super::{MAX_EDGE, decode, decode_with, limits};

/// The committed fixture: 639 bytes, 16x8, red on the left and blue on the
/// right. Asserting its shape here is what makes a fixture swap fail.
const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/cover.jpg");

fn pixel(image: &takofy_model::CoverImage, x: u32, y: u32) -> [u8; 4] {
    let index = ((y * image.width + x) * 4) as usize;
    let rgba = &image.rgba[index..index + 4];
    [rgba[0], rgba[1], rgba[2], rgba[3]]
}

fn jpeg(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]));
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(image)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Jpeg,
        )
        .expect("encode a test jpeg");
    bytes
}

#[test]
fn the_fixture_decodes_to_its_dimensions_and_colours() {
    assert_eq!(
        FIXTURE.len(),
        639,
        "the fixture must not be swapped silently"
    );
    let image = decode(FIXTURE).expect("the fixture decodes");
    assert_eq!((image.width, image.height), (16, 8));

    let left = pixel(&image, 0, 4);
    let right = pixel(&image, 15, 4);
    assert!(
        left[0] > 128 && left[0] > left[1] + 64 && left[0] > left[2] + 64,
        "the left half is red, not {left:?}"
    );
    assert!(
        right[2] > 128 && right[2] > right[0] + 64 && right[2] > right[1] + 64,
        "the right half is blue, not {right:?}"
    );
    assert_eq!(left[3], 255);
}

#[test]
fn a_truncated_fixture_fails_instead_of_panicking() {
    assert!(decode(&FIXTURE[..100]).is_err());
}

#[test]
fn an_image_within_the_bound_keeps_its_size() {
    let image = decode(&jpeg(300, 300)).expect("a 300x300 jpeg decodes");
    assert_eq!((image.width, image.height), (300, 300));
}

#[test]
fn a_large_image_is_downscaled_to_the_max_edge() {
    let image = decode(&jpeg(2660, 1496)).expect("a large jpeg decodes");
    assert_eq!(image.width.max(image.height), MAX_EDGE);
    assert_eq!((image.width, image.height), (MAX_EDGE, 576));
}

#[test]
fn the_limits_are_enforced_on_the_real_decode_path() {
    let mut narrow = limits();
    narrow.max_image_width = Some(8);
    assert!(decode_with(FIXTURE, narrow).is_err());

    let mut tiny = limits();
    tiny.max_alloc = Some(16);
    assert!(decode_with(FIXTURE, tiny).is_err());

    assert!(decode_with(FIXTURE, limits()).is_ok());
}
