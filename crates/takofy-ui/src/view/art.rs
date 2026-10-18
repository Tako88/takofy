//! The box a cover goes in.
//!
//! Nothing fetches or decodes an image yet, so every piece that will carry art
//! draws this instead: a raised block inside a hairline, the same tone whatever
//! the piece. A row, a card and a header band differ in their radius and their
//! border, which is why those are arguments and the fill is not.
//!
//! This is the one seam the art pipeline fills. Callers keep the rects and radii
//! they chose; only this body changes.

use egui::{Color32, CornerRadius, Painter, Rect, Stroke, StrokeKind};

use crate::theme::{Tokens, stroke};

/// Paints a cover's box: a raised block inside a hairline.
pub fn tile(painter: &Painter, rect: Rect, radius: CornerRadius, border: Color32, tokens: &Tokens) {
    painter.rect(
        rect,
        radius,
        tokens.palette.surface_raised,
        Stroke::new(stroke::HAIRLINE, border),
        StrokeKind::Inside,
    );
}
