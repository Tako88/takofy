//! Decoding cover bytes into tightly packed RGBA pixels.
//!
//! The JPEG limits are attached to [`image::ImageReader`] and applied by
//! `decode`, which checks the dimensions before allocating, so a
//! decompression bomb is refused. The work runs on a detached thread (the
//! caller's job), never the runtime.

use image::Limits;

use takofy_model::CoverImage;

use crate::cover::CoverError;

/// The longest edge kept after downscaling, in pixels.
pub(crate) const MAX_EDGE: u32 = 1024;

/// The decode limits: a 4096-pixel edge and at most 64 MiB allocated. The
/// input itself is already bounded by `MAX_COVER_BYTES`.
fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    limits
}

/// Decodes JPEG bytes under the settled limits.
pub(crate) fn decode(bytes: &[u8]) -> Result<CoverImage, CoverError> {
    decode_with(bytes, limits())
}

/// Decodes with an explicit limit set, so a test can prove the limits run on
/// the real decode path rather than only being stored.
fn decode_with(bytes: &[u8], limits: Limits) -> Result<CoverImage, CoverError> {
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Jpeg);
    // `&mut self` stores the limits; `decode` then calls `limits.reserve` and
    // `decoder.set_limits` before any pixels are allocated.
    reader.limits(limits);
    let decoded = reader.decode().map_err(|_| CoverError::Decode)?;
    let scaled = downscale(decoded);
    CoverImage::new(scaled.width(), scaled.height(), scaled.into_raw()).ok_or(CoverError::Decode)
}

/// Shrinks a decoded image so its longer edge is at most [`MAX_EDGE`], then
/// converts to RGBA. The resize runs first so the up-to-4096² source never
/// becomes a full RGBA8 buffer: that conversion allocates 64 MiB outside the
/// decoder's `max_alloc`, and a flat JPEG that size is only tens of KB. Within
/// the bound the RGBA8 buffer is at most 4 MiB.
fn downscale(image: image::DynamicImage) -> image::RgbaImage {
    let (width, height) = (image.width(), image.height());
    let longest = width.max(height);
    if longest <= MAX_EDGE {
        return image.to_rgba8();
    }
    let ratio = f64::from(MAX_EDGE) / f64::from(longest);
    let target_width = ((f64::from(width) * ratio).round() as u32).max(1);
    let target_height = ((f64::from(height) * ratio).round() as u32).max(1);
    image::imageops::resize(
        &image,
        target_width,
        target_height,
        image::imageops::FilterType::Triangle,
    )
}

#[cfg(test)]
mod tests {
    use super::{MAX_EDGE, decode, decode_with, limits};

    /// The committed fixture: 639 bytes, 16x8, red on the left and blue on the
    /// right. Asserting its shape here is what makes a fixture swap fail.
    const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/cover.jpg");

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
}
