use super::super::*;
use super::test_support::*;

use takofy_model::CoverUrl;

/// The desktop entry is another file that names the app id, and a mismatch
/// is invisible until a compositor shows a second icon in the dock.
#[test]
fn the_desktop_entry_names_the_id_the_window_sets() {
    let entry = include_str!("../../../../../packaging/io.github.Tako88.Takofy.desktop");
    let field = |name: &str| {
        entry
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .map(str::trim)
    };
    assert_eq!(field("Icon="), Some(APP_ID));
    assert_eq!(field("StartupWMClass="), Some(APP_ID));
    assert_eq!(field("Exec="), Some("takofy"), "the installed binary");
}

/// The icon is compiled in, so a corrupt one would be a silent blank in the
/// task bar rather than a build failure.
#[test]
fn the_window_icon_decodes_at_the_size_the_packages_install() {
    let icon = window_icon().expect("the bundled icon decodes");
    assert_eq!((icon.width, icon.height), (512, 512));
    assert_eq!(icon.rgba.len(), 512 * 512 * 4);
}

#[test]
fn a_new_window_starts_with_the_width_rule_alone() {
    let (app, _) = test_window();
    assert_eq!(app.pane, PanePref::Auto, "a launch hid the pane");
    for width in [layout::WINDOW_DEFAULT[0], 1440.0] {
        assert_eq!(
            layout::regions_with(width, app.pane),
            layout::regions_for(width),
            "{width}"
        );
    }
}

#[test]
fn the_clear_colour_is_the_window_token() {
    let tokens = Tokens::DARK;
    assert_eq!(
        window_clear_color(&tokens),
        tokens.palette.window.to_normalized_gamma_f32()
    );
}

#[test]
fn run_and_return_is_pinned() {
    assert!(native_options().run_and_return);
}

#[test]
fn the_system_font_fallback_stays_off() {
    assert!(
        !native_options().system_font_fallback,
        "a fallback would hide a missing glyph here and show one elsewhere"
    );
}

#[test]
fn the_shipping_chain_keeps_egui_icons() {
    // This binary links eframe, which is what turns on egui's `default_fonts`;
    // a crate depending on egui alone has no builtin fonts at all. So this is
    // the only place the configuration that actually ships can be asserted.
    let defs = takofy_ui::theme::fonts::definitions();
    for (family, chain) in &defs.families {
        assert!(
            chain.iter().any(|name| name == "egui-icons"),
            "{family:?} would leave egui's own icons undrawn"
        );
    }
}

#[test]
fn the_viewport_uses_the_layout_sizes() {
    let options = native_options();
    assert_eq!(
        options.viewport.inner_size,
        Some(egui::vec2(
            layout::WINDOW_DEFAULT[0],
            layout::WINDOW_DEFAULT[1]
        ))
    );
    assert_eq!(
        options.viewport.min_inner_size,
        Some(egui::vec2(layout::WINDOW_MIN[0], layout::WINDOW_MIN[1]))
    );
}

#[test]
fn a_failed_cover_asks_for_a_wake_at_its_retry() {
    let mut art = Art::default();
    let now = Instant::now();
    let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url");
    art.begin_frame();
    art.look(Some(&set(&url)), 64, now);
    let _ = art.requests(now);
    art.apply(&Update::CoverFailed { url: url.clone() }, now);
    art.begin_frame();
    assert_eq!(
        art.look(Some(&set(&url)), 64, now),
        takofy_ui::cover::Cover::Failed
    );

    let ctx = egui::Context::default();
    // Warm up so egui's own first-frame work (the font atlas and its
    // immediate repaint) is not mistaken for a cover wake.
    for _ in 0..2 {
        let mut warm = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        warm.textures_delta.clear();
    }
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        schedule_wake(ui.ctx(), &art, now);
    });
    output.textures_delta.clear();
    let delay = output
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .expect("the root viewport")
        .repaint_delay;
    assert!(
        delay <= Duration::from_secs(5),
        "a retry in five seconds woke the window in {delay:?}"
    );

    // Nothing pending leaves egui's own default alone. A fresh context is
    // used because the one above is holding the repaint it was asked for.
    let idle = Art::default();
    let ctx = egui::Context::default();
    for _ in 0..2 {
        let mut warm = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        warm.textures_delta.clear();
    }
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        schedule_wake(ui.ctx(), &idle, now);
    });
    output.textures_delta.clear();
    assert_eq!(
        output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .expect("the root viewport")
            .repaint_delay,
        Duration::MAX,
        "an idle window must not be woken"
    );
}
