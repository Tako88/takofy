//! The token layer.
//!
//! `clippy.toml` denies the `egui::Color32` constructors crate-wide, and this is
//! the one file where that allow is lifted — a literal colour anywhere else is a
//! build failure rather than a review comment.
//!
//! Every value here is transcribed from `docs/reference/styles/tokens.css`,
//! including the steps whose consumers have not been written yet. The token layer
//! is the design system, not a list of what happens to be in use: a scale is
//! defined once here so that no component ever invents a value of its own.

#![allow(clippy::disallowed_methods, reason = "this file is the token layer")]

pub mod ease;
mod egui_style;
pub mod fonts;

pub use egui_style::style;

use egui::Color32;

/// The spacing scale.
pub mod space {
    pub const S1: f32 = 4.0;
    pub const S2: f32 = 8.0;
    pub const S3: f32 = 12.0;
    pub const S4: f32 = 16.0;
    pub const S5: f32 = 24.0;
    pub const S6: f32 = 32.0;
    pub const S7: f32 = 48.0;
    pub const S8: f32 = 64.0;
    /// The reference's 1 px gaps, which are not a step on the scale.
    pub const PX: f32 = 1.0;
    /// The gap between major regions, as opposed to within one.
    pub const GUTTER: f32 = S5;
}

/// Nearly square throughout: roundness is what this design gives up.
pub mod radius {
    use egui::CornerRadius;

    /// Square corners: the one place a rule is drawn as a bar, not a chip.
    pub const NONE: CornerRadius = CornerRadius::same(0);
    pub const XS: CornerRadius = CornerRadius::same(1);
    pub const SM: CornerRadius = CornerRadius::same(2);
    pub const MD: CornerRadius = CornerRadius::same(3);
    pub const LG: CornerRadius = CornerRadius::same(4);
    /// A pill. egui clamps a radius larger than the rectangle.
    pub const PILL: CornerRadius = CornerRadius::same(u8::MAX);
}

/// Type sizes of the interface face, in points.
pub mod text {
    pub const XXS: f32 = 10.0;
    pub const XS: f32 = 11.0;
    pub const SM: f32 = 12.0;
    pub const BASE: f32 = 13.0;
    pub const MD: f32 = 15.0;
    pub const LG: f32 = 18.0;
    pub const XL: f32 = 22.0;
    pub const XXL: f32 = 28.0;
    pub const XXXL: f32 = 40.0;
    pub const XXXXL: f32 = 56.0;
}

/// Line height as a multiple of the type size.
pub mod leading {
    pub const TIGHT: f32 = 1.05;
    pub const SNUG: f32 = 1.25;
    pub const NORMAL: f32 = 1.5;
}

/// Letter spacing in em.
pub mod tracking {
    pub const TIGHT: f32 = -0.02;
    /// The reference writes this as a bare `0.02em`; it is not a named token
    /// there, but it appears six times, so it is one here.
    pub const SLIGHT: f32 = 0.02;
    pub const WIDE: f32 = 0.06;
    pub const CAPS: f32 = 0.11;
}

/// The weights the bundled faces carry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Weight {
    Regular,
    Medium,
    Semibold,
    Bold,
    Black,
}

impl Weight {
    pub const ALL: [Self; 5] = [
        Self::Regular,
        Self::Medium,
        Self::Semibold,
        Self::Bold,
        Self::Black,
    ];

    pub fn value(self) -> f32 {
        match self {
            Self::Regular => 400.0,
            Self::Medium => 500.0,
            Self::Semibold => 600.0,
            Self::Bold => 700.0,
            Self::Black => 800.0,
        }
    }
}

/// The three weights JetBrains Mono has a face for. Numbers and field labels use
/// it, and it is the only face with Cyrillic and Greek coverage.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MonoWeight {
    Regular,
    Medium,
    Bold,
}

impl MonoWeight {
    pub const ALL: [Self; 3] = [Self::Regular, Self::Medium, Self::Bold];

    pub fn value(self) -> f32 {
        match self {
            Self::Regular => 400.0,
            Self::Medium => 500.0,
            Self::Bold => 700.0,
        }
    }
}

