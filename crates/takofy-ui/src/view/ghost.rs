//! The carried block, drawn beside the pointer.
//!
//! The ghost rides the tooltip layer, above every panel, so a block dragged out of
//! a list is not cut off by that list's clip rect. An accepted target draws it
//! raised, with a shadow, and drops the marker where it will land; a refused one
//! draws it flat and draws no marker at all.

use egui::{Id, LayerId, Order, Pos2, Rect, Stroke, StrokeKind, Ui, UiBuilder, vec2};

use crate::format::{format_position, total_duration_ms};
use crate::landing::Landing;
use crate::theme::{Tokens, radius, space, stroke};

use super::carry::{Carry, Ghost};
use super::rows::Ghosting;

/// One frame of the ghost: what travels, where it lands, and where the pointer is.
pub struct Paint<'a> {
    pub carry: &'a Carry,
    /// The landing this frame resolved, or `None` when the pointer is over
    /// nothing that takes the block.
    pub landing: Option<Landing>,
    pub pointer: Pos2,
    pub tokens: &'a Tokens,
}

/// Draws the carried block offset from the pointer, and the marker where it lands.
pub fn paint(ui: &mut Ui, paint: Paint<'_>) {
    let Paint {
        carry,
        landing,
        pointer,
        tokens,
    } = paint;
    let flat = landing.is_none();
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
    if let Some(landing) = landing {
        ghost
            .painter()
            .rect_filled(landing.marker, radius::NONE, tokens.palette.mark);
    }
}
