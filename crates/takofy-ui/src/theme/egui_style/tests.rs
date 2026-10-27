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

#[test]
fn the_scrollbar_is_a_thin_bone_thumb_that_brightens_under_the_pointer() {
    let scroll = style(&Tokens::DARK).spacing.scroll;
    assert!(
        scroll.floating,
        "a bar that allocates nothing but its own thumb"
    );
    assert!(
        scroll.foreground_color,
        "the thumb takes the text colour; a widget fill would make it a surface"
    );
    assert_eq!(scroll.floating_width, extent::SCROLLBAR);
    assert_eq!(scroll.bar_width, extent::SCROLLBAR);
    assert_eq!(
        scroll.floating_allocated_width, 0.0,
        "the bar floats, so a list does not reflow when its content overflows"
    );
    assert_eq!(scroll.dormant_background_opacity, 0.0);
    assert_eq!(scroll.dormant_handle_opacity, opacity::SCROLL_REST);
    assert_eq!(scroll.active_handle_opacity, opacity::SCROLL_REST);
    assert_eq!(scroll.interact_handle_opacity, opacity::SCROLL_HOVER);
    assert!(
        scroll.dormant_handle_opacity < scroll.interact_handle_opacity,
        "the thumb has to get louder when the pointer is on it"
    );
}

/// A selection has to be seen: the reference draws it as the accent under
/// inverse text. The field's own fill is `surface_raised`, one step below
/// `surface_hover`; the accent is what tells a selection apart from both.
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
