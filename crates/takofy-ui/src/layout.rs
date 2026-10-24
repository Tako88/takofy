//! Window geometry.
//!
//! Breakpoints and region visibility live here and nowhere else, so no view
//! measures the window and there is nothing to keep in sync.

use egui::{Rect, pos2, vec2};

use crate::theme::{extent, space};

pub mod actionbar;
pub mod cards;
pub mod chips;
pub mod episode;
pub mod hero;
pub mod menu;
pub mod panel;
pub mod queue;
pub mod search_menu;
pub mod sticky;
pub mod table;
pub mod tracks;

use tracks::{Track, solve};

/// Recommended starting size of the window, in points.
/// Wide enough that the Now Playing column is on screen in a window nobody has
/// touched: a default that hides a region is a region a user never finds.
pub const WINDOW_DEFAULT: [f32; 2] = [1240.0, 720.0];
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
/// At and below this width a playlist drops the album column and keeps the
/// artist. The reference does it in the same media query that reduces the
/// playbar, and the artist is what tells a listener which record is playing.
pub const TABLE_REDUCED_MAX: f32 = 1000.0;
/// At and below this width a track list drops the album, the date added and the
/// like, keeping the title, the artist and the duration. The reference does it
/// in a media query on the window, which is why it is a threshold here and not a
/// measure of the table.
pub const TABLE_NARROW_MAX: f32 = 760.0;

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

/// The three columns of the playbar, in points.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PlaybarCols {
    pub now: f32,
    pub centre: f32,
    pub extras: f32,
}

impl PlaybarCols {
    /// The gap between two playbar columns, `space-4`.
    pub const GAP: f32 = space::S4;
    /// The playbar's inline padding, `space-2`.
    pub const PADDING: f32 = space::S2;

    /// Places the solved widths left to right inside `rect`.
    pub fn columns(self, rect: Rect) -> [Rect; 3] {
        let left = rect.left() + Self::PADDING;
        let height = rect.height();
        let now = Rect::from_min_size(pos2(left, rect.top()), vec2(self.now, height));
        let centre = Rect::from_min_size(
            pos2(now.right() + Self::GAP, rect.top()),
            vec2(self.centre, height),
        );
        let extras = Rect::from_min_size(
            pos2(centre.right() + Self::GAP, rect.top()),
            vec2(self.extras, height),
        );
        [now, centre, extras]
    }
}

/// The topbar's items, in points. The toggle exists only in the rail form.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TopbarCols {
    pub toggle: Option<f32>,
    pub history: f32,
    pub search: f32,
    pub trail: f32,
}

impl TopbarCols {
    /// The gap between two topbar items, `space-4`.
    pub const GAP: f32 = space::S4;
    /// The topbar's inline padding, `space-4`.
    pub const PADDING: f32 = space::S4;

    /// Places the solved widths in their flex order, with the trail held to the
    /// right by the free space before it.
    pub fn columns(self, rect: Rect) -> TopbarRects {
        let left = rect.left() + Self::PADDING;
        let right = rect.right() - Self::PADDING;
        let height = rect.height();

        let trail = Rect::from_min_size(
            pos2(right - self.trail, rect.top()),
            vec2(self.trail, height),
        );

        let mut cursor = left;
        let toggle = self.toggle.map(|width| {
            let toggle = Rect::from_min_size(pos2(cursor, rect.top()), vec2(width, height));
            cursor = toggle.right() + Self::GAP;
            toggle
        });
        let history = Rect::from_min_size(pos2(cursor, rect.top()), vec2(self.history, height));
        cursor = history.right() + Self::GAP;
        let search = Rect::from_min_size(pos2(cursor, rect.top()), vec2(self.search, height));

        TopbarRects {
            toggle,
            history,
            search,
            trail,
        }
    }
}

/// The placed topbar items.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TopbarRects {
    pub toggle: Option<Rect>,
    pub history: Rect,
    pub search: Rect,
    pub trail: Rect,
}