/// Fixed dimensions that are not on the spacing scale.
///
/// Not called `layout`, because that is the module that decides which regions a
/// window shows.
pub mod extent {
    pub const SIDEBAR_W: f32 = 256.0;
    pub const NOWPLAYING_W: f32 = 312.0;
    pub const NOWPLAYING_NARROW_W: f32 = 272.0;
    pub const PLAYBAR_H: f32 = 64.0;
    pub const TOPBAR_H: f32 = 56.0;
    pub const ROW_H: f32 = 44.0;
    /// The two bars a placeholder is made of: the reference's 0.5rem and
    /// 0.375rem hairlines of held ink.
    pub const SKELETON_LINE: f32 = 8.0;
    pub const SKELETON_LINE_SM: f32 = 6.0;
    pub const BUTTON: f32 = 28.0;
    pub const BUTTON_LG: f32 = 34.0;
    /// The hit area a small control grows to while it is a drop target, 2.75 rem.
    /// Nothing is drawn at this size: the glyph keeps its own box.
    pub const DROP_HIT: f32 = 44.0;
    pub const SLIDER_H: f32 = 14.0;
    pub const SLIDER_TRACK_H: f32 = 3.0;
    pub const SLIDER_THUMB_W: f32 = 3.0;
    pub const CHIP_PAD_X: f32 = 7.2;
    pub const CHIP_PAD_Y: f32 = 3.2;
    pub const FOCUS_OFFSET: f32 = 2.0;
    /// The reference's scrollbar thumb: 4pt of bone, never a wider bar that
    /// expands on hover.
    pub const SCROLLBAR: f32 = 4.0;

    /// The volume slider's width, 4.5 rem.
    pub const VOLUME_W: f32 = 72.0;
    /// The playbar's cover tile, 2.75 rem.
    pub const PLAYBAR_ART: f32 = 44.0;
    /// A library row's cover tile, 2.25 rem. A track row's art is the same
    /// 2.25 rem box, so it is this token and not a second one of its own.
    pub const LIBRARY_ART: f32 = 36.0;
    /// A queue row's cover tile, 2.5 rem. A different box from a library row's,
    /// because the panel's rows are the reference's own grid.
    pub const QUEUE_ART: f32 = 40.0;
    /// The Now Playing art's floor, 4 rem. The art gives way to the pane's own
    /// minimum at the shortest window the panel is drawn at, and stops here.
    pub const NOWPLAYING_ART_MIN: f32 = 64.0;
    /// An episode row's pitch: its padding, the title, a two-row description and
    /// the meta line. Taller than a track row because the description is part of
    /// the row, and fixed so the list can be virtualised from its index alone.
    pub const EPISODE_ROW_H: f32 = 120.0;
    /// An episode row's play toggle, 2.5 rem: the square the row opens with,
    /// which stands where a track row puts its art.
    pub const EPISODE_TOGGLE: f32 = 40.0;
    /// The measure an episode description is clamped to, in characters. The
    /// reference caps it at `70ch` so a wide window does not stretch a paragraph
    /// from edge to edge.
    pub const EPISODE_TEXT_CH: f32 = 70.0;
    /// The search field's height, 2 rem.
    pub const SEARCH_H: f32 = 32.0;
    /// The search field's flex basis, 14 rem.
    pub const SEARCH_BASIS_W: f32 = 224.0;
    /// The search field's maximum width, 24 rem.
    pub const SEARCH_MAX_W: f32 = 384.0;
    /// The search field's floor. Not a reference value: a round 8 rem chosen
    /// on the reference's own scale, to be tuned at the minimum window.
    pub const SEARCH_MIN_W: f32 = 128.0;
    /// The gap between the search field and its drop-down, 6px. The reference
    /// writes it as `calc(100% + 6px)`: a bare value, not a step on the scale.
    pub const MENU_GAP: f32 = 6.0;
    /// A search menu row's cover tile, 2.25 rem. The same box as a library
    /// row's art, named for the row that reads it.
    pub const MENU_COVER: f32 = 36.0;
    /// A band heading's box: the reference's `8 / 12 / 4` padding about one
    /// line of mono caps.
    pub const MENU_HEAD_H: f32 =
        super::space::S2 + super::text::XXS * super::leading::NORMAL + super::space::S1;
    /// The narrowest title column a menu row keeps while it also draws
    /// `MENU_COVER`. Below it the row drops the cover and hands its width to the
    /// text: a narrow row loses a column before it loses a line. Not a reference
    /// value — the reference's own narrow-window rule, expressed here as the
    /// room about ten characters of `text::BASE` need.
    pub const MENU_TEXT_MIN: f32 = 72.0;
    /// A command menu's width floor and cap, the sheet's 12 rem and 20 rem. A
    /// menu of results takes the field's width instead; only a menu of verbs
    /// sizes to its longest one.
    pub const MENU_MIN_W: f32 = 192.0;
    pub const MENU_MAX_W: f32 = 320.0;
    /// The list rows a submenu shows before its own scroll takes over. The
    /// sheet's number: more lists than this are found by typing, not scrolling.
    pub const SUBMENU_ROWS: usize = 8;
    /// The seek row's maximum width, 32 rem, readouts included.
    pub const PROGRESS_MAX_W: f32 = 512.0;
    /// The time readout's column, 2.5 rem.
    pub const TIME_W: f32 = 40.0;
    /// The Now Playing art's maximum side, 16 rem.
    pub const NOWPLAYING_ART_MAX: f32 = 256.0;

