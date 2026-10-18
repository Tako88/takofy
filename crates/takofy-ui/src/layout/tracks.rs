//! A one-dimensional track solver.
//!
//! A row of columns is described as a basis, a minimum, a maximum and a growth
//! factor. The basis is the size a track asks for before any growth; the
//! minimum and maximum bound the result. Fixed tracks are just tracks that do
//! not grow.
//!
//! When the space cannot hold every minimum, every track falls to its minimum
//! and the caller clips. Nothing here returns a negative or non-finite width.

/// One column in a solved row.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Track {
    /// The size the track asks for before the free space is shared.
    pub basis: f32,
    /// The smallest width the track accepts.
    pub min: f32,
    /// The largest width the track accepts; may be infinite.
    pub max: f32,
    /// How the free space is shared; zero makes the track fixed.
    pub grow: f32,
}

impl Track {
    /// A track that takes exactly `size`.
    pub const fn fixed(size: f32) -> Self {
        Self {
            basis: size,
            min: size,
            max: size,
            grow: 0.0,
        }
    }
}

/// The distance below which two computed widths are the same.
const EPSILON: f32 = 0.000_1;

/// Solves one row of tracks, gaps included, against `available`.
///
/// A negative or non-finite `available` counts as zero. A track whose growth
/// factor is zero takes its basis clamped to its own range; the rest share what
/// is left in proportion to their growth factors. A track that would cross its
/// minimum or maximum is held there and the remainder is shared again.
pub fn solve(available: f32, gap: f32, tracks: &[Track]) -> Vec<f32> {
    if tracks.is_empty() {
        return Vec::new();
    }

    let tracks: Vec<Track> = tracks.iter().map(sane).collect();
    let available = if available.is_finite() && available > 0.0 {
        available
    } else {
        0.0
    };
    let gap = if gap.is_finite() && gap > 0.0 {
        gap
    } else {
        0.0
    };
    let content = (available - gap * (tracks.len() - 1) as f32).max(0.0);

    let minimums: Vec<f32> = tracks.iter().map(|track| track.min).collect();
    if content + EPSILON < minimums.iter().sum() {
        return minimums;
    }

    let mut widths = vec![0.0; tracks.len()];
    let mut flexible = Vec::new();
    let mut free = content;

    for (index, track) in tracks.iter().enumerate() {
        if track.grow > 0.0 {
            flexible.push(index);
        } else {
            let width = track.basis.clamp(track.min, track.max);
            widths[index] = width;
            free -= width;
        }
    }

    if flexible.is_empty() {
        return widths;
    }

    let fixed_floor: f32 =
        widths.iter().sum::<f32>() + flexible.iter().map(|&index| tracks[index].min).sum::<f32>();
    let bases: f32 = flexible.iter().map(|&index| tracks[index].basis).sum();
    if content + EPSILON < fixed_floor || free + EPSILON < bases {
        return minimums;
    }

    free -= bases;
    for &index in &flexible {
        widths[index] = tracks[index].basis;
    }

    let mut active = flexible;
    while !active.is_empty() {
        let total_grow: f32 = active.iter().map(|&index| tracks[index].grow).sum();
        if total_grow <= 0.0 {
            break;
        }

        let mut clamped = Vec::new();
        for &index in &active {
            let track = tracks[index];
            let share = free * track.grow / total_grow;
            let candidate = widths[index] + share;
            if candidate + EPSILON < track.min {
                clamped.push((index, track.min));
            } else if candidate - EPSILON > track.max {
                clamped.push((index, track.max));
            }
        }

        if clamped.is_empty() {
            for &index in &active {
                widths[index] += free * tracks[index].grow / total_grow;
            }
            break;
        }

        for &(index, width) in &clamped {
            free -= width - widths[index];
            widths[index] = width;
        }
        active.retain(|index| !clamped.iter().any(|(done, _)| done == index));
    }

    widths
}

