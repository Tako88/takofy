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
//! because the band pinned. Its trailing cluster carries the name field and,
//! while a query is on, the count over the whole discography; the field is drawn
//! after the chips, so Tab walks the row in its reading order.

use egui::{Pos2, Rect, Ui, pos2, vec2};
use takofy_model::Release;

use crate::availability::Control;
use crate::discography::{self, Chip, Filter};
use crate::format::{filter_label, release_filter_count_label};
use crate::layout::filter_row as cluster_layout;
use crate::layout::sticky::{self, Hold};
use crate::theme::{Tokens, cluster, space, stroke};
use crate::track_filter::QUERY_MAX;

use super::filter_field::{self, Rule};
use super::filter_row::{self, RowSlot};
use super::{filter_count, pin};

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
    /// Every release under the chips, so the field can compose its name with
    /// the chip and the count can be measured over the whole discography.
    pub releases: &'a [Release],
    /// The page's filter: the field edits its query and the count reads it.
    pub filter: &'a mut Filter,
}

/// What drawing the band reported.
pub struct Shown {
    /// The chip a press chose, if any.
    pub picked: Option<Chip>,
    /// The band's rect: its slot in the flow, the floor, or the end's edge.
    pub band: Rect,
    /// How the band stands, which decides the body's clip and the seam.
    pub hold: Hold,
    /// The true release indices the post-edit chip and query leave.
    pub hits: Vec<usize>,
    /// Whether a query is typed after the field's edit.
    pub queried: bool,
}

/// Lays the row out in the flow, draws it where it stands, and reports the band.
pub fn show(ui: &mut Ui, tokens: &Tokens, row: Row<'_>) -> Shown {
    let total = row.releases.len();
    let pre_needle = row.filter.needle();
    let pre_hits = match &pre_needle {
        Some(needle) => discography::narrow(row.releases, row.active, needle),
        None => discography::narrow(row.releases, row.active, ""),
    };
    // The count is measured from the state the frame started with, so the field
    // reserves its column before the edit resolves. A keystroke that brings the
    // count in asks for one more frame rather than moving anything under the
    // pointer.
    let pre_count_w = pre_needle.as_ref().map(|_| {
        filter_count::layout(
            ui,
            &release_filter_count_label(pre_hits.len(), total),
            tokens,
        )
        .size()
        .x
    });
    let slot = RowSlot::Cluster(cluster_layout::size(pre_count_w));
    let laid = filter_row::lay_out(ui, tokens, row.chips, filter_label, slot);
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
    // The band is laid out where it belongs in the flow and painted where it
    // stands. Everything in the cluster moves by this one delta, so the count
    // cannot stay behind in the slot while the field and the chips ride.
    let delta = stuck.rect.min - laid.area.min;

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

    // The field is the row's last widget, after the chips, so Tab walks the row
    // in its reading order. Its own rule is the row's: while the band holds the
    // edge, the field gives up its hairline until it has the keyboard.
    let rule = if stuck.hold == Hold::Resting {
        Rule::Resting
    } else {
        Rule::FocusOnly
    };
    if let Some(field) = laid.slot {
        filter_field::show(
            ui,
            filter_field::Field {
                id: ui.scope_id().with("release-filter"),
                rect: field.translate(delta),
                text: row.filter.edit(),
                limit: QUERY_MAX,
                placeholder: "FILTER",
                label: "Filter releases",
                rule,
            },
            tokens,
        );
    }

    let post_needle = row.filter.needle();
    let queried = post_needle.is_some();
    let hits = match &post_needle {
        Some(needle) => discography::narrow(row.releases, row.active, needle),
        None => discography::narrow(row.releases, row.active, ""),
    };

    if queried {
        let words = release_filter_count_label(hits.len(), total);
        let label = filter_count::layout(ui, &words, tokens);
        if let Some(field) = laid.slot {
            // The count sits one gap left of the field. It is painted only while
            // it has room before the first line's chips: the field is the tool,
            // and the count is the first thing to give. The layout reserved the
            // pre-edit count's box; a post-edit label of another width is placed
            // against the same field, because the field itself never moves. What
            // has to clear the chips is the painted label's left edge, not the
            // reservation's: a label that grew starts left of the box it reserved.
            let count = laid.count.unwrap_or_else(|| {
                Rect::from_min_size(
                    pos2(field.left() - cluster::GAP - label.size().x, field.top()),
                    vec2(label.size().x, field.height()),
                )
            });
            let painted_left = count.right() - label.size().x;
            if cluster_layout::count_clears_chips(
                painted_left,
                laid.chips.iter().map(|(_, rect)| rect),
            ) {
                filter_count::paint(ui, count.translate(delta), label, tokens);
            }
        }
        filter_count::announce(ui, egui::Id::unique("release-count"), &words);
    }

    // A keystroke changes what the blocks show and what they need: the page can
    // draw the post-edit set this frame, but the section's end and the count's
    // reservation were solved from the pre-edit state. Ask for the frame that
    // makes them agree, and only when the set actually moved.
    if pre_hits != hits {
        ui.ctx().request_repaint();
    }

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
        hits,
        queried,
    }
}
