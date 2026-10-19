//! The discography's filter row: the chips the artist's releases allow.
//!
//! The row is one control, not one per chip: `Control::DiscographyFilter` says
//! the row can act, and each chip is drawn inside its own id scope so that the
//! chips do not share one hover, one focus node or one AccessKit node — the
//! same reason a release block scopes itself.
//!
//! The row wraps with [`layout::chips::wrap`], the sidebar's rule, and reports
//! the height its rows took so the blocks below move down by exactly that much.

use egui::{Sense, Ui, Vec2};

use crate::availability::Control;
use crate::discography::Chip;
use crate::format::filter_label;
use crate::layout::chips;
use crate::theme::{Tokens, space};

use super::controls;

/// Draws the row and returns the chip a press chose, if any.
///
/// `active` is the chip drawn as pressed, which is the effective choice rather
/// than the raw one: a choice with no chip is drawn as `All`.
pub fn show(ui: &mut Ui, tokens: &Tokens, row: &[Chip], active: Chip) -> Option<Chip> {
    let sizes: Vec<Vec2> = row
        .iter()
        .map(|chip| controls::chip_size(ui, tokens, filter_label(*chip)))
        .collect();
    // A row is as tall as its tallest chip: unlike the sidebar, the page's row
    // is the chips and nothing else.
    let row_h = sizes.iter().map(|size| size.y).fold(0.0, f32::max);
    let width = ui.available_width();
    let wrapped = chips::wrap(&sizes, width, space::S1, row_h, space::S1);
    let (area, _) = ui.allocate_exact_size(Vec2::new(width, wrapped.height), Sense::hover());

    let mut picked = None;
    for (chip, rect) in row.iter().zip(wrapped.rects) {
        let rect = rect.translate(area.min.to_vec2());
        let label = filter_label(*chip);
        let slot = ui
            .scope_builder(egui::UiBuilder::new().id_salt(label).max_rect(rect), |ui| {
                // The row has already been allocated, so the scope's extent
                // is the chip itself: the parent's cursor stays where the
                // row left it rather than moving to each chip in turn.
                ui.expand_to_include_rect(rect);
                controls::chip_in(
                    ui,
                    tokens,
                    Control::DiscographyFilter,
                    rect,
                    label,
                    *chip == active,
                )
            })
            .inner;
        if slot.response.clicked() {
            picked = Some(*chip);
        }
    }

    ui.add_space(space::S4);
    picked
}
