use std::sync::Arc;

use egui::{Context, Galley, Pos2, RawInput, Rect, vec2};

use super::*;

fn context() -> Context {
    let tokens = Tokens::DARK;
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(crate::theme::style(&tokens));
    ctx
}

/// The galley `clamped` lays out for `label` in a column of `width`.
fn galley(ctx: &Context, label: &str, width: f32, max_rows: usize) -> Arc<Galley> {
    let tokens = Tokens::DARK;
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 400.0))),
        ..Default::default()
    };
    let mut found = None;
    let mut output = ctx.run_ui(input, |ui| {
        found = Some(clamped(
            ui,
            label,
            hero_description(),
            tokens.palette.text_muted,
            tracking_pts(text::XS, tracking::SLIGHT),
            width,
            max_rows,
        ));
    });
    output.textures_delta.clear();
    found.expect("the layout runs")
}

/// A description long enough that two rows cannot hold it.
const LONG: &str = "Windows down, city behind you. Update every Sunday, and \
    again whenever the mood takes me, with no plan and no promise, only the \
    long way home through the sodium light of the ring road.";

/// The same kind of sentence, short enough that two rows hold all of it.
const TWO_ROWS: &str = "Windows down, city behind you, and the long way home.";

#[test]
fn clamped_stops_at_its_row_count_and_elides_the_rest() {
    let ctx = context();
    let width = 260.0;
    let galley = galley(&ctx, LONG, width, 2);
    assert_eq!(galley.rows.len(), 2, "two rows, no more");
    assert!(galley.elided, "a long description elides");
    let last = galley.rows[1].row.text();
    assert!(
        last.ends_with('…'),
        "the elided last row must end in an ellipsis: {last:?}"
    );
    assert!(
        galley.size().x <= width + 0.01,
        "the block is {} wide in a {width} column",
        galley.size().x
    );
}

#[test]
fn clamped_leaves_a_single_row_unelided() {
    let ctx = context();
    let galley = galley(&ctx, "Windows down.", 260.0, 2);
    assert_eq!(galley.rows.len(), 1, "a short description is one row");
    assert!(!galley.elided, "a short description does not elide");
}

#[test]
fn clamped_leaves_a_two_row_description_unelided() {
    let ctx = context();
    let galley = galley(&ctx, TWO_ROWS, 260.0, 2);
    assert_eq!(galley.rows.len(), 2, "the sentence needs both rows");
    assert!(!galley.elided, "a description that fits is not elided");
}

#[test]
fn tracking_scales_with_the_size() {
    assert!((tracking_pts(text::XXS, tracking::CAPS) - 1.1).abs() < 1e-6);
    assert_eq!(
        tracking_pts(20.0, tracking::WIDE),
        2.0 * tracking_pts(10.0, tracking::WIDE)
    );
}

#[test]
fn caps_upper_cases_including_non_ascii() {
    let color = Tokens::DARK.palette.text;
    assert_eq!(caps("straße", tracking::WIDE, color).text(), "STRASSE");
    assert_eq!(caps("sō", tracking::WIDE, color).text(), "SŌ");
}
