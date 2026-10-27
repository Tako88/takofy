//! The shared link: painted words that open a page.
//!
//! A surface lays its line out once and hands the galley here; this module
//! decides what the pointer can click, what the cursor says, what assistive
//! technology announces, and how the hover ink eases. A link with no page to
//! open is painted as plain text and takes nothing.

use std::sync::Arc;

use egui::emath::GuiRounding as _;
use egui::{Color32, FontId, Galley, Pos2, Rect, Sense, Ui, vec2};

use crate::route::{Page, credit_is_show};
use crate::theme::{Tokens, ease, extent, motion, radius, stroke};
use crate::view::text as recipe;

/// The page a link opens.
#[derive(Clone, Copy)]
pub(super) enum Target<'a> {
    Album(&'a str),
    Credit(&'a str),
}

impl Target<'_> {
    /// The page the target names.
    fn page(self) -> Page {
        match self {
            Self::Album(uri) => Page::Album(uri.to_owned()),
            Self::Credit(uri) => Page::credit(uri),
        }
    }

    /// The page kind's name, spelled as the link's label says it.
    fn kind(self) -> &'static str {
        match self {
            Self::Album(_) => "album",
            Self::Credit(uri) if credit_is_show(uri) => "show",
            Self::Credit(_) => "artist",
        }
    }
}

/// The type a link is drawn in.
pub(super) struct Face {
    pub font: FontId,
    pub tracking: f32,
}

/// Lays `text` out as one elided line in `face`.
///
/// The layout ink is a placeholder: the galley is painted through
/// [`Placed::paint`], and layout does not depend on colour.
pub(super) fn lay(ui: &Ui, text: &str, face: &Face, width: f32) -> Arc<Galley> {
    recipe::line(
        ui,
        text,
        face.font.clone(),
        Color32::PLACEHOLDER,
        face.tracking,
        width,
    )
}

/// A line laid out at its place on screen, ready to be painted.
pub(super) struct Placed {
    pub pos: Pos2,
    pub galley: Arc<Galley>,
}

impl Placed {
    /// The box the line occupies on screen.
    pub(super) fn rect(&self) -> Rect {
        Rect::from_min_size(self.pos, self.galley.size())
    }

    /// Paints the line in `color`, which must be a real ink.
    ///
    /// Never [`Color32::PLACEHOLDER`]: the tessellator asserts that it never
    /// receives one. The placeholder is only the argument to the layout call
    /// that measures a line, and the ink is chosen before painting.
    pub(super) fn paint(self, ui: &Ui, color: Color32) {
        ui.painter().galley(self.pos, self.galley, color);
    }

    /// Paints the line as a link, or as plain text when there is no page.
    ///
    /// The hit area is the painted, elided words inside `clip`, never the whole
    /// cell. The interaction is created before the label is painted so the
    /// painter can read its hover. The sense is `Sense::CLICK` rather than
    /// `Sense::click()`: a link must not add a focus stop to a long list.
    ///
    /// The hover eases the words from `rest` to full ink and fades a hairline
    /// underline in under them on the same animation, so a link whose rest ink
    /// is already full ink still answers the pointer.
    pub(super) fn link(
        self,
        ui: &Ui,
        id: egui::Id,
        clip: Rect,
        target: Option<Target<'_>>,
        rest: Color32,
        tokens: &Tokens,
    ) -> Shown {
        let rect = self.rect();
        let hit = rect.intersect(clip);
        let Some(target) = target.filter(|_| hit.width() > 0.0 && hit.height() > 0.0) else {
            self.paint(ui, rest);
            return Shown {
                rect,
                hovered: false,
                page: None,
            };
        };
        let response = ui.interact(hit, id, Sense::CLICK);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::Role::Link,
                true,
                format!("Open {} {}", target.kind(), self.galley.text()),
            )
        });
        let hovered = response.hovered();
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let reveal = ui.ctx().animate_bool_with_time_and_easing(
            id.with("hover"),
            hovered,
            motion::FAST,
            ease::out,
        );
        let ink = rest.lerp_to_gamma(tokens.palette.text, reveal);
        self.paint(ui, ink);
        if reveal > 0.0 {
            // Snap y and height only: a hairline must land on whole pixels, but
            // the rule stays exactly as wide as the words.
            let scale = ui.pixels_per_point();
            let underline = Rect::from_min_size(
                Pos2::new(
                    rect.left(),
                    (rect.bottom() - extent::LINK_UNDERLINE_INSET).round_to_pixels(scale),
                ),
                vec2(rect.width(), stroke::HAIRLINE.round_to_pixels(scale)),
            );
            // The rule fades in by alpha; the ink itself is already the hovered
            // ink, so a mid-fade difference in opacity is the fade at work.
            ui.painter()
                .rect_filled(underline, radius::NONE, ink.gamma_multiply(reveal));
        }
        Shown {
            rect,
            hovered,
            page: response.clicked().then(|| target.page()),
        }
    }
}

/// What one link drew and reported.
pub(super) struct Shown {
    /// The box the painted words occupy.
    pub rect: Rect,
    /// Whether the pointer is on the words.
    pub hovered: bool,
    /// The page a click asked for.
    pub page: Option<Page>,
}

#[cfg(test)]
mod tests;
