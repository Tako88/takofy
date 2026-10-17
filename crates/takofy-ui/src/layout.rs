//! Window geometry.
//!
//! Breakpoints and region visibility live here and nowhere else, so no view
//! contains a width check of its own and there is nothing to keep in sync. A
//! view asks this module what is on screen; it does not measure the window.
//!
//! The target is a resizable desktop window on Windows and Linux. There are no
//! touch affordances and no phone layout: "compact" means a small window, not a
//! small device.

use crate::theme::{Tokens, space};

/// Below this width the sidebar is not worth its space.
pub const COMPACT_MAX: f32 = 760.0;
/// At and above this width the shell has room for a wider sidebar.
pub const WIDE_MIN: f32 = 1280.0;

/// How much room the window has.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Width {
    /// Sidebar hidden; list and now-playing only.
    Compact,
    /// Sidebar, list, now-playing.
    Regular,
    /// Sidebar gets a second column of its own.
    Wide,
}

impl Width {
    /// Classifies an available width in points.
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

/// Which regions the shell draws at a given width.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Regions {
    /// Playlists and library navigation.
    pub sidebar: bool,
    /// The list or detail pane.
    pub main: bool,
    /// The persistent playback bar.
    pub now_playing: bool,
}

/// The shell's column widths, in points. `None` means the region is hidden.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Shell {
    /// Sidebar width.
    pub sidebar: Option<f32>,
    /// Now-playing bar height.
    pub now_playing: f32,
}

/// Which regions are visible at `width`.
pub fn regions_for(width: Width) -> Regions {
    Regions {
        sidebar: width != Width::Compact,
        main: true,
        now_playing: true,
    }
}

/// The shell geometry at `width`, expressed in tokens.
pub fn shell_for(width: Width, tokens: &Tokens) -> Shell {
    let sidebar = match width {
        Width::Compact => None,
        Width::Regular => Some(tokens.art_large + 3.0 * space::XL),
        Width::Wide => Some(2.0 * tokens.art_large + 3.0 * space::XL),
    };

    Shell {
        sidebar,
        now_playing: tokens.art_small + 2.0 * space::MD,
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
    fn the_wide_sidebar_is_never_narrower_than_the_regular_one() {
        let tokens = Tokens::DARK;
        let regular = shell_for(Width::Regular, &tokens).sidebar.unwrap();
        let wide = shell_for(Width::Wide, &tokens).sidebar.unwrap();
        assert!(wide >= regular, "{wide} < {regular}");
    }

    #[test]
    fn no_region_is_hidden_without_a_width_that_hides_it() {
        let tokens = Tokens::DARK;
        for width in [Width::Compact, Width::Regular, Width::Wide] {
            let regions = regions_for(width);
            let shell = shell_for(width, &tokens);
            assert_eq!(regions.sidebar, shell.sidebar.is_some(), "{width:?}");
        }
    }
}
