//! Window geometry.
//!
//! Breakpoints and region visibility live here and nowhere else, so no view
//! measures the window and there is nothing to keep in sync.

use egui::{Rect, pos2};

use crate::theme::{extent, space};

/// Recommended starting size of the window, in points.
pub const WINDOW_DEFAULT: [f32; 2] = [1100.0, 720.0];
/// Smallest the window may be dragged, in points.
pub const WINDOW_MIN: [f32; 2] = [640.0, 480.0];

/// At and below this width the Now Playing column narrows.
pub const NOWPLAYING_NARROW_MAX: f32 = 1360.0;
/// At and below this width the Now Playing column is hidden.
pub const NOWPLAYING_HIDDEN_MAX: f32 = 1120.0;
/// At and below this width the playbar sheds its least-used controls.
pub const PLAYBAR_REDUCED_MAX: f32 = 1000.0;
/// At and below this width the sidebar becomes the rail.
pub const RAIL_MAX: f32 = 900.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sidebar {
    Full,
    Rail,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NowPlaying {
    Hidden,
    Narrow,
    Wide,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Playbar {
    Full,
    Reduced,
}

/// Which form each region takes at the current window width.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Regions {
    pub sidebar: Sidebar,
    pub now_playing: NowPlaying,
    pub playbar: Playbar,
}

/// The widths and heights the regions occupy, each one a token.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Shell {
    pub sidebar: f32,
    /// `None` when the Now Playing column is hidden.
    pub now_playing: Option<f32>,
    pub playbar_h: f32,
    pub topbar_h: f32,
    pub gap: f32,
    pub padding: f32,
}

/// Classifies the window.
///
/// Every threshold is evaluated on its own, because the reference's breakpoints
/// add up rather than replace one another: below 900 the sidebar is a rail and
/// the Now Playing column is hidden and the playbar is reduced, all at once.
pub fn regions_for(width: f32) -> Regions {
    Regions {
        sidebar: if width <= RAIL_MAX {
            Sidebar::Rail
        } else {
            Sidebar::Full
        },
        now_playing: if width <= NOWPLAYING_HIDDEN_MAX {
            NowPlaying::Hidden
        } else if width <= NOWPLAYING_NARROW_MAX {
            NowPlaying::Narrow
        } else {
            NowPlaying::Wide
        },
        playbar: if width <= PLAYBAR_REDUCED_MAX {
            Playbar::Reduced
        } else {
            Playbar::Full
        },
    }
}

pub fn shell_for(regions: &Regions) -> Shell {
    Shell {
        sidebar: match regions.sidebar {
            Sidebar::Full => extent::SIDEBAR_W,
            Sidebar::Rail => extent::SIDEBAR_RAIL_W,
        },
        now_playing: match regions.now_playing {
            NowPlaying::Hidden => None,
            NowPlaying::Narrow => Some(extent::NOWPLAYING_NARROW_W),
            NowPlaying::Wide => Some(extent::NOWPLAYING_W),
        },
        playbar_h: extent::PLAYBAR_H,
        topbar_h: extent::TOPBAR_H,
        gap: space::S2,
        padding: space::S2,
    }
}

/// One rect per region, solved from the window's content rect.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Grid {
    pub sidebar: Rect,
    pub topbar: Rect,
    pub view: Rect,
    pub now_playing: Option<Rect>,
    pub playbar: Rect,
    pub drawer: Rect,
}

/// The regions tile the inner area with `gap`-wide channels between them.
///
/// The shell is the whole input. It already carries the widths the
/// classification produced, so reading the classification again here could only
/// disagree with it.
pub fn grid_for(content: Rect, shell: &Shell) -> Grid {
    let inner = content.shrink(shell.padding);

    let playbar = Rect::from_min_max(
        pos2(inner.left(), inner.bottom() - shell.playbar_h),
        inner.max,
    );
    let panel_bottom = playbar.top() - shell.gap;

    let sidebar = Rect::from_min_max(inner.min, pos2(inner.left() + shell.sidebar, panel_bottom));
    let main_left = sidebar.right() + shell.gap;

    let (main_right, now_playing) = match shell.now_playing {
        Some(width) => {
            let left = inner.right() - width;
            (
                left - shell.gap,
                Some(Rect::from_min_max(
                    pos2(left, inner.top()),
                    pos2(inner.right(), panel_bottom),
                )),
            )
        }
        None => (inner.right(), None),
    };

    let topbar = Rect::from_min_max(
        pos2(main_left, inner.top()),
        pos2(main_right, inner.top() + shell.topbar_h),
    );
    let view = Rect::from_min_max(
        pos2(main_left, topbar.bottom()),
        pos2(main_right, panel_bottom),
    );

    // The drawer is the rail's overflow panel, anchored to the padding corner
    // and clearing the playbar so the transport stays reachable.
    let drawer = Rect::from_min_max(
        inner.min,
        pos2(inner.left() + extent::SIDEBAR_W, playbar.top() - space::S3),
    );

    Grid {
        sidebar,
        topbar,
        view,
        now_playing,
        playbar,
        drawer,
    }
}

