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
    /// The progress bar's maximum width, 32 rem.
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
}

/// How the free space in a row of columns is shared, in the reference's
/// fractional units.
///
/// A row's fixed columns take their width and these share what is left, so their
/// ratio decides how a track list reads: the title is what a listener scans.
///
/// A playlist is the one list whose credit changes track to track, so it carries
/// an artist column and a denser set of weights and gutters to pay for it.
///
/// A nested title's gutter is not a token. The reference composes it as a grid
/// track and a gap, so the sum is not a value it names; whoever lays the title
/// out writes `extent::LIBRARY_ART + space::S3` and invents nothing.
pub mod grow {
    /// The title column of a library or album-block list, `1fr`.
    pub const TITLE: f32 = 1.0;
    /// The album column of a library list, `0.7fr`.
    pub const ALBUM: f32 = 0.7;
    /// The playlist's denser template: title `1.35fr`, artist and album `0.9fr`.
    pub const TITLE_PLAYLIST: f32 = 1.35;
    pub const ARTIST: f32 = 0.9;
    pub const ALBUM_PLAYLIST: f32 = 0.9;
}

/// A header band: its height, its art, and the clamp its title is drawn at.
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
    /// The artist band's minimum height, 17 rem: the reference's artist hero is
    /// taller than a playlist's band because the photograph carries it.
    pub const ARTIST_MIN_H: f32 = 272.0;
    /// The artist title's floor, 2.25 rem, and its cap, 4.5 rem: the reference
    /// clamps the artist's name wider than a playlist's.
    pub const ARTIST_TITLE_MIN: f32 = 36.0;
    pub const ARTIST_TITLE_MAX: f32 = 72.0;
    /// The artist title's share of the viewport width, 5.5 vw, as a fraction.
    pub const ARTIST_TITLE_VW: f32 = 0.055;
    /// Where the artist scrim's middle stop sits down the hero, 45%.
    pub const SCRIM_MID_STOP: f32 = 0.45;
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
    /// The one width a divider is allowed to be.
    pub const HAIRLINE: f32 = 1.0;
    /// The one width a marker is allowed to be: the flag at the leading edge of
    /// the selected row, the current breadcrumb and the sounding row.
    pub const RULE: f32 = 2.0;
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
    /// The wash a header band sits on. The reference writes this as `#3a3a3a`
    /// under a 0.4 opacity; a fill has one channel to carry that, so it is
    /// folded into the alpha here.
    pub hero_wash: Color32,
    /// The artist hero's photo scrim at its top edge: black at 32%.
    pub hero_scrim_top: Color32,
    /// The artist hero's photo scrim at its middle: black at 58%, where the
    /// name sits.
    pub hero_scrim_mid: Color32,
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
        hero_wash: Color32::from_rgba_unmultiplied_const(0x3a, 0x3a, 0x3a, 102),
        hero_scrim_top: Color32::from_rgba_unmultiplied_const(0x00, 0x00, 0x00, 82),
        hero_scrim_mid: Color32::from_rgba_unmultiplied_const(0x00, 0x00, 0x00, 148),
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
mod tests {
    use super::*;

    fn luminance(color: Color32) -> f32 {
        fn channel(value: u8) -> f32 {
            let value = value as f32 / 255.0;
            if value <= 0.039_28 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        }

        0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
    }

