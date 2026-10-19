//! The box a cover goes in.
//!
//! Every piece that draws a cover's box goes through this one seam. The [cover]
//! pipeline hands it a `Cover`, which is `Ready` with a texture, `Loading`,
//! `Missing` or `Failed`.
//!
//! The step that makes the four states look different is a separate, deliberate
//! change; until then every state draws the same raised block inside a hairline,
//! the same tone whatever the piece. A row, a card and a header band differ in
//! their radius and their border, which is why those are arguments and the fill
//! is not.
//!
//! Callers keep the rects and radii they chose; only this body changes.

use egui::{Color32, CornerRadius, Painter, Rect, Stroke, StrokeKind};

use crate::cover::Cover;
use crate::theme::{Tokens, stroke};

/// Paints a cover's box: a raised block inside a hairline.
pub fn tile(
    painter: &Painter,
    rect: Rect,
    radius: CornerRadius,
    border: Color32,
    cover: Cover,
    tokens: &Tokens,
) {
    // Every state draws the same block for now; the appearance step reads this.
    let _ = cover;
    painter.rect(
        rect,
        radius,
        tokens.palette.surface_raised,
        Stroke::new(stroke::HAIRLINE, border),
        StrokeKind::Inside,
    );
}