/// How much room a track list has.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Table {
    /// Every column the reference draws.
    Full,
    /// The playlist has dropped the album so the artist can stay.
    Reduced,
    /// The title and the artist take what the album, the date and the like were
    /// using.
    Narrow,
}

impl Table {
    /// The columns this width leaves room for.
    pub fn columns(self) -> table::Present {
        match self {
            Self::Full => table::Present {
                artist: true,
                album: true,
                added: true,
                like: true,
                duration: true,
                actions: true,
            },
            Self::Reduced => table::Present {
                artist: true,
                album: false,
                added: true,
                like: true,
                duration: true,
                actions: true,
            },
            Self::Narrow => table::Present {
                artist: true,
                album: false,
                added: false,
                like: false,
                duration: true,
                actions: true,
            },
        }
    }
}

/// What the user asked of the Now Playing column, resolved against the width.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PanePref {
    /// The width rule alone.
    #[default]
    Auto,
    /// Hidden wherever the width would show it.
    Hidden,
}

/// What the queue button can do about the pane at the current width.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaneButton {
    /// The width has no pane here, so there is nothing to toggle.
    Unavailable,
    Shown,
    Hidden,
}

impl PanePref {
    /// The preference a press on the queue button leaves.
    ///
    /// A press is inert where the width has no pane, so a click can never
    /// corrupt a preference that applies at a wider window.
    pub fn press(self, width: f32) -> PanePref {
        match pane_button(width, self) {
            PaneButton::Unavailable => self,
            PaneButton::Shown => PanePref::Hidden,
            PaneButton::Hidden => PanePref::Auto,
        }
    }
}

/// Which form each region takes at the current window width.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Regions {
    pub sidebar: Sidebar,
    pub now_playing: NowPlaying,
    pub playbar: Playbar,
    pub table: Table,
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
        table: if width <= TABLE_NARROW_MAX {
            Table::Narrow
        } else if width <= TABLE_REDUCED_MAX {
            Table::Reduced
        } else {
            Table::Full
        },
    }
}

/// The window's regions with the user's choice about the pane applied.
///
/// The width still decides every other region, and still decides that the pane
/// is absent: a choice only ever takes the pane away.
pub fn regions_with(width: f32, pane: PanePref) -> Regions {
    let mut regions = regions_for(width);
    if pane == PanePref::Hidden {
        regions.now_playing = NowPlaying::Hidden;
    }
    regions
}

/// What the queue button can do at this width, so the view can draw it honestly.
pub fn pane_button(width: f32, pane: PanePref) -> PaneButton {
    if width <= NOWPLAYING_HIDDEN_MAX {
        return PaneButton::Unavailable;
    }
    match pane {
        PanePref::Auto => PaneButton::Shown,
        PanePref::Hidden => PaneButton::Hidden,
    }
}

