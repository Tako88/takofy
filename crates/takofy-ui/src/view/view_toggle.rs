//! The two-button group that picks how the library draws its saved things.
//!
//! The reference puts it at the trailing edge of the chip row: two squares, a
//! hairline between them, the chosen one taking the text colour for its border.

use egui::{Pos2, Rect, Ui, Vec2};

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
        if slot.response.clicked() && !selected {
            picked = Some(layout);
        }
    }
    // The pair reads as one control: a frame around both cells and a hairline
    // where they meet, which is the width `width` reserves.
    ui.painter().rect_stroke(
        group,
        radius::SM,
        egui::Stroke::new(stroke::HAIRLINE, tokens.palette.border),
        egui::StrokeKind::Inside,
    );
    let divider = Rect::from_min_size(
        Pos2::new(
            group.center().x - stroke::HAIRLINE / 2.0,
            group.top() + space::S1 / 2.0,
        ),
        Vec2::new(stroke::HAIRLINE, group.height() - space::S1),
    );
    ui.painter()
        .rect_filled(divider, 0.0, tokens.palette.border);
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
mod tests;
