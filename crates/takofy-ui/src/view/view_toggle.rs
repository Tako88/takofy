//! The two-button group that picks how the library draws its saved things.
//!
//! The reference puts it at the trailing edge of the chip row: two squares, a
//! hairline between them, the chosen one taking the text colour for its border.

use egui::{Pos2, Rect, Sense, Ui, Vec2};

use crate::availability::Control;
use crate::icon::Icon;
use crate::library::MediaLayout;
use crate::theme::icon::Size;
use crate::theme::{Tokens, extent, radius, space, stroke};

use super::controls;

/// The width the group takes at the trailing edge of a row.
pub fn width() -> f32 {
    2.0 * extent::BUTTON + stroke::HAIRLINE
}

/// The rect the group occupies, flush against `row`'s right edge and centred on
/// it.
pub fn rect(row: Rect) -> Rect {
    Rect::from_min_size(
        Pos2::new(row.right() - width(), row.center().y - extent::BUTTON / 2.0),
        Vec2::new(width(), extent::BUTTON),
    )
}

/// Draws the pair and returns the density a press asked for.
pub fn show(ui: &mut Ui, tokens: &Tokens, row: Rect, chosen: MediaLayout) -> Option<MediaLayout> {
    let group = rect(row);
    let cell = Vec2::splat(extent::BUTTON);
    let mut picked = None;
    for (layout, x) in [
        (MediaLayout::Grid, group.left()),
        (MediaLayout::List, group.right() - extent::BUTTON),
    ] {
        let rect = Rect::from_min_size(Pos2::new(x, group.top()), cell);
        let selected = chosen == layout;
        let slot = controls::icon_button_keyed(
            ui,
            tokens,
            control_of(layout),
            ("library-layout", layout),
            rect,
            icon_of(layout),
            Size::SM,
            label_of(layout),
            true,
        );
        if selected {
            ui.painter().rect_stroke(
                rect,
                radius::SM,
                egui::Stroke::new(stroke::HAIRLINE, tokens.palette.text),
                egui::StrokeKind::Inside,
            );
            ui.painter()
                .rect_filled(rect.shrink(1.0), radius::SM, tokens.palette.surface_raised);
            // The fill went over the glyph, so it is painted again on top.
            crate::icon::paint(
                ui.painter(),
                icon_of(layout),
                Rect::from_center_size(rect.center(), Vec2::splat(Size::SM.side)),
                Size::SM,
                tokens.palette.text,
            );
        }
        if slot.response.clicked() {
            picked = Some(layout);
        }
    }
    // The group's own frame: the reference draws one border around the pair.
    let _ = ui.interact(
        group,
        ui.scope_id().with("library-layout-group"),
        Sense::hover(),
    );
    picked
}

fn control_of(layout: MediaLayout) -> Control {
    match layout {
        MediaLayout::Grid => Control::LayoutGrid,
        MediaLayout::List => Control::LayoutList,
    }
}

fn icon_of(layout: MediaLayout) -> Icon {
    match layout {
        MediaLayout::Grid => Icon::Grid,
        MediaLayout::List => Icon::List,
    }
}

fn label_of(layout: MediaLayout) -> &'static str {
    match layout {
        MediaLayout::Grid => "Grid layout",
        MediaLayout::List => "List layout",
    }
}

/// The gap the chip row keeps before the group, so a chip never touches it.
pub fn reserved() -> f32 {
    width() + space::S2
}

#[cfg(test)]
mod tests {
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
        }
    }
}
