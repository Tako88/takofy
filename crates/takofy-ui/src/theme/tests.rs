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
fn the_stuck_actionbar_is_the_references_actionbar_h() {
    // `--actionbar-h` is 3.75 rem at the 16-point base: the number the
    // reference says anything sticky under the row clears.
    assert_eq!(actionbar::STUCK_H, 60.0);
    // At rest the row is the play tile inside `space-4` on both sides.
    assert_eq!(actionbar::REST_H, 76.0);
    assert_eq!(actionbar::REST_H, hero::PLAY + 2.0 * space::S4);
    assert_eq!(actionbar::STUCK_H, hero::PLAY + 2.0 * space::S2);
    // The field is the reference's 11 rem, and its floor is a third of the
    // row it sits in: the sweep test is what makes the floor safe.
    assert_eq!(actionbar::FIELD_W, 176.0);
    assert_eq!(actionbar::FIELD_MIN_W, 120.0);
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
fn the_hero_wash_is_the_reference_fallback() {
    assert_eq!(
        Palette::DARK.hero_wash.to_srgba_unmultiplied(),
        [0x2a, 0x2a, 0x31, 255],
        "the fallback is the reference's own opaque tint"
    );
}

#[test]
fn the_tint_parameters_are_ordered() {
    let p = TintParams::DARK;
    assert!(p.tone_min < p.tone_max, "a tone range needs two ends");
    assert!(p.saturation_min < p.saturation_max);
    assert!(
        p.lightness_floor < p.tone_min,
        "the floor is below every tone the artwork can ask for"
    );
    assert!(p.lightness_step > 0.0, "a step has to move");
}

#[test]
fn the_rule_is_heavier_than_the_hairline() {
    const { assert!(stroke::HAIRLINE < stroke::RULE) };
}

#[test]
fn the_menu_heading_is_padding_about_one_line_of_caps() {
    assert_eq!(extent::MENU_HEAD_H, 27.0);
}

#[test]
fn the_title_column_grows_more_than_the_album_column() {
    const { assert!(grow::ALBUM > 0.0) };
    const { assert!(grow::ALBUM < grow::TITLE) };
}

#[test]
fn the_plays_credit_is_tighter_than_the_playlists() {
    const { assert!(grow::CREDIT_PLAYS > 0.0) };
    const { assert!(grow::CREDIT_PLAYS < grow::ARTIST) };
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
    // 16 points to the rem, which is the base the token layer uses.
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
        assert!(hero::ARTIST_PLATE == 16.0 * REM);
        assert!(hero::ARTIST_PLATE_NARROW == 13.0 * REM);
        assert!(hero::ARTIST_PLATE_PHONE == 6.5 * REM);
        assert!(hero::ARTIST_TITLE_MIN == 1.75 * REM);
        assert!(hero::ARTIST_TITLE_MAX == 3.25 * REM);
        assert!(hero::ARTIST_TITLE_PHONE_MIN == 1.75 * REM);
        assert!(hero::ARTIST_TITLE_PHONE_MAX == 2.75 * REM);
        assert!(hero::ARTIST_TITLE_VW < hero::ARTIST_TITLE_PHONE_VW);
        assert!(extent::CARD_PLAY == 2.0 * REM);
        assert!(hero::MIN_H == 11.0 * REM);
        assert!(hero::ART == 11.0 * REM);
        assert!(hero::PLAY == 2.75 * REM);
        assert!(hero::TITLE_MIN == 2.0 * REM);
        // The button's own padding is a bare 0.35 rem, not a step on the scale.
        assert!(extent::BUTTON_PAD_Y == 0.35 * REM);
    };
}
