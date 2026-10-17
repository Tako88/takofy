//! The token layer.
//!
//! `clippy.toml` denies the `egui::Color32` constructors crate-wide, and this is
//! the one file where that allow is lifted — a literal colour anywhere else is a
//! build failure rather than a review comment.

#![allow(clippy::disallowed_methods, reason = "this file is the token layer")]

use egui::{Color32, FontFamily, FontId, TextStyle, Visuals};

/// A 4-point grid, so vertical rhythm stays consistent.
pub mod space {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 20.0;
    pub const XL: f32 = 32.0;
}

pub mod radius {
    use egui::CornerRadius;

    pub const SM: CornerRadius = CornerRadius::same(4);
    pub const MD: CornerRadius = CornerRadius::same(8);
    pub const LG: CornerRadius = CornerRadius::same(12);
}

/// Only the size varies; the families are fixed in [`style`].
pub mod text {
    pub const BODY: f32 = 14.0;
    pub const LABEL: f32 = 12.0;
    pub const TITLE: f32 = 20.0;
}

/// The only `Color32` values in the codebase.
pub struct Palette {
    /// Behind everything.
    pub bg: Color32,
    /// Panels sitting on `bg`.
    pub surface: Color32,
    /// Popups and menus, above `surface`.
    pub surface_raised: Color32,
    /// Primary text.
    pub text: Color32,
    /// Secondary text: artist, album, duration, placeholder.
    pub text_muted: Color32,
    /// Playhead, selection, active row. Used sparingly.
    pub accent: Color32,
    /// Hairlines and dividers.
    pub border: Color32,
}

impl Palette {
    pub const DARK: Self = Self {
        bg: Color32::from_rgb(0x0b, 0x0b, 0x0d),
        surface: Color32::from_rgb(0x15, 0x15, 0x18),
        surface_raised: Color32::from_rgb(0x1f, 0x1f, 0x24),
        text: Color32::from_rgb(0xf2, 0xf2, 0xf4),
        text_muted: Color32::from_rgb(0x8b, 0x8b, 0x94),
        accent: Color32::from_rgb(0x1d, 0xd7, 0x60),
        border: Color32::from_rgb(0x2a, 0x2a, 0x30),
    };
}

/// The palette plus the geometry that is not a spacing step.
pub struct Tokens {
    pub palette: Palette,
    /// Fixed row height, so lists scan as a grid.
    pub row_height: f32,
    /// Cover art edge length in the now-playing bar.
    pub art_small: f32,
    /// Cover art edge length in a grid cell.
    pub art_large: f32,
}

impl Tokens {
    pub const DARK: Self = Self {
        palette: Palette::DARK,
        row_height: 48.0,
        art_small: 40.0,
        art_large: 160.0,
    };
}

/// Every field egui lets us set comes from a token. Anything left at egui's
/// default is a value the UI cannot reach, so it needs no token.
pub fn style(tokens: &Tokens) -> egui::Style {
    let p = &tokens.palette;

    let mut style = egui::Style {
        visuals: Visuals {
            dark_mode: true,
            override_text_color: Some(p.text),
            panel_fill: p.bg,
            window_fill: p.surface,
            faint_bg_color: p.surface,
            extreme_bg_color: p.surface_raised,
            window_stroke: egui::Stroke::new(1.0, p.border),
            ..Visuals::dark()
        },
        ..Default::default()
    };

    style.spacing.item_spacing = egui::vec2(space::SM, space::SM);
    style.spacing.button_padding = egui::vec2(space::MD, space::SM);
    style.spacing.menu_margin = egui::Margin::same(space::SM as i8);

    style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, p.border);
    style.visuals.widgets.inactive.bg_fill = p.surface_raised;
    style.visuals.widgets.hovered.bg_fill = p.border;
    style.visuals.widgets.active.bg_fill = p.accent;
    style.visuals.selection.bg_fill = p.accent.gamma_multiply(0.35);
    style.visuals.selection.stroke = egui::Stroke::new(1.0, p.accent);

    style.text_styles = [
        (
            TextStyle::Body,
            FontId::new(text::BODY, FontFamily::Proportional),
        ),
        (
            TextStyle::Button,
            FontId::new(text::BODY, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(text::LABEL, FontFamily::Proportional),
        ),
        (
            TextStyle::Heading,
            FontId::new(text::TITLE, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(text::LABEL, FontFamily::Monospace),
        ),
    ]
    .into();

    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_styles_cover_every_variant() {
        let style = style(&Tokens::DARK);
        for variant in [
            TextStyle::Small,
            TextStyle::Body,
            TextStyle::Monospace,
            TextStyle::Button,
            TextStyle::Heading,
        ] {
            assert!(
                style.text_styles.contains_key(&variant),
                "{variant:?} has no token, so egui would fall back to a default size"
            );
        }
    }
}