pub fn shell_for(regions: &Regions) -> Shell {
    Shell {
        sidebar: match regions.sidebar {
            Sidebar::Full => extent::SIDEBAR_W,
            // The sidebar is the drawer below 900, so the column takes nothing.
            Sidebar::Rail => 0.0,
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
    let main_left = if shell.sidebar > 0.0 {
        sidebar.right() + shell.gap
    } else {
        inner.left()
    };

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

/// The width of the playbar's extras, derived from the controls it holds.
///
/// The full bar has four icon buttons and the volume, with a `space-1` gap
/// between each; the reduced bar hides the first two buttons and keeps the
/// volume's `space-2` margin on each side.
pub fn playbar_extras(form: Playbar) -> f32 {
    match form {
        Playbar::Full => {
            4.0 * extent::BUTTON + extent::VOLUME_W + 2.0 * space::S2 + 4.0 * space::S1
        }
        Playbar::Reduced => {
            2.0 * extent::BUTTON + extent::VOLUME_W + 2.0 * space::S2 + 2.0 * space::S1
        }
    }
}

/// Solves the playbar's three columns inside `rect`.
pub fn playbar_for(rect: Rect, form: Playbar) -> PlaybarCols {
    let available = rect.width() - 2.0 * PlaybarCols::PADDING;
    let tracks = match form {
        Playbar::Full => [
            Track {
                basis: 0.0,
                min: extent::PLAYBAR_NOW_MIN,
                max: f32::INFINITY,
                grow: 1.0,
            },
            Track {
                basis: 0.0,
                min: extent::PLAYBAR_CENTRE_MIN,
                max: f32::INFINITY,
                grow: 2.0,
            },
            Track {
                basis: 0.0,
                min: extent::PLAYBAR_EXTRAS_MIN,
                max: f32::INFINITY,
                grow: 1.0,
            },
        ],
        Playbar::Reduced => [
            Track {
                basis: 0.0,
                min: extent::PLAYBAR_NOW_MIN_REDUCED,
                max: f32::INFINITY,
                grow: 1.0,
            },
            Track {
                basis: 0.0,
                min: extent::PLAYBAR_CENTRE_MIN_REDUCED,
                max: f32::INFINITY,
                grow: 1.5,
            },
            Track::fixed(playbar_extras(Playbar::Reduced)),
        ],
    };
    let widths = solve(available, PlaybarCols::GAP, &tracks);
    PlaybarCols {
        now: widths[0],
        centre: widths[1],
        extras: widths[2],
    }
}

/// The space the topbar's non-trail items leave free, in points.
fn topbar_free(rect: Rect, sidebar: Sidebar) -> f32 {
    let toggle = match sidebar {
        Sidebar::Full => 0.0,
        Sidebar::Rail => extent::BUTTON,
    };
    let items = match sidebar {
        Sidebar::Full => 3.0,
        Sidebar::Rail => 4.0,
    };
    rect.width()
        - 2.0 * TopbarCols::PADDING
        - toggle
        - (3.0 * extent::BUTTON + 2.0 * space::S1)
        - TopbarCols::GAP * (items - 1.0)
}

/// Solves the topbar's items inside `rect`.
///
/// The trail gets its natural width first, so the search field yields down to
/// its floor before the trail loses a single point; only once the field is at
/// its floor does the trail take what is left and truncate.
pub fn topbar_for(rect: Rect, sidebar: Sidebar, trail_natural: f32) -> TopbarCols {
    let toggle = match sidebar {
        Sidebar::Full => None,
        Sidebar::Rail => Some(extent::BUTTON),
    };
    let history = 3.0 * extent::BUTTON + 2.0 * space::S1;
    let free = topbar_free(rect, sidebar).max(0.0);
    let natural = if trail_natural.is_finite() && trail_natural > 0.0 {
        trail_natural
    } else {
        0.0
    };
    let search = (free - natural).clamp(extent::SEARCH_MIN_W, extent::SEARCH_MAX_W);
    let trail = (free - search).clamp(0.0, natural);

    TopbarCols {
        toggle,
        history,
        search,
        trail,
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
        solve_with(width, height, PanePref::Auto)
    }

    fn solve_with(width: f32, height: f32, pane: PanePref) -> (Grid, Regions, Shell) {
        let content = content(width, height);
        let regions = regions_with(content.width(), pane);
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

        assert_eq!(regions_for(TABLE_NARROW_MAX).table, Table::Narrow);
        assert_eq!(regions_for(TABLE_NARROW_MAX + 0.1).table, Table::Reduced);
        assert_eq!(regions_for(TABLE_REDUCED_MAX).table, Table::Reduced);
        assert_eq!(regions_for(TABLE_REDUCED_MAX + 0.1).table, Table::Full);
    }

    #[test]
    fn a_narrow_table_keeps_the_title_the_artist_its_duration_and_its_controls() {
        let narrow = Table::Narrow.columns();
        assert!(!narrow.album && !narrow.added && !narrow.like);
        // A playlist is the one list whose artist cannot be inferred from the
        // list itself, so it is never dropped. The duration is what a listener
        // scrubs by, and the more button is the only way to reach a track's
        // actions, so neither is ever dropped either.
        assert!(narrow.artist);
        assert!(narrow.duration && narrow.actions);

        let full = Table::Full.columns();
        assert!(full.artist && full.album && full.added && full.like);
        assert!(full.duration && full.actions);
    }

    #[test]
    fn the_album_goes_before_the_artist_below_1000() {
        let reduced = Table::Reduced.columns();
        assert!(!reduced.album);
        assert!(reduced.artist);
        // The date added and the like stay until the window is much narrower.
        assert!(reduced.added && reduced.like);
    }

    #[test]
    fn regions_with_auto_equals_the_width_rule_at_every_width() {
        let edges = [
            NOWPLAYING_NARROW_MAX,
            NOWPLAYING_HIDDEN_MAX,
            PLAYBAR_REDUCED_MAX,
            RAIL_MAX,
            TABLE_REDUCED_MAX,
            TABLE_NARROW_MAX,
        ];
        let sweep = (WINDOW_MIN[0] as u32..=2000).map(|width| width as f32);
        let thresholds = edges
            .iter()
            .flat_map(|edge| [edge - 0.1, *edge, edge + 0.1]);
        for width in sweep.chain(thresholds) {
            assert_eq!(
                regions_with(width, PanePref::Auto),
                regions_for(width),
                "{width}"
            );
        }
    }

    #[test]
    fn a_hidden_choice_hides_only_the_pane() {
        for width in [1120.1, 1200.0, 1240.0, 1360.0, 1360.1, 1440.0, 2000.0] {
            let shown = regions_for(width);
            let hidden = regions_with(width, PanePref::Hidden);
            assert_eq!(hidden.now_playing, NowPlaying::Hidden, "{width}");
            assert_eq!(hidden.sidebar, shown.sidebar, "{width}");
            assert_eq!(hidden.playbar, shown.playbar, "{width}");
            assert_eq!(hidden.table, shown.table, "{width}");
        }
    }

    #[test]
    fn at_and_below_1120_the_choice_is_moot() {
        for width in [WINDOW_MIN[0], 900.0, 1000.0, 1120.0] {
            assert_eq!(
                regions_with(width, PanePref::Hidden),
                regions_with(width, PanePref::Auto),
                "{width}"
            );
        }
    }

    #[test]
    fn the_choice_never_opens_a_pane_the_width_hides() {
        // The only choice a preference can express is "hidden"; there is no
        // variant that would force a pane onto a window with no room for it.
        for width in [WINDOW_MIN[0], 900.0, 1120.0] {
            assert_eq!(
                regions_with(width, PanePref::Hidden).now_playing,
                NowPlaying::Hidden,
                "{width}"
            );
        }
    }

    #[test]
    fn pane_button_is_unavailable_at_and_below_1120_and_follows_the_choice_above() {
        for width in [WINDOW_MIN[0], 900.0, 1000.0, 1120.0] {
            for pane in [PanePref::Auto, PanePref::Hidden] {
                assert_eq!(
                    pane_button(width, pane),
                    PaneButton::Unavailable,
                    "{width} {pane:?}"
                );
            }
        }
        assert_eq!(pane_button(1120.1, PanePref::Auto), PaneButton::Shown);
        assert_eq!(pane_button(1120.1, PanePref::Hidden), PaneButton::Hidden);
        assert_eq!(pane_button(1440.0, PanePref::Auto), PaneButton::Shown);
    }

    #[test]
    fn press_flips_where_the_pane_exists_and_is_inert_where_it_does_not() {
        assert_eq!(PanePref::Auto.press(1240.0), PanePref::Hidden);
        assert_eq!(PanePref::Hidden.press(1240.0), PanePref::Auto);
        assert_eq!(PanePref::Auto.press(1120.1), PanePref::Hidden);

        for width in [WINDOW_MIN[0], 900.0, 1120.0] {
            for pane in [PanePref::Auto, PanePref::Hidden] {
                assert_eq!(pane.press(width), pane, "{width} {pane:?}");
            }
        }
    }

    #[test]
    fn a_hidden_choice_survives_a_trip_below_the_threshold() {
        let hidden = PanePref::Auto.press(1240.0);
        assert_eq!(hidden, PanePref::Hidden);

        let narrow = hidden.press(800.0);
        assert_eq!(narrow, PanePref::Hidden, "a narrow press changed nothing");
        assert_eq!(
            regions_with(1240.0, narrow).now_playing,
            NowPlaying::Hidden,
            "the pane came back on the way up"
        );
    }

    #[test]
    fn the_reduced_playbar_never_has_a_pane_to_toggle() {
        for width in (WINDOW_MIN[0] as u32)..=(PLAYBAR_REDUCED_MAX as u32) {
            let width = width as f32;
            assert_eq!(regions_for(width).playbar, Playbar::Reduced, "{width}");
            for pane in [PanePref::Auto, PanePref::Hidden] {
                assert_eq!(
                    pane_button(width, pane),
                    PaneButton::Unavailable,
                    "{width} {pane:?}"
                );
            }
        }
    }

    #[test]
    fn a_user_hidden_pane_hands_its_width_to_the_view() {
        let (shown, _, _) = solve(1240.0, 900.0);
        let (hidden, regions, shell) = solve_with(1240.0, 900.0, PanePref::Hidden);
        assert_eq!(regions.now_playing, NowPlaying::Hidden);
        assert!(hidden.now_playing.is_none());
        assert_eq!(hidden.view.right(), 1240.0 - shell.padding);
        assert!(hidden.view.width() > shown.view.width());
    }

    #[test]
    fn the_regions_tile_the_inner_area_with_the_pane_hidden_by_choice() {
        let (grid, _, shell) = solve_with(1240.0, 900.0, PanePref::Hidden);
        let inner = content(1240.0, 900.0).shrink(shell.padding);

        let area =
            grid.sidebar.area() + grid.topbar.area() + grid.view.area() + grid.playbar.area();
        let column_gaps = shell.gap * grid.sidebar.height();
        let row_gap = shell.gap * inner.width();
        let tiled = area + column_gaps + row_gap;

        assert!(
            (tiled - inner.area()).abs() < 0.01,
            "regions cover {tiled}, inner area is {}",
            inner.area()
        );
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

        let drawer = shell_for(&regions_for(800.0));
        assert_eq!(drawer.sidebar, 0.0, "below 900 the sidebar is the drawer");
    }

    #[test]
    fn the_minimum_window_is_the_narrowest_layout() {
        let regions = regions_for(WINDOW_MIN[0]);
        assert_eq!(regions.sidebar, Sidebar::Rail);
        assert_eq!(regions.now_playing, NowPlaying::Hidden);
        assert_eq!(regions.playbar, Playbar::Reduced);
    }

    #[test]
    fn the_default_window_shows_the_now_playing_column() {
        assert_ne!(
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

        // Below 900 there is no sidebar column, so the main pane starts at the
        // inner edge and no gap is left for a column that is not there.
        let (narrow, _, _) = solve(800.0, 720.0);
        assert_eq!(narrow.sidebar.width(), 0.0);
        assert_eq!(narrow.topbar.left(), narrow.sidebar.left());
        assert!(narrow.now_playing.is_none());
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
    fn the_narrow_shell_gives_the_sidebar_to_the_drawer() {
        let (full, _, _) = solve(1440.0, 900.0);
        let (narrow, regions, shell) = solve(800.0, 900.0);
        assert_eq!(narrow.sidebar.width(), 0.0);
        assert!(narrow.sidebar.width() < full.sidebar.width());
        assert_eq!(regions.sidebar, Sidebar::Rail);
        assert_eq!(narrow.sidebar.width(), shell.sidebar);
        // The main pane starts where the sidebar would have, because there is
        // nothing there to make room for.
        assert_eq!(narrow.topbar.left(), narrow.sidebar.left());
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
        // The sidebar is the drawer at this width, so it has no width of its
        // own to check: the drawer is where it is drawn.
        let mut rects = vec![grid.topbar, grid.view, grid.playbar, grid.drawer];
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

    fn assert_close(left: f32, right: f32) {
        assert!((left - right).abs() < 0.01, "{left} is not {right}");
    }

    fn playbar_at(width: f32) -> (Rect, Playbar) {
        let regions = regions_for(width);
        let shell = shell_for(&regions);
        let grid = grid_for(content(width, 900.0), &shell);
        (grid.playbar, regions.playbar)
    }

    fn playbar_tracks_width(rect: Rect) -> f32 {
        rect.width() - 2.0 * PlaybarCols::PADDING - 2.0 * PlaybarCols::GAP
    }

    fn topbar_rect(width: f32) -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(width, extent::TOPBAR_H))
    }

    #[test]
    fn the_reduced_extras_are_152_from_the_tokens() {
        assert_eq!(playbar_extras(Playbar::Reduced), 152.0);
    }

    #[test]
    fn the_full_extras_are_216_from_the_tokens() {
        assert_eq!(playbar_extras(Playbar::Full), 216.0);
    }

    #[test]
    fn the_reduced_playbar_fits_the_minimum_window_with_56_to_spare() {
        let (rect, form) = playbar_at(WINDOW_MIN[0]);
        assert_eq!(form, Playbar::Reduced);
        let columns = playbar_for(rect, form);
        let minimums = extent::PLAYBAR_NOW_MIN_REDUCED
            + extent::PLAYBAR_CENTRE_MIN_REDUCED
            + playbar_extras(Playbar::Reduced);
        let slack = playbar_tracks_width(rect) - minimums;
        assert_close(slack, 56.0);
        assert_close(
            columns.now + columns.centre + columns.extras,
            playbar_tracks_width(rect),
        );
    }

    #[test]
    fn the_full_extras_fit_their_column_at_the_narrowest_full_width() {
        let (rect, form) = playbar_at(PLAYBAR_REDUCED_MAX + 0.1);
        assert_eq!(form, Playbar::Full);
        let columns = playbar_for(rect, form);
        assert!(columns.extras >= playbar_extras(Playbar::Full));
    }

    #[test]
    fn every_column_meets_its_minimum_at_every_width_from_the_minimum_window_up() {
        for width in (WINDOW_MIN[0] as u32)..=2000 {
            let (rect, form) = playbar_at(width as f32);
            let columns = playbar_for(rect, form);
            let (now_min, centre_min, extras_min) = match form {
                Playbar::Full => (
                    extent::PLAYBAR_NOW_MIN,
                    extent::PLAYBAR_CENTRE_MIN,
                    extent::PLAYBAR_EXTRAS_MIN,
                ),
                Playbar::Reduced => (
                    extent::PLAYBAR_NOW_MIN_REDUCED,
                    extent::PLAYBAR_CENTRE_MIN_REDUCED,
                    playbar_extras(Playbar::Reduced),
                ),
            };
            assert!(columns.now >= now_min, "{width}: now {}", columns.now);
            assert!(
                columns.centre >= centre_min,
                "{width}: centre {}",
                columns.centre
            );
            assert!(
                columns.extras >= extras_min,
                "{width}: extras {}",
                columns.extras
            );
        }
    }

    #[test]
    fn below_the_supported_width_columns_hold_their_minimums_and_overflow_right() {
        let width = WINDOW_MIN[0] - 100.0;
        let (rect, form) = playbar_at(width);
        assert_eq!(form, Playbar::Reduced);
        let columns = playbar_for(rect, form);
        assert_eq!(columns.now, extent::PLAYBAR_NOW_MIN_REDUCED);
        assert_eq!(columns.centre, extent::PLAYBAR_CENTRE_MIN_REDUCED);
        assert_eq!(columns.extras, playbar_extras(Playbar::Reduced));
        let used = columns.now
            + columns.centre
            + columns.extras
            + 2.0 * PlaybarCols::PADDING
            + 2.0 * PlaybarCols::GAP;
        assert!(used > rect.width());
    }

    #[test]
    fn playbar_columns_are_one_gap_apart_and_inside_the_rect() {
        let (rect, form) = playbar_at(1440.0);
        let [now, centre, extras] = playbar_for(rect, form).columns(rect);
        assert_eq!(now.left(), rect.left() + PlaybarCols::PADDING);
        assert_close(now.right() + PlaybarCols::GAP, centre.left());
        assert_close(centre.right() + PlaybarCols::GAP, extras.left());
        assert!(extras.right() <= rect.right() - PlaybarCols::PADDING + 0.01);
        for column in [now, centre, extras] {
            assert_eq!(column.top(), rect.top());
            assert_eq!(column.bottom(), rect.bottom());
        }
    }

    #[test]
    fn the_playbar_weights_follow_the_reference_at_1440_and_at_800() {
        let (full, form) = playbar_at(1440.0);
        let columns = playbar_for(full, form);
        let tracks = playbar_tracks_width(full);
        assert_close(columns.now, tracks / 4.0);
        assert_close(columns.centre, tracks / 2.0);
        assert_close(columns.extras, tracks / 4.0);

        let (reduced, form) = playbar_at(800.0);
        let columns = playbar_for(reduced, form);
        let flex = playbar_tracks_width(reduced) - playbar_extras(Playbar::Reduced);
        let unit = flex / 2.5;
        assert_close(columns.now, unit);
        assert_close(columns.centre, 1.5 * unit);
    }

    #[test]
    fn the_search_field_is_capped_at_its_maximum() {
        let columns = topbar_for(topbar_rect(1440.0), Sidebar::Full, 100.0);
        assert_eq!(columns.search, extent::SEARCH_MAX_W);
    }

    #[test]
    fn the_search_field_yields_to_the_trail_before_the_trail_truncates() {
        let rect = topbar_rect(1100.0);
        let free = topbar_free(rect, Sidebar::Full);

        let natural = free - 200.0;
        let columns = topbar_for(rect, Sidebar::Full, natural);
        assert_close(columns.search, 200.0);
        assert_close(columns.trail, natural);

        let natural = free - 50.0;
        let columns = topbar_for(rect, Sidebar::Full, natural);
        assert_eq!(columns.search, extent::SEARCH_MIN_W);
        assert!(columns.trail < natural);
        assert_close(columns.search + columns.trail, free);
    }

    #[test]
    fn the_trail_never_overlaps_the_search_field() {
        for width in 640..=2000 {
            let rect = topbar_rect(width as f32);
            let sidebar = regions_for(width as f32).sidebar;
            let free = topbar_free(rect, sidebar);
            let columns = topbar_for(rect, sidebar, 300.0);
            assert!(
                columns.search + columns.trail <= free + 0.01,
                "{width}: search {} + trail {} beats {free}",
                columns.search,
                columns.trail
            );
        }
    }

    #[test]
    fn the_toggle_exists_only_on_the_rail() {
        let full = topbar_for(topbar_rect(1440.0), Sidebar::Full, 200.0);
        let rail = topbar_for(topbar_rect(800.0), Sidebar::Rail, 200.0);
        assert_eq!(full.toggle, None);
        assert_eq!(rail.toggle, Some(extent::BUTTON));
        assert!(full.columns(topbar_rect(1440.0)).toggle.is_none());
        assert!(rail.columns(topbar_rect(800.0)).toggle.is_some());
    }

    #[test]
    fn the_trail_is_flush_right_against_the_padding() {
        let rect = topbar_rect(1440.0);
        let columns = topbar_for(rect, Sidebar::Full, 200.0).columns(rect);
        assert_close(columns.trail.right(), rect.right() - TopbarCols::PADDING);
    }
}
