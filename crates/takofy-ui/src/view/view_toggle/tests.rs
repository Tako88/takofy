use super::*;
use crate::theme::{fonts, style};

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx
}

#[test]
fn the_group_is_flush_with_the_rows_trailing_edge() {
    let row = Rect::from_min_size(Pos2::new(24.0, 100.0), Vec2::new(600.0, 28.0));
    let rect = rect(row);
    assert!((rect.right() - row.right()).abs() < 0.01, "flush right");
    assert!((rect.center().y - row.center().y).abs() < 0.01, "centred");
    assert!((rect.width() - (2.0 * extent::BUTTON + stroke::HAIRLINE)).abs() < 0.01);
    assert!((rect.height() - extent::BUTTON).abs() < 0.01);
}

/// Pressing the other density asks for it; pressing the chosen one asks for
/// nothing, because a density is not a toggle.
#[test]
fn the_unpressed_cell_asks_for_its_density() {
    let tokens = Tokens::DARK;
    let ctx = context();
    let row = Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 28.0));
    let cells = rect(row);

    for (chosen, press_x, expected) in [
        (
            MediaLayout::Grid,
            cells.right() - extent::BUTTON / 2.0,
            MediaLayout::List,
        ),
        (
            MediaLayout::List,
            cells.left() + extent::BUTTON / 2.0,
            MediaLayout::Grid,
        ),
    ] {
        let mut picked = None;
        let mut first = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(700.0, 100.0))),
                ..Default::default()
            },
            |ui| {
                picked = show(ui, &tokens, row, chosen);
            },
        );
        first.textures_delta.clear();
        assert_eq!(picked, None, "no pointer, no press");

        // A press is two passes: the pointer arrives, then the click lands.
        let mut command = None;
        for event in crate::view::test_support::click_events(Pos2::new(press_x, 14.0)) {
            let mut pass = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(700.0, 100.0))),
                    events: vec![event],
                    ..Default::default()
                },
                |ui| {
                    if let Some(next) = show(ui, &tokens, row, chosen) {
                        command = Some(next);
                    }
                },
            );
            pass.textures_delta.clear();
        }
        assert_eq!(command, Some(expected), "chosen {chosen:?}");

        // Pressing the density already chosen asks for nothing: the group is
        // a choice, not a toggle.
        let same_x = match chosen {
            MediaLayout::Grid => cells.left() + extent::BUTTON / 2.0,
            MediaLayout::List => cells.right() - extent::BUTTON / 2.0,
        };
        let mut again = None;
        for event in crate::view::test_support::click_events(Pos2::new(same_x, 14.0)) {
            let mut pass = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(700.0, 100.0))),
                    events: vec![event],
                    ..Default::default()
                },
                |ui| {
                    if let Some(next) = show(ui, &tokens, row, chosen) {
                        again = Some(next);
                    }
                },
            );
            pass.textures_delta.clear();
        }
        assert_eq!(again, None, "the chosen cell asks for nothing");
    }
}
