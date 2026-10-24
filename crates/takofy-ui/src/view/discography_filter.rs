//! The discography's chip row as a band: it holds under the artist's action row
//! while the blocks pass beneath it, and lets go at the end of the discography.
//!
//! The reference makes this row `position: sticky; top: var(--actionbar-h)`
//! *inside the discography's own section*, so it releases at the section's end
//! rather than banding over the likeness row below. The band is opaque: the rows
//! under it are painted after it, so anything it left clear would show their ink
//! rather than the page behind it. While it is not in its slot it wears a
//! hairline, which is the reference's `.is-stuck` edge saying the rows slide
//! under it.
//!
//! The band is laid out where it belongs in the flow and drawn where it stands,
//! so its slot stays reserved and the page's scroll extent does not change
//! because the band pinned.

use egui::{Pos2, Rect, Ui, vec2};

use crate::availability::Control;
use crate::discography::Chip;
use crate::format::filter_label;
use crate::layout::sticky::{self, Hold};
use crate::theme::{Tokens, space, stroke};

use super::filter_row::{self, RowSlot};
use super::pin;

/// What the band filters, and how far its section runs.
pub struct Row<'a> {
    /// The chips the artist's releases allow.
    pub chips: &'a [Chip],
    /// The chip drawn as pressed.
    pub active: Chip,
    /// The height of every block the chip keeps, from the first to the last.
    pub body_h: f32,
    /// The top of the page's scroll viewport, read before the action row shrank
    /// the clip to its own bottom.
    pub viewport_top: f32,
    /// The pane's own rect, so the band's surface reaches both of its edges
    /// rather than stopping at the gutter the chips sit inside.
    pub pane: Rect,
}

/// What drawing the band reported.
pub struct Shown {
    /// The chip a press chose, if any.
    pub picked: Option<Chip>,
    /// The band's rect: its slot in the flow, the floor, or the end's edge.
    pub band: Rect,
    /// How the band stands, which decides the body's clip and the seam.
    pub hold: Hold,
}

/// Lays the row out in the flow, draws it where it stands, and reports the band.
pub fn show(ui: &mut Ui, tokens: &Tokens, row: Row<'_>) -> Shown {
    let laid = filter_row::lay_out(ui, tokens, row.chips, filter_label, RowSlot::None);
    // The band covers the step the reference keeps under its chips: the surface
    // has to reach the row's own bottom edge, or the rows sliding under it show
    // through the gap.
    let flow = Rect::from_min_size(
        laid.area.min,
        vec2(laid.area.width(), laid.area.height() + space::S4),
    );
    // The row's trailing step, which is also what puts the body's top one item
    // gap below the band.
    ui.add_space(space::S4);
    let body_top = ui.cursor().min.y;
    let floor = ui.clip_rect().top();
    let end = body_top + row.body_h;
    let stuck = sticky::stick(flow, floor, end);

    if stuck.hold != Hold::Resting {
        // Opaque and painted before the chips, because the blocks under the band
        // are painted after it: anything the band left clear would show their ink
        // rather than the page behind them. The surface reaches both pane edges,
        // so the rows do not appear at the gutters beside it.
        let bleed = Rect::from_min_max(
            Pos2::new(row.pane.left(), stuck.rect.top()),
            Pos2::new(row.pane.right(), stuck.rect.bottom()),
        );
        ui.painter().rect_filled(bleed, 0.0, tokens.palette.surface);
        // The reference's `.is-stuck` edge: it stays while the band is riding the
        // end as well as while it holds the floor, because in both the rows pass
        // under it. Inside the band's own height, which is what anything sticky
        // below it clears.
        ui.painter().rect_filled(
            Rect::from_min_size(
                Pos2::new(bleed.left(), bleed.bottom() - stroke::HAIRLINE),
                vec2(bleed.width(), stroke::HAIRLINE),
            ),
            0.0,
            tokens.palette.border,
        );
    }

    let picked = filter_row::draw_at(
        ui,
        tokens,
        &laid,
        row.active,
        |_| Control::DiscographyFilter,
        filter_label,
        stuck.rect.min,
    );

    // Only a band at the floor speaks for the viewport's top. A released band
    // sits above the floor, and a reveal measured from it would ask for a scroll
    // above the clip's own top, which egui answers by dragging the page upward
    // every frame.
    if stuck.hold == Hold::Holding {
        pin::raise(ui.ctx(), stuck.rect.bottom() - row.viewport_top);
    }

    Shown {
        picked,
        band: stuck.rect,
        hold: stuck.hold,
    }
}