    /// The playbar's minimum columns: 11 rem, 18 rem, 11 rem.
    pub const PLAYBAR_NOW_MIN: f32 = 176.0;
    pub const PLAYBAR_CENTRE_MIN: f32 = 288.0;
    pub const PLAYBAR_EXTRAS_MIN: f32 = 176.0;
    /// The reduced playbar's minimums: 9 rem and 14 rem.
    pub const PLAYBAR_NOW_MIN_REDUCED: f32 = 144.0;
    pub const PLAYBAR_CENTRE_MIN_REDUCED: f32 = 224.0;

    /// A track row's fixed columns, from the reference's `--track-cols` for a
    /// playlist: the index and the art are 2.25 rem, the date added is 5.5 rem,
    /// the duration is 3.5 rem, and the like and the actions are 1.75 rem each.
    pub const TRACK_INDEX_W: f32 = 36.0;
    pub const TRACK_ADDED_W: f32 = 88.0;
    pub const TRACK_LIKE_W: f32 = 28.0;
    pub const TRACK_DURATION_W: f32 = 56.0;
    pub const TRACK_ACTIONS_W: f32 = 28.0;
    /// The box a row's number or glyph sits in, 1.5 rem. The reference
    /// right-justifies it inside the wider index column.
    pub const TRACK_NUMBER_W: f32 = 24.0;
    /// The list density's Type and Size cells, 9 rem and 5 rem. The reference
    /// draws them only in that density, which is why they are not track columns.
    pub const TYPE_W: f32 = 144.0;
    pub const SIZE_W: f32 = 80.0;

    /// A card's minimum width, 9 rem, which is what the grid's auto-fill counts.
    pub const CARD_MIN_W: f32 = 144.0;
    /// The library grid's minimum tile width: the reference's `11rem`, which is
    /// wider than the release grid's `9rem`.
    pub const MEDIA_MIN_W: f32 = 176.0;
    /// A card's play tile, 2 rem.
    pub const CARD_PLAY: f32 = 32.0;
    /// A release block's art, 7 rem.
    pub const RELEASE_ART: f32 = 112.0;
    /// A release block's own play tile, 2.5 rem, which is the reference's
    /// `.play-button` rather than the hero's larger call to action.
    pub const RELEASE_PLAY: f32 = 40.0;
    /// The sign-in card's width. Not a reference value: the reference draws no
    /// sign-in card. 24 rem is the measure it gives a narrow field, and it
    /// leaves a margin to spare at the minimum window.
    pub const SIGNIN_W: f32 = 384.0;
    /// A bordered button's box padding, 0.35 rem above and below its label. It is
    /// not a step on the spacing scale — the reference writes it as a bare value
    /// and uses it for every button.
    pub const BUTTON_PAD_Y: f32 = 5.6;
    /// The gap from the bottom of a link's line box to its underline.
    pub const LINK_UNDERLINE_INSET: f32 = 2.0;
}

