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
mod tests {
    use super::*;
    use crate::theme::{Palette, TintParams};

    fn uniform(rgb: [u8; 3]) -> Vec<u8> {
        vec![rgb[0], rgb[1], rgb[2], 255]
    }

    fn mean_of(rgba: &[u8]) -> Mean {
        mean(rgba, TintParams::DARK.alpha_floor).expect("an opaque pixel")
    }

    fn tinted(rgb: [u8; 3]) -> [u8; 3] {
        normalise(mean_of(&uniform(rgb)), &TintParams::DARK)
    }

    fn channels(rgb: [u8; 3]) -> [f64; 3] {
        [
            f64::from(rgb[0]) / 255.0,
            f64::from(rgb[1]) / 255.0,
            f64::from(rgb[2]) / 255.0,
        ]
    }

    fn color_channels(color: egui::Color32) -> [f64; 3] {
        [
            f64::from(color.r()) / 255.0,
            f64::from(color.g()) / 255.0,
            f64::from(color.b()) / 255.0,
        ]
    }

    fn contrast(a: [f64; 3], b: [f64; 3]) -> f64 {
        let (a, b) = (relative_luminance(a), relative_luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// The reference's own table, re-derived by hand from `tintFromArtwork`.
    const TABLE: [([u8; 3], [u8; 3]); 7] = [
        ([51, 51, 51], [51, 51, 51]),
        ([128, 128, 128], [60, 60, 60]),
        ([255, 255, 255], [60, 60, 60]),
        ([255, 0, 0], [116, 27, 27]),
        ([0, 0, 0], [41, 41, 41]),
        ([255, 255, 0], [60, 60, 14]),
        ([0, 0, 255], [27, 27, 116]),
    ];

    #[test]
    fn the_reference_table() {
        for (mean, want) in TABLE {
            assert_eq!(tinted(mean), want, "mean {mean:?}");
        }
    }

    /// The light palette's constants only have to be expressible: light mode
    /// is not built, but its mirrored tone has to fall out of the same params.
    #[test]
    fn the_light_tone_is_expressible_and_lighter_than_the_dark_one() {
        let light = TintParams {
            tone_offset: 0.9,
            tone_slope: -0.18,
            saturation_min: 0.14,
            saturation_max: 0.34,
            luminance_ceiling: 0.62,
            ..TintParams::DARK
        };
        let mean = mean_of(&uniform([255, 0, 0]));
        let dark = normalise(mean, &TintParams::DARK);
        let got = normalise(mean, &light);
        assert!(
            relative_luminance(channels(got)) > relative_luminance(channels(dark)),
            "light {got:?} is not lighter than dark {dark:?}"
        );
    }

    /// A muted teal whose gain lands strictly inside the clamp, so the gain
    /// itself, not the clamp, decides the colour.
    #[test]
    fn the_saturation_gain_is_the_reference_constant() {
        let (_, raw, _) = to_hsl(channels([70, 95, 92]));
        assert!(
            raw * 2.2 > 0.32 && raw * 2.2 < 0.62,
            "the case exercises the gain, not the clamp"
        );
        assert_eq!(tinted([70, 95, 92]), [32, 65, 61]);

        let weaker = TintParams {
            saturation_gain: 2.0,
            ..TintParams::DARK
        };
        assert_ne!(
            normalise(mean_of(&uniform([70, 95, 92])), &weaker),
            tinted([70, 95, 92]),
            "a weaker gain must change the colour"
        );
    }

    /// Hue 1/3, the `max == g` branch: saturation 0.62, tone 0.28, stepped to
    /// 0.175, which rounds to 17, 72, 17.
    #[test]
    fn a_green_mean_takes_the_green_hue_branch() {
        let (hue, _, _) = to_hsl(channels([0, 255, 0]));
        assert!((hue - 1.0 / 3.0).abs() < 1e-12, "hue is {hue}");
        assert_eq!(tinted([0, 255, 0]), [17, 72, 17]);
    }

    /// Green under blue wraps the hue past 5/6: hue 0.9163, the same band as
    /// red would take, so the channels land at 116, 27, 72.
    #[test]
    fn a_magenta_mean_wraps_the_hue() {
        let mean = channels([255, 0, 128]);
        let (hue, _, _) = to_hsl(mean);
        assert!(mean[1] < mean[2], "green under blue takes the wrap");
        assert!(hue > 5.0 / 6.0, "hue is {hue}");
        assert_eq!(tinted([255, 0, 128]), [116, 27, 72]);
    }

    /// The band is what the ink sits on, so every pinned band must leave both
    /// text tiers above AA. A bright lime is the worst case the tone allows.
    #[test]
    fn every_pinned_band_leaves_its_text_aa() {
        let palette = &Palette::DARK;
        for (mean, _) in TABLE.into_iter().chain([([128, 255, 0], [41, 66, 16])]) {
            let band = channels(tinted(mean));
            for (name, ink) in [("text", palette.text), ("text_muted", palette.text_muted)] {
                let ratio = contrast(band, color_channels(ink));
                assert!(ratio >= 4.5, "{name} on {mean:?} is {ratio:.2}:1");
            }
        }
    }

    #[test]
    fn the_alpha_floor_is_inclusive() {
        assert_eq!(mean(&[255, 0, 0, 7], 8), None, "seven is too faint");
        assert_eq!(
            mean(&[255, 0, 0, 8], 8),
            Some([255.0, 0.0, 0.0]),
            "eight is opaque enough"
        );
    }

    #[test]
    fn nothing_opaque_has_no_mean() {
        assert_eq!(mean(&[10, 20, 30, 0, 40, 50, 60, 7], 8), None);
        assert_eq!(mean(&[], 8), None);
    }

    #[test]
    fn transparent_pixels_do_not_count_toward_the_mean() {
        assert_eq!(
            mean(&[255, 0, 0, 255, 0, 0, 255, 0], 8),
            Some([255.0, 0.0, 0.0]),
            "the blue pixel is transparent"
        );
        assert_eq!(
            mean(&[255, 0, 0, 255, 0, 0, 255, 255], 8),
            Some([127.5, 0.0, 127.5]),
            "two opaque pixels average"
        );
    }

    #[test]
    fn a_buffer_that_is_not_whole_pixels_ignores_its_tail() {
        assert_eq!(mean(&[255, 0, 0, 255, 9, 9, 9], 8), Some([255.0, 0.0, 0.0]));
    }

    #[test]
    fn a_mean_under_the_neutral_threshold_stays_exactly_grey() {
        // Saturation ~0.023, under the 0.03 threshold.
        let got = tinted([190, 187, 187]);
        let (_, saturation, _) = to_hsl(channels([190, 187, 187]));
        assert!(saturation < 0.03, "the case exercises the grey branch");
        assert_eq!(got[0], got[1], "a grey band has no hue");
        assert_eq!(got[1], got[2], "a grey band has no hue");
    }

    #[test]
    fn the_saturation_is_clamped_at_both_ends() {
        let p = TintParams::DARK;

        let mean = mean_of(&uniform([180, 170, 170]));
        let (_, raw, _) = to_hsl([mean[0] / 255.0, mean[1] / 255.0, mean[2] / 255.0]);
        assert!(raw >= p.neutral_below && raw * p.saturation_gain < p.saturation_min);
        let (_, floor, _) = to_hsl(channels(normalise(mean, &p)));
        assert!((floor - p.saturation_min).abs() < 0.01, "sat is {floor}");

        let mean = mean_of(&uniform([255, 0, 0]));
        let (_, raw, _) = to_hsl([1.0, 0.0, 0.0]);
        assert!(raw * p.saturation_gain > p.saturation_max);
        let (_, ceiling, _) = to_hsl(channels(normalise(mean, &p)));
        assert!(
            (ceiling - p.saturation_max).abs() < 0.01,
            "sat is {ceiling}"
        );
    }

    #[test]
    fn the_loop_steps_until_just_under_the_ceiling() {
        let p = TintParams::DARK;
        let mean = mean_of(&uniform([255, 255, 0]));
        let (hue, raw, lightness) = to_hsl([1.0, 1.0, 0.0]);
        let tone = lightness.clamp(p.tone_min, p.tone_max);
        let saturation = (raw * p.saturation_gain).clamp(p.saturation_min, p.saturation_max);

        let mut settled = tone;
        let mut steps = 0;
        while settled > p.lightness_floor
            && relative_luminance(hsl_to_rgb(hue, saturation, settled)) > p.luminance_ceiling
        {
            settled -= p.lightness_step;
            steps += 1;
        }
        assert!(steps > 0, "the case exercises the loop");
        assert_eq!(
            normalise(mean, &p),
            to_bytes(hsl_to_rgb(hue, saturation, settled))
        );
        assert!(
            relative_luminance(hsl_to_rgb(hue, saturation, settled)) <= p.luminance_ceiling,
            "the settled colour is over the ceiling"
        );
        assert!(
            relative_luminance(hsl_to_rgb(hue, saturation, settled + p.lightness_step))
                > p.luminance_ceiling,
            "one step lighter is still under the ceiling, so the loop over-stepped"
        );
    }

    #[test]
    fn another_parameter_set_changes_the_result() {
        let mean = mean_of(&uniform([255, 0, 0]));
        let bright = TintParams {
            tone_min: 0.60,
            tone_max: 0.70,
            luminance_ceiling: 0.99,
            ..TintParams::DARK
        };
        assert_eq!(normalise(mean, &TintParams::DARK), [116, 27, 27]);
        assert_ne!(
            normalise(mean, &bright),
            normalise(mean, &TintParams::DARK),
            "the params decide the colour, not the mean alone"
        );
    }

    #[test]
    fn the_mean_is_the_pixels_in_twenty_four_bit() {
        assert_eq!(mean_of(&[1, 2, 3, 255]), [1.0, 2.0, 3.0]);
        assert_eq!(mean_of(&[0, 0, 0, 255]), [0.0, 0.0, 0.0]);
    }
}