    fn contrast_ratio(a: Color32, b: Color32) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        let (high, low) = if a > b { (a, b) } else { (b, a) };
        (high + 0.05) / (low + 0.05)
    }

    #[test]
    fn the_spacing_scale_is_strictly_increasing() {
        let scale = [
            space::S1,
            space::S2,
            space::S3,
            space::S4,
            space::S5,
            space::S6,
            space::S7,
            space::S8,
        ];
        for pair in scale.windows(2) {
            assert!(pair[0] < pair[1], "{:?} is not increasing", pair);
        }
    }

    #[test]
    fn the_type_scale_is_strictly_increasing() {
        let scale = [
            text::XXS,
            text::XS,
            text::SM,
            text::BASE,
            text::MD,
            text::LG,
            text::XL,
            text::XXL,
            text::XXXL,
            text::XXXXL,
        ];
        for pair in scale.windows(2) {
            assert!(pair[0] < pair[1], "{:?} is not increasing", pair);
        }
    }

    #[test]
    fn the_weight_scale_is_strictly_increasing() {
        for pair in Weight::ALL.windows(2) {
            assert!(
                pair[0].value() < pair[1].value(),
                "{:?} is not lighter than {:?}",
                pair[0],
                pair[1]
            );
        }
        for pair in MonoWeight::ALL.windows(2) {
            assert!(pair[0].value() < pair[1].value(), "{:?}", pair);
        }
    }

    #[test]
    fn every_text_tier_is_readable_on_the_panel_it_sits_on() {
        let p = &Palette::DARK;
        for (name, color) in [
            ("text", p.text),
            ("text_muted", p.text_muted),
            ("text_subtle", p.text_subtle),
        ] {
            let ratio = contrast_ratio(color, p.surface);
            assert!(ratio >= 4.5, "{name} is {ratio:.2}:1 on surface, below AA");
        }
    }

    #[test]
    fn the_narrow_now_playing_is_narrower_than_the_wide_one() {
        const { assert!(extent::NOWPLAYING_NARROW_W < extent::NOWPLAYING_W) };
    }

    #[test]
    fn the_dark_palette_says_so() {
        // Both sides are constants, so this is settled at compile time rather
        // than by a test that could be skipped.
        const { assert!(Palette::DARK.dark) };
        const { assert!(Tokens::DARK.palette.dark) };
    }

    #[test]
    fn the_translucent_tokens_keep_their_alpha() {
        let p = &Palette::DARK;
        assert_eq!(p.border.to_srgba_unmultiplied()[3], 28);
        assert_eq!(p.border_strong.to_srgba_unmultiplied()[3], 66);
        assert_eq!(p.scrim.to_srgba_unmultiplied()[3], 199);
        assert_eq!(p.row_hover.to_srgba_unmultiplied()[3], 15);
        assert_eq!(p.row_selected.to_srgba_unmultiplied()[3], 41);
        assert_eq!(p.hero_wash.to_srgba_unmultiplied()[3], 102);
    }

    #[test]
    fn the_shadows_keep_their_blur_and_stay_flat() {
        let p = &Palette::DARK;
        assert_eq!((p.shadow_sm.blur, p.shadow_sm.spread), (2, 0));
        assert_eq!((p.shadow_md.blur, p.shadow_md.spread), (20, 0));
        assert_eq!((p.shadow_lg.blur, p.shadow_lg.spread), (50, 0));
        assert_eq!(p.shadow_sm.color.to_srgba_unmultiplied()[3], 128);
        assert_eq!(p.shadow_md.color.to_srgba_unmultiplied()[3], 140);
        assert_eq!(p.shadow_lg.color.to_srgba_unmultiplied()[3], 166);
    }

    #[test]
    fn the_icon_sizes_increase_and_the_stroke_never_grows_with_them() {
        let sizes = [
            icon::Size::SM,
            icon::Size::BASE,
            icon::Size::MD,
            icon::Size::LG,
        ];
        for pair in sizes.windows(2) {
            assert!(pair[0].side < pair[1].side, "{:?} is not larger", pair);
            assert!(
                pair[0].stroke >= pair[1].stroke,
                "{:?} got a heavier stroke",
                pair
            );
        }
    }

    #[test]
    fn the_icon_grid_is_the_sprites_viewbox() {
        assert_eq!(icon::GRID, 24.0);
    }

    #[test]
    fn text_disabled_is_a_band_below_text_subtle() {
        let p = &Palette::DARK;
        for (name, behind) in [
            ("surface", p.surface),
            ("panel", p.panel),
            ("window", p.window),
        ] {
            let ratio = contrast_ratio(p.text_disabled, behind);
            assert!(
                (2.0..=3.0).contains(&ratio),
                "text_disabled is {ratio:.2}:1 on {name}"
            );
            assert!(
                ratio < contrast_ratio(p.text_subtle, behind),
                "text_disabled is not below text_subtle on {name}"
            );
        }
    }

    #[test]
    fn slight_tracking_sits_between_none_and_wide() {
        const { assert!(0.0 < tracking::SLIGHT) };
        const { assert!(tracking::SLIGHT < tracking::WIDE) };
    }

    #[test]
    fn pressed_is_bone_at_a_twentieth_of_an_alpha() {
        let [r, g, b, a] = Palette::DARK.pressed.to_srgba_unmultiplied();
        assert_eq!(a, 20);
        // Unmultiplying a low alpha is lossy by up to about 6 per channel.
        for (got, want) in [(r, 0xf2), (g, 0xf0), (b, 0xea)] {
            assert!(
                got.abs_diff(want) <= 6,
                "pressed is {r},{g},{b},{a}, not bone"
            );
        }
    }

    #[test]
    fn the_row_hover_is_bone_but_lighter_than_pressed() {
        let p = &Palette::DARK;
        let [r, g, b, a] = p.row_hover.to_srgba_unmultiplied();
        assert_eq!(a, 15, "the reference draws the row hover at 0.06");
        for (got, want) in [(r, 0xf2), (g, 0xf0), (b, 0xea)] {
            assert!(got.abs_diff(want) <= 6, "row_hover is {r},{g},{b},{a}");
        }
        assert!(
            a < p.pressed.to_srgba_unmultiplied()[3],
            "a hover is lighter than a selection"
        );
    }

    /// Composites a translucent fill over an opaque one, in the same channel
    /// order the painter blends, so a contrast check sees the row as drawn.
    fn over(fill: Color32, behind: Color32) -> Color32 {
        let [r, g, b, a] = fill.to_srgba_unmultiplied();
        let [br, bg, bb, _] = behind.to_srgba_unmultiplied();
        let mix = |front: u8, back: u8| {
            let t = a as f32 / 255.0;
            (front as f32 * t + back as f32 * (1.0 - t)).round() as u8
        };
        Color32::from_rgb(mix(r, br), mix(g, bg), mix(b, bb))
    }

    #[test]
    fn the_selected_row_is_bone_at_sixteen_percent() {
        let p = &Palette::DARK;
        let [r, g, b, a] = p.row_selected.to_srgba_unmultiplied();
        assert_eq!(a, 41);
        for (got, want) in [(r, 0xf2), (g, 0xf0), (b, 0xea)] {
            assert!(
                got.abs_diff(want) <= 6,
                "row_selected is {r},{g},{b},{a}, not bone"
            );
        }
        assert!(
            a > 2 * p.row_hover.to_srgba_unmultiplied()[3],
            "a selection has to read well above a hover"
        );
    }

    #[test]
    fn the_marker_is_opaque_bone_and_not_the_accent() {
        let p = &Palette::DARK;
        let [r, g, b, a] = p.mark.to_srgba_unmultiplied();
        assert_eq!((r, g, b, a), (0xf2, 0xf0, 0xea, 255));
        assert_ne!(p.mark, p.accent);
    }

    #[test]
    fn a_lifted_row_is_set_back_not_hidden() {
        assert!((0.5..1.0).contains(&opacity::LIFTED));
    }

    #[test]
    fn muted_ink_and_the_accent_bar_stay_readable_inside_a_selected_row() {
        let p = &Palette::DARK;
        for behind in [p.surface, p.panel, p.surface_raised] {
            let row = over(p.row_selected, behind);
            let muted = contrast_ratio(p.text_muted, row);
            let accent = contrast_ratio(p.accent, row);
            assert!(muted >= 4.5, "muted ink on a selected row is {muted:.2}:1");
            assert!(accent >= 4.0, "the accent bar on a row is {accent:.2}:1");
        }
    }

    #[test]
    fn the_drag_edge_is_sixty_four_points() {
        assert_eq!(scroll::DRAG_EDGE, space::S8);
    }

    #[test]
    fn the_hero_wash_is_the_reference_tint_at_its_own_opacity() {
        let [r, g, b, a] = Palette::DARK.hero_wash.to_srgba_unmultiplied();
        // The reference's `--hero-tint` is #3a3a3a under a 0.4 opacity.
        for (got, want) in [(r, 0x3a), (g, 0x3a), (b, 0x3a)] {
            assert!(got.abs_diff(want) <= 6, "hero_wash is {r},{g},{b},{a}");
        }
    }

    #[test]
    fn the_artist_scrim_darkens_from_top_to_middle() {
        let p = &Palette::DARK;
        // The reference's gradient: black at 0.32, then black at 0.58.
        for (name, color) in [
            ("hero_scrim_top", p.hero_scrim_top),
            ("hero_scrim_mid", p.hero_scrim_mid),
        ] {
            let [r, g, b, _] = color.to_srgba_unmultiplied();
            assert_eq!((r, g, b), (0, 0, 0), "{name} is not black: {r},{g},{b}");
        }
        assert_eq!(p.hero_scrim_top.to_srgba_unmultiplied()[3], 82);
        assert_eq!(p.hero_scrim_mid.to_srgba_unmultiplied()[3], 148);
        assert!(
            p.hero_scrim_top.to_srgba_unmultiplied()[3]
                < p.hero_scrim_mid.to_srgba_unmultiplied()[3],
            "the scrim darkens toward the middle"
        );
        // The hero's title is drawn over the middle of the scrim, so the text
        // tier must stay readable there.
        let ratio = contrast_ratio(p.text, p.hero_scrim_mid);
        assert!(ratio >= 4.5, "text over the artist scrim is {ratio:.2}:1");
    }

    #[test]
    fn the_rule_is_heavier_than_the_hairline() {
        const { assert!(stroke::HAIRLINE < stroke::RULE) };
    }

    #[test]
    fn the_title_column_grows_more_than_the_album_column() {
        const { assert!(grow::ALBUM > 0.0) };
        const { assert!(grow::ALBUM < grow::TITLE) };
    }

    #[test]
    fn the_hero_title_clamp_has_a_floor_below_its_cap() {
        const { assert!(hero::TITLE_MIN < text::XXXXL) };
        const { assert!(hero::TITLE_VW > 0.0) };
    }

    #[test]
    fn the_hero_play_tile_is_larger_than_a_card_tile() {
        const { assert!(extent::CARD_PLAY < hero::PLAY) };
    }

    #[test]
    fn a_cover_is_filtered_and_never_tiled() {
        use egui::{TextureFilter, TextureWrapMode};

        assert!(matches!(
            art::TEXTURE.wrap_mode,
            TextureWrapMode::ClampToEdge
        ));
        assert!(matches!(art::TEXTURE.magnification, TextureFilter::Linear));
        assert!(matches!(art::TEXTURE.minification, TextureFilter::Linear));
        assert!(art::TEXTURE.mipmap_mode.is_none());
    }

    #[test]
    fn the_equaliser_bars_shrink_and_grow_without_vanishing() {
        const { assert!(eq::MIN_SCALE > 0.0) };
        const { assert!(eq::MIN_SCALE < 1.0) };
        // A bar is a bar, not a square.
        const { assert!(eq::BAR_W < eq::H) };
        // The last bar starts inside the first bar's own pulse, so the group is
        // one movement and not a wave chasing its own tail.
        const { assert!((eq::BARS as f32 - 1.0) * eq::DELAY < eq::PULSE) };
        // A frame is a fraction of a pulse, or the picture would skip.
        const { assert!(eq::FRAME < eq::PULSE / 8.0) };
    }

    #[test]
    fn the_menu_width_tokens_are_twelve_and_twenty_rem() {
        // The sheet's `.menu:has(> .menu__action)` floor and cap, in points.
        const REM: f32 = 16.0;
        const {
            assert!(extent::MENU_MIN_W == 12.0 * REM);
            assert!(extent::MENU_MAX_W == 20.0 * REM);
        };
    }

    #[test]
    fn the_track_columns_are_the_reference_rem_values() {
        // 16 points to the rem, which is the base every value in this file uses.
        const REM: f32 = 16.0;
        const {
            assert!(extent::TRACK_INDEX_W == 2.25 * REM);
            assert!(extent::TRACK_ADDED_W == 5.5 * REM);
            assert!(extent::TRACK_LIKE_W == 1.75 * REM);
            assert!(extent::TRACK_DURATION_W == 3.5 * REM);
            assert!(extent::TRACK_ACTIONS_W == 1.75 * REM);
            assert!(extent::LIBRARY_ART == 2.25 * REM);
            assert!(extent::QUEUE_ART == 2.5 * REM);
            assert!(extent::EPISODE_TOGGLE == 2.5 * REM);
            assert!(extent::EPISODE_ROW_H > extent::EPISODE_TOGGLE);
            assert!(extent::NOWPLAYING_ART_MIN == 4.0 * REM);
            assert!(extent::NOWPLAYING_ART_MIN < extent::NOWPLAYING_ART_MAX);
            assert!(extent::CARD_MIN_W == 9.0 * REM);
            assert!(extent::RELEASE_ART == 7.0 * REM);
            assert!(extent::RELEASE_PLAY == 2.5 * REM);
            assert!(hero::ARTIST_MIN_H == 17.0 * REM);
            assert!(hero::ARTIST_TITLE_MIN == 2.25 * REM);
            assert!(hero::ARTIST_TITLE_MAX == 4.5 * REM);
            assert!(hero::ARTIST_TITLE_VW > hero::TITLE_VW);
            assert!(hero::SCRIM_MID_STOP > 0.0 && hero::SCRIM_MID_STOP < 1.0);
            assert!(extent::CARD_PLAY == 2.0 * REM);
            assert!(hero::MIN_H == 11.0 * REM);
            assert!(hero::ART == 11.0 * REM);
            assert!(hero::PLAY == 2.75 * REM);
            assert!(hero::TITLE_MIN == 2.0 * REM);
            // The button's own padding is a bare 0.35 rem, not a step on the scale.
            assert!(extent::BUTTON_PAD_Y == 0.35 * REM);
        };
    }
}