/// How the free space in a row of columns is shared, in the reference's
/// fractional units.
///
/// A row's fixed columns take their width and these share what is left, so their
/// ratio decides how a track list reads: the title is what a listener scans.
///
/// A playlist's template is dense: it carries the artist and the album, and pays
/// for them with a wider title and tighter gutters. The plays template is tight:
/// its artist takes a narrow share of the title, and the row keeps the wider
/// gutter instead of the playlist's.
///
/// A nested title's gutter is not a token. The reference composes it as a grid
/// track and a gap, so the sum is not a value it names; whoever lays the title
/// out writes `extent::LIBRARY_ART + space::S3` and invents nothing.
pub mod grow {
    /// The plays and slim rows' title, `1fr`. The playlist reads
    /// `grow::TITLE_PLAYLIST` instead.
    pub const TITLE: f32 = 1.0;
    /// The album column's weight on a row with no credit template, `0.7fr`.
    ///
    /// No constructor produces this today: a slim row clears the album, a plays
    /// row reads `grow::CREDIT_PLAYS`, and the playlist reads
    /// `grow::ALBUM_PLAYLIST`. It is kept for a hand-built
    /// `Present { album: true, shape: Slim }`. Being `pub`, it is not dead-by-lint,
    /// so no warning points at the unpinned branch.
    pub const ALBUM: f32 = 0.7;
    /// The playlist's denser template: title `1.35fr`, artist and album `0.9fr`.
    pub const TITLE_PLAYLIST: f32 = 1.35;
    /// The playlist's artist share, `0.9fr`. The plays row reads
    /// `grow::CREDIT_PLAYS` instead.
    pub const ARTIST: f32 = 0.9;
    /// The plays row's one credit column, `0.55fr`: the reference's
    /// `.tracks--plays` `minmax(0, 0.55fr)`, measured against a `1fr` title.
    ///
    /// A plays row credits one thing — an album's table names the artist, an
    /// artist's table names the album — so this is the share that credit takes
    /// whichever column carries it.
    pub const CREDIT_PLAYS: f32 = 0.55;
    /// The playlist's album share, `0.9fr`.
    pub const ALBUM_PLAYLIST: f32 = 0.9;
}

/// A header band: its height, its art, and the clamp its title is drawn at. The
/// artist band replaces the art tile with a plate and adds a shelf beside it.
pub mod hero {

    /// The band's minimum height, 11 rem.
    pub const MIN_H: f32 = 176.0;
    /// The band's art tile, 11 rem. The same value as the band's own height, and
    /// a different role: the tile is square and the band may grow taller.
    pub const ART: f32 = 176.0;
    /// How many rows a playlist description may take before it elides.
    ///
    /// The reference lets the description wrap without a bound; the band's height
    /// is fixed, so the words are clamped to the two rows that fit it.
    pub const DESCRIPTION_ROWS: usize = 2;
    /// The play tile under the band, 2.75 rem. Larger than a card's, because it
    /// is the band's own call to action.
    pub const PLAY: f32 = 44.0;
    /// The title's floor, 2 rem, which is not a step on the type scale: the
    /// reference clamps the size rather than choosing one of the steps.
    pub const TITLE_MIN: f32 = 32.0;
    /// The title's share of the viewport width, 4.5 vw, as a fraction of it.
    pub const TITLE_VW: f32 = 0.045;
    /// The artist band's sleeve, 16 rem, the narrow tier's 13 rem and the stacked
    /// tier's 6.5 rem. The sleeve also holds the band open, so it is one number.
    pub const ARTIST_PLATE: f32 = 256.0;
    pub const ARTIST_PLATE_NARROW: f32 = 208.0;
    pub const ARTIST_PLATE_PHONE: f32 = 104.0;
    /// The artist title: `clamp(1.75rem, 4vw, 3.25rem)`, and the phone tier's
    /// `clamp(1.75rem, 10vw, 2.75rem)`.
    pub const ARTIST_TITLE_MIN: f32 = 28.0;
    pub const ARTIST_TITLE_VW: f32 = 0.04;
    pub const ARTIST_TITLE_MAX: f32 = 52.0;
    pub const ARTIST_TITLE_PHONE_MIN: f32 = 28.0;
    pub const ARTIST_TITLE_PHONE_VW: f32 = 0.10;
    pub const ARTIST_TITLE_PHONE_MAX: f32 = 44.0;
    /// How many rows the artist's name may take before it elides.
    pub const ARTIST_TITLE_ROWS: usize = 3;
}

