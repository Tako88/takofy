//! The one place tokens become an egui `Style`.
//!
//! Splitting this out of the token layer keeps two reasons to change apart: the
//! values, which are a transcription of the design, and this mapping, which is a
//! fact about egui's field names.

use egui::{FontId, Margin, Stroke, TextStyle, Vec2, Visuals};

use super::fonts;
use super::{Palette, Tokens, Weight, motion, radius, space, stroke, text};

pub fn style(tokens: &Tokens) -> egui::Style {
    let p = &tokens.palette;

    let mut style = egui::Style {
        visuals: visuals(p),
        ..Default::default()
    };

    style.animation_time = motion::BASE;

    style.spacing.item_spacing = Vec2::new(space::S2, space::S2);
    style.spacing.button_padding = Vec2::new(space::S3, space::S2);
    style.spacing.menu_margin = Margin::same(space::S2 as i8);

    style.text_styles = [
        (
            TextStyle::Body,
            FontId::new(text::BASE, fonts::sans(Weight::Regular)),
        ),
        (
            TextStyle::Button,
            FontId::new(text::BASE, fonts::sans(Weight::Medium)),
        ),
        (
            TextStyle::Small,
            FontId::new(text::SM, fonts::sans(Weight::Regular)),
        ),
        (
            TextStyle::Heading,
            FontId::new(text::XL, fonts::sans(Weight::Bold)),
        ),
        (
            TextStyle::Monospace,
            FontId::new(text::SM, fonts::mono(super::MonoWeight::Regular)),
        ),
    ]
    .into();

    style
}

fn visuals(p: &Palette) -> Visuals {
    let mut v = if p.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };

    v.panel_fill = p.surface;
    v.window_fill = p.surface_raised;
    v.faint_bg_color = p.surface_raised;
    v.extreme_bg_color = p.surface_sunken;
    v.window_stroke = Stroke::new(stroke::HAIRLINE, p.border);
    v.window_corner_radius = radius::LG;
    v.menu_corner_radius = radius::LG;
    v.popup_shadow = p.shadow_md;
    v.window_shadow = p.shadow_lg;
    v.weak_text_color = Some(p.text_muted);
    v.hyperlink_color = p.verified;

    for widget in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        widget.corner_radius = radius::MD;
        widget.fg_stroke = Stroke::new(stroke::HAIRLINE, p.text);
    }

    v.widgets.noninteractive.bg_stroke = Stroke::new(stroke::HAIRLINE, p.border);

    v.widgets.inactive.bg_fill = p.surface_raised;
    v.widgets.inactive.bg_stroke = Stroke::new(stroke::HAIRLINE, p.border);
    v.widgets.hovered.bg_fill = p.surface_hover;
    v.widgets.hovered.bg_stroke = Stroke::new(stroke::HAIRLINE, p.border_strong);
    v.widgets.active.bg_fill = p.surface_hover;
    v.widgets.active.bg_stroke = Stroke::new(stroke::HAIRLINE, p.border_strong);
    v.widgets.open.bg_fill = p.surface_hover;
    v.widgets.open.bg_stroke = Stroke::new(stroke::HAIRLINE, p.border_strong);

    // Text selection is the only thing egui paints from this field, and the
    // reference gives it the accent: `::selection { background: accent; color:
    // text-inverse }`. Green on a row would say sound is coming out, so rows are
    // never selected through egui — their fill is painted from the tokens.
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(stroke::HAIRLINE, p.text_inverse);

    v
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

    #[test]
    fn every_text_style_names_a_family_that_exists() {
        let definitions = fonts::definitions();
        for (variant, font) in style(&Tokens::DARK).text_styles {
            assert!(
                definitions.families.contains_key(&font.family),
                "{variant:?} asks for {:?}, which no font is registered under",
                font.family
            );
        }
    }

    #[test]
    fn the_dark_palette_paints_from_a_dark_base() {
        let style = style(&Tokens::DARK);
        assert!(style.visuals.dark_mode);
        assert_eq!(style.animation_time, motion::BASE);
    }

    /// A selection has to be seen: the reference draws it as the accent under
    /// inverse text, and the text field's own fill is a grey one step from the
    /// row hover that used to stand here.
    #[test]
    fn text_selection_is_the_accent_under_inverse_text() {
        let tokens = Tokens::DARK;
        let style = style(&tokens);
        assert_eq!(style.visuals.selection.bg_fill, tokens.palette.accent);
        assert_eq!(
            style.visuals.selection.stroke.color,
            tokens.palette.text_inverse
        );
        assert_ne!(
            style.visuals.selection.bg_fill, tokens.palette.surface_hover,
            "a selection that matches the field's own hover cannot be seen"
        );
    }

    #[test]
    fn text_colour_is_not_flattened() {
        let style = style(&Tokens::DARK);
        assert!(
            style.visuals.override_text_color.is_none(),
            "a single override colour would flatten the muted tiers"
        );
    }

    #[test]
    fn plain_text_takes_the_text_token() {
        // Body colour is read from `noninteractive.fg_stroke`, so leaving that
        // state out of the loop above silently drops every label to egui's own
        // grey.
        let style = style(&Tokens::DARK);
        assert_eq!(style.visuals.text_color(), Palette::DARK.text);
        assert_eq!(style.visuals.weak_text_color(), Palette::DARK.text_muted);
    }

    #[test]
    fn every_widget_state_is_squared_off_to_the_radius_scale() {
        let style = style(&Tokens::DARK);
        let widgets = &style.visuals.widgets;
        for (name, corner) in [
            ("noninteractive", widgets.noninteractive.corner_radius),
            ("inactive", widgets.inactive.corner_radius),
            ("hovered", widgets.hovered.corner_radius),
            ("active", widgets.active.corner_radius),
            ("open", widgets.open.corner_radius),
        ] {
            assert_eq!(corner, radius::MD, "{name}");
        }
    }
}
