use egui::{Color32, Pos2, RawInput, Rect, vec2};

use super::*;
use crate::theme::extent;

fn context() -> egui::Context {
    let tokens = Tokens::DARK;
    let ctx = egui::Context::default();
    ctx.set_fonts(crate::theme::fonts::definitions());
    ctx.set_global_style(crate::theme::style(&tokens));
    ctx
}

fn input() -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 800.0))),
        ..Default::default()
    }
}

/// Runs one frame and hands back whatever the pass recorded.
fn frame<T>(ctx: &egui::Context, mut add: impl FnMut(&mut Ui) -> T) -> T {
    let mut out = None;
    let mut output = ctx.run_ui(input(), |ui| out = Some(add(ui)));
    output.textures_delta.clear();
    out.expect("a frame ran")
}

/// The fills one frame painted, with the rect each was painted into.
fn fills(ctx: &egui::Context, add: impl FnMut(&mut Ui)) -> Vec<(Rect, Color32)> {
    let mut output = ctx.run_ui(input(), add);
    output.textures_delta.clear();
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape) => Some((shape.rect, shape.fill)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_fade_snaps_to_nothing_while_carrying() {
    let ctx = context();
    let id = Id::unique("fade");
    let row = Rect::from_min_size(Pos2::ZERO, vec2(400.0, extent::ROW_H));

    // Let the reveal come up while nothing is carried.
    let mut raised = 0.0;
    for _ in 0..40 {
        raised = frame(&ctx, |ui| {
            let _ = ui.allocate_rect(row, egui::Sense::hover());
            fade(ui, id, true, Look::default())
        });
    }
    assert!(raised > 0.99, "the reveal never came up: {raised}");

    // The frame the block is lifted the reveal is gone, with no in-between.
    let snapped = frame(&ctx, |ui| {
        let _ = ui.allocate_rect(row, egui::Sense::hover());
        fade(
            ui,
            id,
            true,
            Look {
                carrying: true,
                ..Look::default()
            },
        )
    });
    assert_eq!(snapped, 0.0, "a carried row eased out instead of snapping");

    // And back again, eased rather than snapped.
    let back = frame(&ctx, |ui| {
        let _ = ui.allocate_rect(row, egui::Sense::hover());
        fade(ui, id, true, Look::default())
    });
    assert!(back < raised, "the reveal returned at once: {back}");
}

#[test]
fn a_picked_row_takes_the_selection_fill_and_not_the_hover() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let row = Rect::from_min_size(Pos2::ZERO, vec2(400.0, extent::ROW_H));

    let picked = fills(&ctx, |ui| {
        let _ = ui.allocate_rect(row, egui::Sense::hover());
        wash(
            ui,
            row,
            1.0,
            Look {
                selected: true,
                ..Look::default()
            },
            &tokens,
        );
    });
    assert_eq!(
        picked.iter().map(|(_, fill)| *fill).collect::<Vec<_>>(),
        vec![tokens.palette.row_selected],
        "a picked row under the pointer painted more than the selection"
    );

    let hovered = fills(&ctx, |ui| {
        let _ = ui.allocate_rect(row, egui::Sense::hover());
        wash(ui, row, 1.0, Look::default(), &tokens);
    });
    assert_eq!(
        hovered.iter().map(|(_, fill)| *fill).collect::<Vec<_>>(),
        vec![tokens.palette.row_hover],
        "a hovered row painted something else"
    );

    let resting = fills(&ctx, |ui| {
        let _ = ui.allocate_rect(row, egui::Sense::hover());
        wash(ui, row, 0.0, Look::default(), &tokens);
    });
    assert!(
        resting.is_empty(),
        "a row at rest painted {} fills",
        resting.len()
    );
}

#[test]
fn a_lifted_row_is_drawn_at_the_lifted_opacity() {
    let ctx = context();
    let row = Rect::from_min_size(Pos2::ZERO, vec2(400.0, extent::ROW_H));

    let before = frame(&ctx, |ui| {
        let _ = ui.allocate_rect(row, egui::Sense::hover());
        let before = dim(
            ui,
            Look {
                lifted: true,
                ..Look::default()
            },
        );
        assert!(
            (ui.opacity() - opacity::LIFTED).abs() < 1e-6,
            "{}",
            ui.opacity()
        );
        ui.set_opacity(before);
        before
    });
    assert!((before - 1.0).abs() < 1e-6, "{before}");
    assert!(
        (frame(&ctx, |ui| {
            let _ = ui.allocate_rect(row, egui::Sense::hover());
            dim(ui, Look::default())
        }) - 1.0)
            .abs()
            < 1e-6
    );
}

#[test]
fn a_focused_row_does_not_scroll_the_area_while_carrying() {
    let row = Rect::from_min_size(Pos2::ZERO, vec2(700.0, extent::ROW_H));

    let offset = |look: Look| {
        let ctx = context();
        let id = Id::unique("rows-scroll-test");
        let mut offset = 0.0;
        for _ in 0..6 {
            let mut output = ctx.run_ui(input(), |ui| {
                let area = egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        // The row sits well below the fold, so revealing it
                        // means scrolling the area.
                        ui.allocate_space(vec2(700.0, 1000.0));
                        let rect = Rect::from_min_size(ui.cursor().min, row.size());
                        ui.allocate_space(row.size());
                        ui.ctx().memory_mut(|memory| memory.request_focus(id));
                        let _ = body(ui, rect, id, look);
                    });
                offset = area.state.clamped_offset().y;
            });
            output.textures_delta.clear();
        }
        offset
    };

    assert!(
        offset(Look::default()) > 0.0,
        "a focused row off the fold did not come into view"
    );
    assert_eq!(
        offset(Look {
            carrying: true,
            ..Look::default()
        }),
        0.0,
        "the list scrolled to a row while a block was carried"
    );
}