/// The action row under a band: the row of things that act on the whole context.
///
/// The reference's `.actionbar` rests at `padding-block: var(--space-4)` around a
/// 44-point play tile and condenses to `var(--space-2)` once it pins. The pinned
/// height is the reference's `--actionbar-h`, 3.75 rem, which is the number it
/// documents for anything sticky underneath to clear. The hairline a pinned row
/// wears is painted inside that height rather than under it, so a row that clears
/// the bar's bottom clears the rule as well.
pub mod actionbar {
    use super::{hero, space};

    /// The row's block padding at rest, `space-4`.
    pub const PAD_REST: f32 = space::S4;
    /// The row's block padding once it pins, `space-2`.
    pub const PAD_STUCK: f32 = space::S2;
    /// The height the row holds in the page's flow: the resting row.
    pub const REST_H: f32 = hero::PLAY + 2.0 * PAD_REST;
    /// The height the row takes once it pins: `--actionbar-h`.
    pub const STUCK_H: f32 = hero::PLAY + 2.0 * PAD_STUCK;
    /// The gap after the play tile, `space-5`.
    pub const GAP: f32 = space::S5;
    /// The gap between two tools, `space-2`.
    pub const TOOLS_GAP: f32 = space::S2;
    /// The width the playlist's filter field asks for in the row, 11 rem.
    pub const FIELD_W: f32 = 176.0;
    /// The narrowest the field is drawn. Below this there is no field, so an
    /// unseen query never narrows the list.
    pub const FIELD_MIN_W: f32 = 120.0;
}

/// The discography's chip row carries a count and a name field at its trailing
/// edge. The field is wider here than the action row's (`15rem`, not `11rem`) and
/// does not shrink: the chips wrap around it instead.
pub mod cluster {
    use super::{extent, space};

    /// The field's column, `15rem`, which it keeps at every width.
    pub const FIELD_W: f32 = 240.0;
    /// The field's height: the same 1.75rem row the other fields use.
    pub const FIELD_H: f32 = extent::BUTTON;
    /// The space between the count and the field, `space-4`.
    pub const GAP: f32 = space::S4;
}

/// The equaliser that stands in for a playing row's number.
///
/// Three bars, each starting its pulse a little after the one before it, so the
/// group reads as one movement rather than three. A held track stops the pulse
/// where it is; it does not clear the bars, because the track is still the one
/// the transport is on.
pub mod eq {
    /// How many bars there are, left to right.
    pub const BARS: usize = 3;
    /// A bar's width, 2 px.
    pub const BAR_W: f32 = 2.0;
    /// The gap between two bars, 2 px.
    pub const GAP: f32 = 2.0;
    /// The tallest a bar is drawn, 0.8125 rem.
    pub const H: f32 = 13.0;
    /// How far behind the previous bar each bar starts, 220 ms.
    pub const DELAY: f32 = 0.22;
    /// One pulse, 900 ms.
    pub const PULSE: f32 = 0.9;
    /// How often the pulse asks for a redraw, in seconds: one frame at 60 Hz.
    /// The pulse is the one thing on screen that is never still, and the window's
    /// steady redraw is an order of magnitude slower than this.
    pub const FRAME: f32 = 0.016;
    /// The shortest a bar gets, a quarter of its height. A bar shrinks and grows
    /// again; it never disappears.
    pub const MIN_SCALE: f32 = 0.25;
}

