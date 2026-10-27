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
mod tests;
