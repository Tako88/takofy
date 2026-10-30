//! Window geometry.
//!
//! Breakpoints and region visibility live here and nowhere else, so no view
//! measures the window and there is nothing to keep in sync.

use egui::{Rect, pos2, vec2};

use crate::theme::{extent, space, stroke};

pub mod actionbar;
pub mod cards;
pub mod chips;
pub mod device_list;
pub mod episode;
pub mod filter_row;
pub mod hero;
pub mod menu;
pub mod panel;
pub mod playbar;
pub mod queue;
pub mod search_menu;
pub mod section;
pub mod sticky;
pub mod table;
pub mod tracks;

use tracks::{Track, solve};

/// Recommended starting size of the window, in points.
/// Wide enough that the Now Playing column is on screen in a window nobody has
/// touched: a default that hides a region is a region a user never finds.
pub const WINDOW_DEFAULT: [f32; 2] = [1240.0, 720.0];
/// Smallest the window may be dragged, in points.
pub const WINDOW_MIN: [f32; 2] = [640.0, 500.0];

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
                shape: table::Shape::Playlist,
                artist: true,
                album: true,
                added: true,
                like: true,
                duration: true,
                actions: true,
            },
            Self::Reduced => table::Present {
                shape: table::Shape::Playlist,
                artist: true,
                album: false,
                added: true,
                like: true,
                duration: true,
                actions: true,
            },
            Self::Narrow => table::Present {
                shape: table::Shape::Playlist,
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

/// The width of the topbar's separator slot, in points.
///
/// The mark is a hairline, so the slot is the mark and the `space-1` margin
/// that keeps it clear of the buttons on either side.
pub const HISTORY_RULE: f32 = stroke::HAIRLINE + 2.0 * space::S1;

/// The width of the topbar's nav cluster, in points.
///
/// Four buttons, a `space-1` gap between them, and the rule's whole slot. The
/// cluster is one reservation, so the layout and the row that draws it cannot
/// disagree about how much room it takes.
pub fn history_width() -> f32 {
    4.0 * extent::BUTTON + 4.0 * space::S1 + HISTORY_RULE
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
        - history_width()
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
    let history = history_width();
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
mod tests;