/// The sprite's own geometry.
///
/// The reference draws every glyph on a 24-unit viewBox and strokes it at a
/// width in those units, so the drawn width follows the size: doubling the icon
/// doubles the stroke. `side` is in points, `stroke` is in grid units.
pub mod icon {
    /// The reference sprite's `viewBox` is `0 0 24 24` for every symbol.
    pub const GRID: f32 = 24.0;

    #[derive(Clone, Copy, PartialEq, Debug)]
    pub struct Size {
        pub side: f32,
        pub stroke: f32,
    }

    impl Size {
        pub const SM: Self = Self {
            side: 12.0,
            stroke: 1.8,
        };
        pub const BASE: Self = Self {
            side: 16.0,
            stroke: 1.8,
        };
        pub const MD: Self = Self {
            side: 18.0,
            stroke: 1.6,
        };
        pub const LG: Self = Self {
            side: 22.0,
            stroke: 1.6,
        };

        /// The width this size draws, in points.
        pub fn stroke_pts(self) -> f32 {
            self.stroke * self.side / GRID
        }
    }
}

/// How a decoded cover reaches the screen.
pub mod art {
    use egui::{Color32, TextureOptions};

    /// The tint a textured rect multiplies its image by. Such a rect still needs
    /// a fill even when it is nothing but a picture, and white is the one value
    /// that leaves the pixels as they were decoded.
    pub const TINT: Color32 = Color32::WHITE;

    /// A cover is scaled, never tiled: smooth filters both ways, and the edge is
    /// clamped. A stretched image that sampled the opposite edge would show a
    /// smeared rim on the tile's last pixel.
    ///
    /// No mipmaps: the decoder already brings every cover down to a size close to
    /// the tile, so the cost would buy nothing.
    pub const TEXTURE: TextureOptions = TextureOptions::LINEAR;
}

/// Durations in seconds, and the one easing curve the design uses.
pub mod motion {
    pub const FAST: f32 = 0.110;
    pub const BASE: f32 = 0.180;
    pub const SLOW: f32 = 0.300;
    pub const EASE_OUT: [f32; 4] = [0.3, 0.0, 0.0, 1.0];
    /// One pass of the light across a placeholder, and the offset that keeps a
    /// row of them from sweeping as a single sheet.
    pub const SHEEN: f32 = 1.700;
    pub const STAGGER: f32 = 0.120;
}

pub mod scroll {
    use super::space::S8;

    /// How far one line of wheel travel moves the content. A row is 44, so a
    /// notch clears a row and a bit rather than most of one.
    pub const POINTS_PER_LINE: f32 = 60.0;
    /// How near the edge of a scrolling area a carried row has to be held
    /// before the area follows it: one spacing step.
    pub const DRAG_EDGE: f32 = S8;
    /// The fastest a drag scrolls, in points per second, at the edge itself.
    pub const DRAG_MAX: f32 = 900.0;
}

/// How far a thing is set back from the surface it sits on.
pub mod opacity {
    /// A row in flight: still legible, clearly not on the page.
    pub const LIFTED: f32 = 0.55;
    /// The scrollbar at rest: present, but it never competes with a row.
    pub const SCROLL_REST: f32 = 0.16;
    /// The scrollbar under the pointer, or being dragged.
    pub const SCROLL_HOVER: f32 = 0.30;
}

pub mod stroke {
    /// The one width a divider or a link's underline is allowed to be.
    pub const HAIRLINE: f32 = 1.0;
    /// The one width a marker is allowed to be: the flag at the leading edge of
    /// the selected row, the current breadcrumb and the sounding row.
    pub const RULE: f32 = 2.0;
}

