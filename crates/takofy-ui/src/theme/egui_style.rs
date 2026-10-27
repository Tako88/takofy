//! The one place tokens become an egui `Style`.
//!
//! Splitting this out of the token layer keeps two reasons to change apart: the
//! values, which are a transcription of the design, and this mapping, which is a
//! fact about egui's field names.

use egui::style::ScrollStyle;
use egui::{FontId, Margin, Stroke, TextStyle, Vec2, Visuals};

use super::fonts;
use super::{Palette, Tokens, Weight, extent, motion, opacity, radius, space, stroke, text};

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
    style.spacing.scroll = ScrollStyle {
        // The reference's thumb: bone on a transparent track, at rest and under
        // the pointer, so hovering brightens one thing instead of revealing it.
        foreground_color: true,
        floating_width: extent::SCROLLBAR,
        bar_width: extent::SCROLLBAR,
        // The bar floats over the list's gutter. Reserving its width would make
        // every row four points narrower the moment a list starts to overflow.
        floating_allocated_width: 0.0,
        dormant_background_opacity: 0.0,
        active_background_opacity: 0.0,
        interact_background_opacity: 0.0,
        dormant_handle_opacity: opacity::SCROLL_REST,
        // Hovering anywhere over the list is not a reason to brighten the bar;
        // only the pointer on the bar itself is, as in the reference.
        active_handle_opacity: opacity::SCROLL_REST,
        interact_handle_opacity: opacity::SCROLL_HOVER,
        ..ScrollStyle::floating()
    };

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
mod tests;