#[cfg(test)]
mod tests {
    use egui::{Pos2, vec2};

    use super::*;

    fn content(width: f32, height: f32) -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(width, height))
    }

    fn solve(width: f32, height: f32) -> (Grid, Regions, Shell) {
        let content = content(width, height);
        let regions = regions_for(content.width());
        let shell = shell_for(&regions);
        (grid_for(content, &shell), regions, shell)
    }

    #[test]
    fn each_threshold_is_inclusive_at_its_own_value_and_not_one_point_above() {
        assert_eq!(
            regions_for(NOWPLAYING_NARROW_MAX).now_playing,
            NowPlaying::Narrow
        );
        assert_eq!(
            regions_for(NOWPLAYING_NARROW_MAX + 0.1).now_playing,
            NowPlaying::Wide
        );

        assert_eq!(
            regions_for(NOWPLAYING_HIDDEN_MAX).now_playing,
            NowPlaying::Hidden
        );
        assert_eq!(
            regions_for(NOWPLAYING_HIDDEN_MAX + 0.1).now_playing,
            NowPlaying::Narrow
        );

        assert_eq!(regions_for(PLAYBAR_REDUCED_MAX).playbar, Playbar::Reduced);
        assert_eq!(
            regions_for(PLAYBAR_REDUCED_MAX + 0.1).playbar,
            Playbar::Full
        );

        assert_eq!(regions_for(RAIL_MAX).sidebar, Sidebar::Rail);
        assert_eq!(regions_for(RAIL_MAX + 0.1).sidebar, Sidebar::Full);
    }

    #[test]
    fn the_thresholds_are_evaluated_independently() {
        let regions = regions_for(800.0);
        assert_eq!(regions.sidebar, Sidebar::Rail);
        assert_eq!(regions.now_playing, NowPlaying::Hidden);
        assert_eq!(regions.playbar, Playbar::Reduced);
    }

    #[test]
    fn now_playing_is_wide_above_1360_and_narrow_between_1121_and_1360() {
        assert_eq!(regions_for(1360.1).now_playing, NowPlaying::Wide);
        for width in [1120.1, 1300.0, 1360.0] {
            assert_eq!(
                regions_for(width).now_playing,
                NowPlaying::Narrow,
                "{width}"
            );
        }
    }

    #[test]
    fn now_playing_is_hidden_at_and_below_1120() {
        for width in [WINDOW_MIN[0], 900.0, 1120.0] {
            assert_eq!(
                regions_for(width).now_playing,
                NowPlaying::Hidden,
                "{width}"
            );
        }
    }

    #[test]
    fn the_sidebar_is_a_rail_at_and_below_900() {
        for width in [WINDOW_MIN[0], 800.0, 900.0] {
            assert_eq!(regions_for(width).sidebar, Sidebar::Rail, "{width}");
        }
        assert_eq!(regions_for(900.1).sidebar, Sidebar::Full);
    }

    #[test]
    fn a_hidden_now_playing_has_no_width() {
        assert_eq!(shell_for(&regions_for(1120.0)).now_playing, None);
    }

    #[test]
    fn the_visible_widths_are_the_extent_tokens() {
        let wide = shell_for(&regions_for(1440.0));
        assert_eq!(wide.sidebar, extent::SIDEBAR_W);
        assert_eq!(wide.now_playing, Some(extent::NOWPLAYING_W));
        assert_eq!(wide.playbar_h, extent::PLAYBAR_H);
        assert_eq!(wide.topbar_h, extent::TOPBAR_H);

        let narrow = shell_for(&regions_for(1200.0));
        assert_eq!(narrow.sidebar, extent::SIDEBAR_W);
        assert_eq!(narrow.now_playing, Some(extent::NOWPLAYING_NARROW_W));

        let rail = shell_for(&regions_for(800.0));
        assert_eq!(rail.sidebar, extent::SIDEBAR_RAIL_W);
    }

    #[test]
    fn the_minimum_window_is_the_narrowest_layout() {
        let regions = regions_for(WINDOW_MIN[0]);
        assert_eq!(regions.sidebar, Sidebar::Rail);
        assert_eq!(regions.now_playing, NowPlaying::Hidden);
        assert_eq!(regions.playbar, Playbar::Reduced);
    }

    #[test]
    fn the_default_window_hides_the_now_playing_column() {
        // The same thing the reference does at 1100; whether the default should
        // change is a question for when the column has contents.
        assert_eq!(
            regions_for(WINDOW_DEFAULT[0]).now_playing,
            NowPlaying::Hidden
        );
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
    fn the_outer_edge_is_the_padding_on_every_side() {
        let (grid, _, shell) = solve(1440.0, 900.0);
        assert_eq!(grid.sidebar.left(), shell.padding);
        assert_eq!(grid.sidebar.top(), shell.padding);
        assert_eq!(grid.playbar.right(), 1440.0 - shell.padding);
        assert_eq!(grid.playbar.bottom(), 900.0 - shell.padding);
        assert_eq!(grid.now_playing.unwrap().right(), 1440.0 - shell.padding);
    }

    #[test]
    fn adjacent_columns_are_exactly_one_gap_apart() {
        let (grid, _, shell) = solve(1440.0, 900.0);
        assert_eq!(grid.sidebar.right() + shell.gap, grid.topbar.left());
        assert_eq!(
            grid.topbar.right() + shell.gap,
            grid.now_playing.unwrap().left()
        );

        let (rail, _, shell) = solve(800.0, 720.0);
        assert_eq!(rail.sidebar.right() + shell.gap, rail.topbar.left());
        assert!(rail.now_playing.is_none());
    }

    #[test]
    fn the_playbar_spans_the_full_inner_width() {
        let (grid, _, shell) = solve(1440.0, 900.0);
        assert_eq!(grid.playbar.left(), shell.padding);
        assert_eq!(grid.playbar.right(), 1440.0 - shell.padding);
    }

    #[test]
    fn the_panel_row_clears_the_playbar_by_one_gap() {
        let (grid, _, shell) = solve(1440.0, 900.0);
        assert_eq!(grid.sidebar.bottom() + shell.gap, grid.playbar.top());
        assert_eq!(
            grid.now_playing.unwrap().bottom() + shell.gap,
            grid.playbar.top()
        );
        assert_eq!(grid.view.bottom() + shell.gap, grid.playbar.top());
    }

    #[test]
    fn the_view_is_what_the_other_columns_leave() {
        let (grid, _, shell) = solve(1440.0, 900.0);
        assert_eq!(grid.view.left(), grid.topbar.left());
        assert_eq!(
            grid.view.right(),
            grid.now_playing.unwrap().left() - shell.gap
        );
        assert_eq!(grid.view.top(), grid.topbar.bottom());
    }

    #[test]
    fn a_hidden_now_playing_hands_its_width_to_the_view() {
        let (hidden, _, shell) = solve(1120.0, 900.0);
        assert!(hidden.now_playing.is_none());
        assert_eq!(hidden.view.right(), 1120.0 - shell.padding);

        let (narrow, _, _) = solve(1121.0, 900.0);
        assert!(narrow.now_playing.is_some());
        assert!(narrow.view.width() < hidden.view.width());
    }

    #[test]
    fn the_rail_is_narrower_than_the_full_sidebar() {
        let (full, _, _) = solve(1440.0, 900.0);
        let (rail, regions, shell) = solve(800.0, 900.0);
        assert_eq!(rail.sidebar.width(), extent::SIDEBAR_RAIL_W);
        assert!(rail.sidebar.width() < full.sidebar.width());
        assert_eq!(regions.sidebar, Sidebar::Rail);
        assert_eq!(rail.sidebar.width(), shell.sidebar);
    }

    #[test]
    fn the_topbar_sits_above_the_view_with_no_overlap() {
        let (grid, _, shell) = solve(1440.0, 900.0);
        assert_eq!(grid.topbar.bottom(), grid.view.top());
        assert_eq!(grid.topbar.top(), grid.view.top() - shell.topbar_h);
        assert_eq!(grid.topbar.height(), shell.topbar_h);
    }

    #[test]
    fn no_rect_is_negative_or_degenerate_at_the_minimum_window() {
        let (grid, _, _) = solve(WINDOW_MIN[0], WINDOW_MIN[1]);
        let mut rects = vec![
            grid.sidebar,
            grid.topbar,
            grid.view,
            grid.playbar,
            grid.drawer,
        ];
        if let Some(now_playing) = grid.now_playing {
            rects.push(now_playing);
        }
        for rect in rects {
            assert!(rect.width() > 0.0, "{rect:?} has no width");
            assert!(rect.height() > 0.0, "{rect:?} has no height");
        }
    }

    #[test]
    fn the_drawer_starts_on_the_padding_corner_and_clears_the_playbar() {
        let (grid, _, shell) = solve(1440.0, 900.0);
        assert_eq!(grid.drawer.left(), shell.padding);
        assert_eq!(grid.drawer.top(), shell.padding);
        assert_eq!(grid.drawer.width(), extent::SIDEBAR_W);
        assert_eq!(grid.drawer.bottom(), grid.playbar.top() - space::S3);
    }

    #[test]
    fn the_regions_tile_the_inner_area_without_gaps_or_overlaps() {
        let (grid, _, shell) = solve(1440.0, 900.0);
        let inner = content(1440.0, 900.0).shrink(shell.padding);

        let mut area = grid.sidebar.area() + grid.topbar.area() + grid.view.area();
        area += grid.playbar.area();
        if let Some(now_playing) = grid.now_playing {
            area += now_playing.area();
        }

        let columns = if grid.now_playing.is_some() { 3 } else { 2 };
        let column_gaps = shell.gap * grid.sidebar.height() * (columns - 1) as f32;
        let row_gap = shell.gap * inner.width();
        let tiled = area + column_gaps + row_gap;

        assert!(
            (tiled - inner.area()).abs() < 0.01,
            "regions cover {tiled}, inner area is {}",
            inner.area()
        );
    }
}