/// The band tint's knobs, so one mean yields a colour for any palette.
///
/// The derivation reads the artwork's mean in HSL, normalises it to a tone that
/// reads as a tinted header, and steps that tone down until the band clears the
/// ink drawn on it. A light theme supplies another value of these, not another
/// derivation; light mode is not built, only expressible.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TintParams {
    /// The lowest alpha a pixel needs to count toward the mean.
    pub alpha_floor: u8,
    /// The lightness the narrowest tone maps to.
    pub tone_offset: f64,
    /// How far the lightness moves from the offset across the tone range. Light
    /// mode mirrors the scale, so its slope is negative.
    pub tone_slope: f64,
    /// The narrowest tone the artwork's lightness may ask for.
    pub tone_min: f64,
    /// The widest tone it may ask for.
    pub tone_max: f64,
    /// Below this saturation the mean is grey, and the band stays neutral.
    pub neutral_below: f64,
    /// How far the mean's saturation is pushed past a photograph's near-grey.
    pub saturation_gain: f64,
    /// The least saturation a band is given.
    pub saturation_min: f64,
    /// The most saturation a band is given.
    pub saturation_max: f64,
    /// The WCAG relative luminance a band must stay at or under.
    pub luminance_ceiling: f64,
    /// How far one step down the lightness scale moves.
    pub lightness_step: f64,
    /// The lightness the stepping stops at.
    pub lightness_floor: f64,
}

impl TintParams {
    pub const DARK: Self = Self {
        alpha_floor: 8,
        tone_offset: 0.0,
        tone_slope: 1.0,
        tone_min: 0.16,
        tone_max: 0.28,
        neutral_below: 0.03,
        saturation_gain: 2.2,
        saturation_min: 0.32,
        saturation_max: 0.62,
        luminance_ceiling: 0.05,
        lightness_step: 0.015,
        lightness_floor: 0.04,
    };
}

/// The only `Color32` values in the codebase.
///
/// A palette is a value rather than a branch: a component reads
/// `tokens.palette.text` and never asks which theme is on. Adding a light theme
/// is a second constructor here, not a change to any view.
pub struct Palette {
    /// Which base egui paints from, so `dark_mode` is never set by hand.
    pub dark: bool,
    /// Behind every panel.
    pub window: Color32,
    /// Panels sitting on the window.
    pub surface: Color32,
    /// Popups and menus, above `surface`.
    pub surface_raised: Color32,
    /// The fill under the pointer on a control — an icon button, the clear
    /// button — and on egui's own widgets. Rows hover with `row_hover` and are
    /// picked with `row_selected`.
    pub surface_hover: Color32,
    /// Wells that read as deeper than the window.
    pub surface_sunken: Color32,
    /// The raised panel a Now Playing pane will sit on.
    pub panel: Color32,
    /// Hairlines and dividers.
    pub border: Color32,
    /// A divider that has to be seen.
    pub border_strong: Color32,
    /// What sits between the window and a modal.
    pub scrim: Color32,
    /// A placeholder's fill: the type's own bone held at 9%, so a skeleton sits
    /// inside the palette rather than reading as a grey box dropped on top of it.
    pub skeleton: Color32,
    /// The light that crosses a placeholder.
    pub skeleton_sheen: Color32,
    /// A placeholder's text bar: one step brighter than its fill, because a line
    /// of type is what it stands in for.
    pub skeleton_rule: Color32,
    /// Primary text.
    pub text: Color32,
    /// Secondary text: artist, album, duration, placeholder.
    pub text_muted: Color32,
    /// The third tier, for text that may be dimmed rather than read.
    pub text_subtle: Color32,
    /// Present but not usable: a waiting control's glyph.
    pub text_disabled: Color32,
    /// Text on a light fill.
    pub text_inverse: Color32,
    /// Sound is coming out. Never a surface. Pistachio rather than mint, so the
    /// signal carries no cyan; ~7:1 on the surface, and black text sits on it
    /// when it is a tile.
    pub accent: Color32,
    pub accent_hover: Color32,
    /// A selected chip's fill: bone at low alpha, not a gamma-scaled text.
    pub pressed: Color32,
    /// The fill under the pointer on a row, bone at 6%.
    pub row_hover: Color32,
    /// The fill under a picked row, bone at 16%: the strongest wash a row takes.
    pub row_selected: Color32,
    /// The rule that shows where a carried row will land. Opaque bone, never
    /// the accent.
    pub mark: Color32,
    /// The band's fill before its art has arrived, and whenever it has none:
    /// the reference's `--color-hero-tint`, painted opaque.
    pub hero_wash: Color32,
    /// The band tint's parameters, so a light palette can re-normalise a mean
    /// that is already stored rather than fetch the cover again.
    pub tint: TintParams,
    /// A verified mark, which is not the playback colour.
    pub verified: Color32,
    pub shadow_sm: egui::Shadow,
    pub shadow_md: egui::Shadow,
    pub shadow_lg: egui::Shadow,
}