/// A track with its invalid values replaced by harmless ones.
fn sane(track: &Track) -> Track {
    let min = if track.min.is_finite() && track.min > 0.0 {
        track.min
    } else {
        0.0
    };
    let max = if track.max.is_nan() || track.max < min {
        min
    } else {
        track.max
    };
    let basis = if track.basis.is_finite() && track.basis > 0.0 {
        track.basis.min(max)
    } else {
        0.0
    };
    let grow = if track.grow.is_finite() && track.grow > 0.0 {
        track.grow
    } else {
        0.0
    };
    Track {
        basis,
        min,
        max,
        grow,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flexible(basis: f32, min: f32, max: f32, grow: f32) -> Track {
        Track {
            basis,
            min,
            max,
            grow,
        }
    }

    fn assert_close(left: f32, right: f32) {
        assert!((left - right).abs() < 0.001, "{left} is not {right}");
    }

    #[test]
    fn fixed_tracks_take_their_size() {
        let widths = solve(100.0, 0.0, &[Track::fixed(20.0), Track::fixed(30.0)]);
        assert_eq!(widths, vec![20.0, 30.0]);
    }

    #[test]
    fn flexible_tracks_share_the_rest_by_weight() {
        let widths = solve(
            100.0,
            0.0,
            &[
                Track::fixed(20.0),
                flexible(0.0, 0.0, f32::INFINITY, 1.0),
                flexible(0.0, 0.0, f32::INFINITY, 2.0),
            ],
        );
        assert_close(widths[0], 20.0);
        assert_close(widths[1], 80.0 / 3.0);
        assert_close(widths[2], 160.0 / 3.0);
    }

    #[test]
    fn a_track_below_its_minimum_is_held_and_the_others_share_the_remainder() {
        let widths = solve(
            120.0,
            0.0,
            &[
                flexible(0.0, 100.0, f32::INFINITY, 1.0),
                flexible(0.0, 0.0, f32::INFINITY, 1.0),
            ],
        );
        assert_eq!(widths, vec![100.0, 20.0]);
    }

    #[test]
    fn a_track_above_its_maximum_is_capped() {
        let widths = solve(
            160.0,
            0.0,
            &[
                flexible(0.0, 0.0, 60.0, 1.0),
                flexible(0.0, 0.0, f32::INFINITY, 1.0),
            ],
        );
        assert_eq!(widths, vec![60.0, 100.0]);
    }

    #[test]
    fn gaps_come_out_of_the_available_width() {
        let widths = solve(
            100.0,
            10.0,
            &[Track::fixed(30.0), flexible(0.0, 0.0, f32::INFINITY, 1.0)],
        );
        assert_eq!(widths, vec![30.0, 60.0]);
        assert_close(widths.iter().sum::<f32>() + 10.0, 100.0);
    }

    #[test]
    fn the_result_sums_to_the_available_width_when_nothing_is_capped() {
        let widths = solve(
            300.0,
            12.0,
            &[
                flexible(0.0, 20.0, f32::INFINITY, 1.0),
                flexible(0.0, 20.0, f32::INFINITY, 2.0),
                flexible(0.0, 20.0, f32::INFINITY, 1.0),
            ],
        );
        assert_close(widths.iter().sum::<f32>() + 24.0, 300.0);
    }

    #[test]
    fn when_the_minimums_do_not_fit_every_track_holds_its_minimum() {
        let widths = solve(
            100.0,
            0.0,
            &[
                flexible(0.0, 80.0, f32::INFINITY, 1.0),
                flexible(0.0, 80.0, f32::INFINITY, 1.0),
            ],
        );
        assert_eq!(widths, vec![80.0, 80.0]);
    }

    #[test]
    fn no_width_is_negative_or_nan_for_any_input() {
        let tracks = [
            Track::fixed(20.0),
            flexible(-10.0, -5.0, f32::NAN, -1.0),
            flexible(f32::NAN, 10.0, f32::INFINITY, 1.0),
            flexible(30.0, 10.0, 20.0, f32::INFINITY),
        ];

        for available in [-100.0, 0.0, f32::NAN, f32::INFINITY, 5.0, 500.0] {
            for gap in [-4.0, 0.0, f32::NAN, 8.0] {
                let widths = solve(available, gap, &tracks);
                assert_eq!(widths.len(), tracks.len());
                for width in widths {
                    assert!(width.is_finite(), "{width} is not finite");
                    assert!(width >= 0.0, "{width} is negative");
                }
            }
        }
    }
}
