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
    pub const SIDEBAR_RAIL_W: f32 = 64.0;
    pub const ROW_H: f32 = 44.0;
    pub const BUTTON: f32 = 28.0;
    pub const BUTTON_LG: f32 = 34.0;
    pub const SLIDER_H: f32 = 14.0;
    pub const SLIDER_TRACK_H: f32 = 3.0;
    pub const SLIDER_THUMB_W: f32 = 3.0;
    pub const CHIP_PAD_X: f32 = 7.2;
    pub const CHIP_PAD_Y: f32 = 3.2;
    pub const FOCUS_OFFSET: f32 = 2.0;

    /// The volume slider's width, 4.5 rem.
    pub const VOLUME_W: f32 = 72.0;
    /// The playbar's cover tile, 2.75 rem.
    pub const PLAYBAR_ART: f32 = 44.0;
    /// The account button's avatar tile, 1.25 rem.
    pub const AVATAR: f32 = 20.0;
    /// A library row's cover tile, 2.25 rem. A track row's art is the same
    /// 2.25 rem box, so it is this token and not a second one of its own.
    pub const LIBRARY_ART: f32 = 36.0;
    /// The search field's height, 2 rem.
    pub const SEARCH_H: f32 = 32.0;
    /// The search field's flex basis, 14 rem.
    pub const SEARCH_BASIS_W: f32 = 224.0;
    /// The search field's maximum width, 24 rem.
    pub const SEARCH_MAX_W: f32 = 384.0;
    /// The search field's floor. Not a reference value: a round 8 rem chosen
    /// on the reference's own scale, to be tuned at the minimum window.
    pub const SEARCH_MIN_W: f32 = 128.0;
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

    /// A card's minimum width, 9 rem, which is what the grid's auto-fill counts.
    pub const CARD_MIN_W: f32 = 144.0;
    /// A card's play tile, 2 rem.
    pub const CARD_PLAY: f32 = 32.0;
    /// The sign-in card's width. Not a reference value: the reference draws no
    /// sign-in card. 24 rem is the measure it gives a narrow field, and it
    /// leaves a margin to spare at the minimum window.
    pub const SIGNIN_W: f32 = 384.0;
}

/// How the free space in a row of columns is shared, in the reference's
/// fractional units.
///
/// A row's fixed columns take their width and these share what is left, so their
/// ratio decides how a track list reads: the title is what a listener scans.
///
/// A nested title's gutter is not a token. The reference composes it as a grid
/// track and a gap, so the sum is not a value it names; whoever lays the title
/// out writes `extent::LIBRARY_ART + space::S3` and invents nothing.
pub mod grow {
    /// The title column, `1fr`.
    pub const TITLE: f32 = 1.0;
    /// The album column, `0.65fr` in the reference's playlist template.
    pub const ALBUM: f32 = 0.65;
}

/// A header band: its height, its art, and the clamp its title is drawn at.
pub mod hero {
    /// The band's minimum height, 11 rem.
    pub const MIN_H: f32 = 176.0;
    /// The band's art tile, 11 rem. The same value as the band's own height, and
    /// a different role: the tile is square and the band may grow taller.
    pub const ART: f32 = 176.0;
    /// The play tile under the band, 2.75 rem. Larger than a card's, because it
    /// is the band's own call to action.
    pub const PLAY: f32 = 44.0;
    /// The title's floor, 2 rem, which is not a step on the type scale: the
    /// reference clamps the size rather than choosing one of the steps.
    pub const TITLE_MIN: f32 = 32.0;
    /// The title's share of the viewport width, 4.5 vw, as a fraction of it.
    pub const TITLE_VW: f32 = 0.045;
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

/// Durations in seconds, and the one easing curve the design uses.
pub mod motion {
    pub const FAST: f32 = 0.110;
    pub const BASE: f32 = 0.180;
    pub const SLOW: f32 = 0.300;
    pub const EASE_OUT: [f32; 4] = [0.3, 0.0, 0.0, 1.0];
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
    /// The fill under the pointer, and the selected row.
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
    /// Sound is coming out. Never a surface.
    pub accent: Color32,
    pub accent_hover: Color32,
    /// A selected chip's fill: bone at low alpha, not a gamma-scaled text.
    pub pressed: Color32,
    /// The fill under the pointer on a track row, bone at 6%. Not the palette's
    /// `surface_hover`, which is a lighter grey from the reference's own scale.
    pub row_hover: Color32,
    /// The wash a header band sits on. The reference writes this as `#3a3a3a`
    /// under a 0.4 opacity; a fill has one channel to carry that, so it is
    /// folded into the alpha here.
    pub hero_wash: Color32,
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
        text: Color32::from_rgb(0xf2, 0xf0, 0xea),
        text_muted: Color32::from_rgb(0xb0, 0xac, 0xa4),
        text_subtle: Color32::from_rgb(0x83, 0x80, 0x7a),
        text_disabled: Color32::from_rgb(0x4d, 0x4c, 0x48),
        text_inverse: Color32::from_rgb(0x0b, 0x0b, 0x0c),
        accent: Color32::from_rgb(0x1e, 0xd7, 0x60),
        accent_hover: Color32::from_rgb(0x3e, 0xe5, 0x7b),
        pressed: Color32::from_rgba_unmultiplied_const(0xf2, 0xf0, 0xea, 20),
        row_hover: Color32::from_rgba_unmultiplied_const(0xf2, 0xf0, 0xea, 15),
        hero_wash: Color32::from_rgba_unmultiplied_const(0x3a, 0x3a, 0x3a, 102),
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

    #[test]
    fn the_hero_wash_is_the_reference_tint_at_its_own_opacity() {
        let [r, g, b, a] = Palette::DARK.hero_wash.to_srgba_unmultiplied();
        // The reference's `--hero-tint` is #3a3a3a under a 0.4 opacity.
        for (got, want) in [(r, 0x3a), (g, 0x3a), (b, 0x3a)] {
            assert!(got.abs_diff(want) <= 6, "hero_wash is {r},{g},{b},{a}");
        }
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
            assert!(extent::CARD_MIN_W == 9.0 * REM);
            assert!(extent::CARD_PLAY == 2.0 * REM);
            assert!(hero::MIN_H == 11.0 * REM);
            assert!(hero::ART == 11.0 * REM);
            assert!(hero::PLAY == 2.75 * REM);
            assert!(hero::TITLE_MIN == 2.0 * REM);
        };
    }
}