impl Palette {
    pub const DARK: Self = Self {
        dark: true,
        window: Color32::from_rgb(0x00, 0x00, 0x00),
        surface: Color32::from_rgb(0x07, 0x07, 0x08),
        surface_raised: Color32::from_rgb(0x12, 0x12, 0x15),
        surface_hover: Color32::from_rgb(0x1c, 0x1c, 0x20),
        surface_sunken: Color32::from_rgb(0x00, 0x00, 0x00),
        panel: Color32::from_rgb(0x0b, 0x0b, 0x0d),
        // The border is a bone hairline at 11% and 26%; the scrim is 78% black.
        border: Color32::from_rgba_unmultiplied_const(0xf2, 0xf0, 0xea, 28),
        border_strong: Color32::from_rgba_unmultiplied_const(0xf2, 0xf0, 0xea, 66),
        scrim: Color32::from_rgba_unmultiplied_const(0x00, 0x00, 0x00, 199),
        skeleton: Color32::from_rgba_unmultiplied_const(0xf2, 0xf0, 0xea, 23),
        skeleton_sheen: Color32::from_rgba_unmultiplied_const(0xf2, 0xf0, 0xea, 43),
        skeleton_rule: Color32::from_rgba_unmultiplied_const(0xf2, 0xf0, 0xea, 46),
        text: Color32::from_rgb(0xf2, 0xf0, 0xea),
        text_muted: Color32::from_rgb(0xb0, 0xac, 0xa4),
        text_subtle: Color32::from_rgb(0x83, 0x80, 0x7a),
        text_disabled: Color32::from_rgb(0x4d, 0x4c, 0x48),
        text_inverse: Color32::from_rgb(0x0b, 0x0b, 0x0c),
        accent: Color32::from_rgb(0x7d, 0xa5, 0x66),
        accent_hover: Color32::from_rgb(0x93, 0xbb, 0x7c),
        pressed: Color32::from_rgba_unmultiplied_const(0xf2, 0xf0, 0xea, 20),
        row_hover: Color32::from_rgba_unmultiplied_const(0xf2, 0xf0, 0xea, 15),
        row_selected: Color32::from_rgba_unmultiplied_const(0xf2, 0xf0, 0xea, 41),
        mark: Color32::from_rgb(0xf2, 0xf0, 0xea),
        hero_wash: Color32::from_rgb(0x2a, 0x2a, 0x31),
        tint: TintParams::DARK,
        verified: Color32::from_rgb(0x9f, 0xc6, 0xe8),
        shadow_sm: egui::Shadow {
            offset: [0, 1],
            blur: 2,
            spread: 0,
            color: Color32::from_rgba_unmultiplied_const(0x00, 0x00, 0x00, 128),
        },
        shadow_md: egui::Shadow {
            offset: [0, 6],
            blur: 20,
            spread: 0,
            color: Color32::from_rgba_unmultiplied_const(0x00, 0x00, 0x00, 140),
        },
        shadow_lg: egui::Shadow {
            offset: [0, 20],
            blur: 50,
            spread: 0,
            color: Color32::from_rgba_unmultiplied_const(0x00, 0x00, 0x00, 166),
        },
    };
}

/// The colour a band paints from a mean the tint derivation has normalised.
///
/// `Color32` is built here because this is the token layer: the derivation and
/// the store work in plain bytes.
pub(crate) fn tint_color(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// What a theme can vary. Everything else — geometry, type, motion — is a
/// constant module, because no theme changes it.
pub struct Tokens {
    pub palette: Palette,
}

impl Tokens {
    pub const DARK: Self = Self {
        palette: Palette::DARK,
    };
}

#[cfg(test)]
mod tests;
