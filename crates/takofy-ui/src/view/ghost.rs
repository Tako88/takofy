//! The carried block, drawn beside the pointer.
//!
//! The ghost rides the tooltip layer, above every panel, so a block dragged out of
//! a list is not cut off by that list's clip rect. An accepted target draws it
//! raised, with a shadow, and drops the marker where it will land; an add draws
//! it raised with no marker; a refused one draws it flat and draws no marker at
//! all.

use egui::{Id, LayerId, Order, Pos2, Rect, Stroke, StrokeKind, Ui, UiBuilder, vec2};

use crate::format::{format_position, total_duration_ms};
use crate::landing::Target;
use crate::theme::{Tokens, radius, space, stroke};

use super::carry::{Carry, Ghost};
use super::rows::Ghosting;

/// One frame of the ghost: what travels, what it lands on, and where the pointer
/// is.
pub struct Paint<'a> {
    pub carry: &'a Carry,
    /// The target this frame resolved, or `None` when the pointer is over
    /// nothing that takes the block.
    pub target: Option<Target>,
    pub pointer: Pos2,
    pub tokens: &'a Tokens,
}

/// Draws the carried block offset from the pointer, and the marker where it lands.
pub fn paint(ui: &mut Ui, paint: Paint<'_>) {
    let Paint {
        carry,
        target,
        pointer,
        tokens,
    } = paint;
    let flat = target.is_none();
    let rect = Rect::from_min_size(
        pointer + vec2(space::S3, space::S4),
        vec2((carry.size.x - space::S3).max(0.0), carry.size.y),
    );

    let mut ghost = ui.new_child(
        UiBuilder::new()
            .layer_id(LayerId::new(Order::Tooltip, Id::unique("carry")))
            .max_rect(rect),
    );
    // The ghost follows the pointer across panels, so it is clipped to the window
    // and not to the list it came from.
    ghost.set_clip_rect(ghost.ctx().content_rect());

    // The row an add would join, under the card so the card stays readable.
    if let Some(filled) = target.and_then(Target::fill) {
        ghost
            .painter()
            .rect_filled(filled, radius::SM, tokens.palette.row_selected);
    }

    let (fill, border) = if flat {
        (tokens.palette.surface, tokens.palette.border)
    } else {
        (tokens.palette.surface_raised, tokens.palette.border_strong)
    };
    if !flat {
        ghost
            .painter()
            .add(tokens.palette.shadow_md.as_shape(rect, radius::MD));
    }
    ghost.painter().rect(
        rect,
        radius::MD,
        fill,
        Stroke::new(stroke::HAIRLINE, border),
        StrokeKind::Inside,
    );

    let total = total_duration_ms(&carry.entries);
    let total = (total > 0).then(|| format_position(total));
    let ghosting = Ghosting {
        count: carry.entries.len(),
        total: total.as_deref(),
        flat,
    };
    match &carry.ghost {
        Ghost::Table(lifted) => lifted.paint(&ghost, rect, ghosting, tokens),
        Ghost::Queue(lifted) => lifted.paint(&ghost, rect, ghosting, tokens),
    }

    // Last, so neither a row nor the ghost's own fill can cover the rule that
    // says where the block will land.
    if let Some(marker) = target.and_then(Target::marker) {
        ghost
            .painter()
            .rect_filled(marker, radius::NONE, tokens.palette.mark);
    }
}

#[cfg(test)]
mod tests {
    use egui::{Color32, Pos2, RawInput, Rect, Shape, Vec2, pos2, vec2};

    use super::*;
    use crate::cover::Cover;
    use crate::landing::{Landing, Slot};
    use crate::theme::{extent, style};
    use crate::view::queue_row;

    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::theme::fonts::definitions());
        ctx.set_global_style(style(&Tokens::DARK));
        ctx
    }

    fn carry() -> Carry {
        Carry {
            source: super::super::carry::Source::Queue,
            rows: vec![0],
            entries: Vec::new(),
            size: vec2(200.0, extent::ROW_H),
            ghost: Ghost::Queue(queue_row::Lifted::new(
                "A Title".to_owned(),
                "A Band".to_owned(),
                None,
                Cover::Missing,
            )),
            playlist: None,
        }
    }

    fn slot() -> Slot {
        Slot {
            row: 1,
            rect: Rect::from_min_size(pos2(40.0, 100.0), vec2(200.0, extent::ROW_H)),
        }
    }

    fn landing() -> Landing {
        Landing {
            boundary: 2,
            marker: Rect::from_min_size(pos2(40.0, 160.0), vec2(200.0, stroke::RULE)),
        }
    }

    /// Paints one frame of the ghost and answers what it drew.
    fn painted(target: Option<Target>) -> Vec<Shape> {
        let ctx = context();
        let tokens = Tokens::DARK;
        let carry = carry();
        let pointer = pos2(20.0, 60.0);
        let mut output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 400.0))),
                ..Default::default()
            },
            |ui| {
                paint(
                    ui,
                    Paint {
                        carry: &carry,
                        target,
                        pointer,
                        tokens: &tokens,
                    },
                );
            },
        );
        output.textures_delta.clear();
        output
            .shapes
            .into_iter()
            .map(|clipped| clipped.shape)
            .collect()
    }

    fn fills(shapes: &[Shape], color: Color32) -> Vec<Rect> {
        shapes
            .iter()
            .filter_map(|shape| match shape {
                Shape::Rect(rect) if rect.fill == color => Some(rect.rect),
                _ => None,
            })
            .collect()
    }

    fn rules(shapes: &[Shape]) -> Vec<Rect> {
        let tokens = Tokens::DARK;
        shapes
            .iter()
            .filter_map(|shape| match shape {
                Shape::Rect(rect)
                    if rect.fill == tokens.palette.mark
                        && (rect.rect.height() - stroke::RULE).abs() < 0.01 =>
                {
                    Some(rect.rect)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_add_paints_the_row_fill_and_no_rule() {
        let tokens = Tokens::DARK;
        let shapes = painted(Some(Target::Add(slot())));

        assert!(
            fills(&shapes, tokens.palette.row_selected).contains(&slot().rect),
            "the row under the pointer takes the fill"
        );
        assert!(rules(&shapes).is_empty(), "an add draws no rule");
    }

    #[test]
    fn a_rootlist_move_paints_the_rule_and_no_fill() {
        let tokens = Tokens::DARK;
        let shapes = painted(Some(Target::Rootlist(landing())));

        assert_eq!(rules(&shapes), vec![landing().marker]);
        assert!(
            fills(&shapes, tokens.palette.row_selected).is_empty(),
            "a reorder draws no fill"
        );
    }

    #[test]
    fn a_refused_target_paints_neither() {
        let tokens = Tokens::DARK;
        let shapes = painted(None);

        assert!(rules(&shapes).is_empty());
        assert!(fills(&shapes, tokens.palette.row_selected).is_empty());
        assert!(
            fills(&shapes, tokens.palette.surface)
                .iter()
                .any(|rect| *rect == ghost_box(pos2(20.0, 60.0), carry().size)),
            "a refused target draws the card flat"
        );
    }

    /// The box the card is drawn in, offset from the pointer.
    fn ghost_box(pointer: Pos2, size: Vec2) -> Rect {
        Rect::from_min_size(
            pointer + vec2(space::S3, space::S4),
            vec2((size.x - space::S3).max(0.0), size.y),
        )
    }
}
