//! Window geometry.
//!
//! Breakpoints and region visibility live here and nowhere else, so no view
//! measures the window and there is nothing to keep in sync.

use crate::theme::extent;

/// Recommended starting size of the window, in points.
pub const WINDOW_DEFAULT: [f32; 2] = [1100.0, 720.0];
/// Smallest the window may be dragged, in points.
pub const WINDOW_MIN: [f32; 2] = [640.0, 480.0];

/// Below this width the sidebar is not worth its space.
pub const COMPACT_MAX: f32 = 760.0;
/// At and above this width the shell has room for a wider sidebar.
pub const WIDE_MIN: f32 = 1280.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Width {
    /// Sidebar hidden; list and now-playing only.
    Compact,
    /// Sidebar, list, now-playing.
    Regular,
    /// Room to spare. Resolves to the same shell as `Regular` today.
    Wide,
}

impl Width {
    pub fn of(points: f32) -> Self {
        if points < COMPACT_MAX {
            Self::Compact
        } else if points < WIDE_MIN {
            Self::Regular
        } else {
            Self::Wide
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Regions {
    /// Playlists and library navigation.
    pub sidebar: bool,
    /// The list or detail pane.
    pub main: bool,
    /// The persistent playback bar.
    pub now_playing: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Shell {
    /// `None` means the region is hidden.
    pub sidebar: Option<f32>,
    pub now_playing: f32,
}

pub fn regions_for(width: Width) -> Regions {
    Regions {
        sidebar: width != Width::Compact,
        main: true,
        now_playing: true,
    }
}

/// The breakpoints decide which regions show; every width here is an extent
/// token, so no view measures the window for itself.
pub fn shell_for(width: Width) -> Shell {
    let sidebar = match width {
        Width::Compact => None,
        // The reference has one sidebar width. A wider window spends the extra
        // room on the regions beside the sidebar, not on a wider sidebar.
        Width::Regular | Width::Wide => Some(extent::SIDEBAR_W),
    };

    Shell {
        sidebar,
        now_playing: extent::PLAYBAR_H,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_windows_hide_the_sidebar_but_never_the_controls() {
        for points in [200.0, 759.9] {
            let regions = regions_for(Width::of(points));
            assert!(!regions.sidebar, "{points} should drop the sidebar");
            assert!(regions.main, "{points} must still show the list");
            assert!(regions.now_playing, "{points} must still show playback");
        }
    }

    #[test]
    fn breakpoints_are_inclusive_at_their_lower_bound() {
        assert_eq!(Width::of(COMPACT_MAX), Width::Regular);
        assert_eq!(Width::of(WIDE_MIN), Width::Wide);
    }

    #[test]
    fn the_default_window_starts_in_the_regular_layout() {
        assert_eq!(Width::of(WINDOW_DEFAULT[0]), Width::Regular);
    }

    #[test]
    fn the_minimum_window_is_still_compact() {
        assert_eq!(Width::of(WINDOW_MIN[0]), Width::Compact);
    }

    #[test]
    fn the_minimum_window_fits_inside_the_default() {
        for axis in 0..2 {
            assert!(
                WINDOW_MIN[axis] <= WINDOW_DEFAULT[axis],
                "minimum axis {axis} exceeds the default"
            );
        }
    }

    #[test]
    fn both_wide_layouts_use_the_single_sidebar_width() {
        for width in [Width::Regular, Width::Wide] {
            assert_eq!(
                shell_for(width).sidebar,
                Some(extent::SIDEBAR_W),
                "{width:?}"
            );
        }
    }

    #[test]
    fn no_region_is_hidden_without_a_width_that_hides_it() {
        for width in [Width::Compact, Width::Regular, Width::Wide] {
            let regions = regions_for(width);
            let shell = shell_for(width);
            assert_eq!(regions.sidebar, shell.sidebar.is_some(), "{width:?}");
        }
    }
}
