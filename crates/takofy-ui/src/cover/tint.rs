//! The band's colour, derived from the pixels of the image it drew.
//!
//! The mean of the image's opaque pixels, read in HSL, is normalised to a tone
//! that reads as a tinted header: a saturation floor because a photograph's mean
//! is close to neutral, and a lightness stepped down until the band's luminance
//! clears the ink drawn on it. A bright yellow at the same HSL lightness is far
//! brighter than a blue, so the step alone cannot be trusted.
//!
//! The mean is what the store keeps: the arithmetic is a handful of floats, so
//! another palette can normalise the same mean without fetching the cover again.

use crate::theme::TintParams;

/// The mean of an image's opaque pixels, in 0–255 RGB.
pub type Mean = [f64; 3];

/// The mean colour of the pixels whose alpha is at least `alpha_floor`.
///
/// Bytes are read as RGBA quadruples; a trailing partial pixel is ignored. `None`
/// when no pixel clears the floor, since an image with nothing opaque has no
/// colour to derive.
pub fn mean(rgba: &[u8], alpha_floor: u8) -> Option<Mean> {
    let mut sum = [0.0; 3];
    let mut count = 0u64;
    for pixel in rgba.as_chunks::<4>().0 {
        if pixel[3] < alpha_floor {
            continue;
        }
        for (total, channel) in sum.iter_mut().zip(pixel) {
            *total += f64::from(*channel);
        }
        count += 1;
    }
    if count == 0 {
        return None;
    }
    let count = count as f64;
    Some([sum[0] / count, sum[1] / count, sum[2] / count])
}

/// The band's colour for `mean`, as 0–255 RGB.
pub fn normalise(mean: Mean, params: &TintParams) -> [u8; 3] {
    let rgb = [mean[0] / 255.0, mean[1] / 255.0, mean[2] / 255.0];
    let (hue, saturation, lightness) = to_hsl(rgb);
    let tone = lightness.clamp(params.tone_min, params.tone_max);
    let lightness = params.tone_offset + params.tone_slope * tone;
    let saturation = if saturation < params.neutral_below {
        0.0
    } else {
        (saturation * params.saturation_gain).clamp(params.saturation_min, params.saturation_max)
    };
    to_bytes(settle(hue, saturation, lightness, params))
}

/// The colour at `lightness`, stepped down while it is brighter than the
/// ceiling.
///
/// The test is on the current lightness before the step, so the last step may
/// land below the floor rather than the loop running forever at it.
fn settle(hue: f64, saturation: f64, mut lightness: f64, params: &TintParams) -> [f64; 3] {
    let mut rgb = hsl_to_rgb(hue, saturation, lightness);
    while lightness > params.lightness_floor && relative_luminance(rgb) > params.luminance_ceiling {
        lightness -= params.lightness_step;
        rgb = hsl_to_rgb(hue, saturation, lightness);
    }
    rgb
}

fn to_bytes(rgb: [f64; 3]) -> [u8; 3] {
    rgb.map(|channel| (channel * 255.0).round().clamp(0.0, 255.0) as u8)
}

fn to_hsl([red, green, blue]: [f64; 3]) -> (f64, f64, f64) {
    let max = red.max(green).max(blue);
    let min = red.min(green).min(blue);
    let lightness = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, lightness);
    }
    let delta = max - min;
    // The true lightness picks the formula, before any tone is chosen.
    let saturation = if lightness > 0.5 {
        delta / (2.0 - max - min)
    } else {
        delta / (max + min)
    };
    let hue = if max == red {
        ((green - blue) / delta + if green < blue { 6.0 } else { 0.0 }) / 6.0
    } else if max == green {
        ((blue - red) / delta + 2.0) / 6.0
    } else {
        ((red - green) / delta + 4.0) / 6.0
    };
    (hue, saturation, lightness)
}

fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> [f64; 3] {
    if saturation == 0.0 {
        return [lightness; 3];
    }
    let q = if lightness < 0.5 {
        lightness * (1.0 + saturation)
    } else {
        lightness + saturation - lightness * saturation
    };
    let p = 2.0 * lightness - q;
    let channel = |t: f64| {
        let t = if t < 0.0 {
            t + 1.0
        } else if t > 1.0 {
            t - 1.0
        } else {
            t
        };
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 1.0 / 2.0 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [
        channel(hue + 1.0 / 3.0),
        channel(hue),
        channel(hue - 1.0 / 3.0),
    ]
}

/// WCAG relative luminance of an sRGB triple in 0–1.
fn relative_luminance(rgb: [f64; 3]) -> f64 {
    let linear = |channel: f64| {
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(rgb[0]) + 0.7152 * linear(rgb[1]) + 0.0722 * linear(rgb[2])
}

#[cfg(test)]
mod tests;
