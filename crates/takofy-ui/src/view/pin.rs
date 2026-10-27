//! The seam between a row that pins and a row that has to be seen.
//!
//! egui's `scroll_to_rect` measures a rect against the scroll area's own clip,
//! which is the parent of the `Ui` a page draws in. A page that shrinks its clip
//! to keep rows from painting over the pinned row therefore hides the rows from
//! the pointer and from the culling test, but not from the scroll target: a row
//! whose top sits inside the pinned row's strip is "already in view" to egui and
//! is never brought out from under it.
//!
//! So the pinned row records how far below the viewport's top it reaches, and
//! every reveal asks for its rect's top edge shifted up by that much. The shift
//! is safe on a row that is already clear: egui measures the shifted rect
//! against the same clip and asks for no movement at all.

use egui::{Id, Rect, Ui, vec2};

fn id() -> Id {
    Id::unique("pinned-row-inset")
}

/// How far below the viewport's top this frame's pinned row reaches.
///
/// Zero when nothing pinned, which is every page without a pinned row and every
/// page that has not scrolled past it yet.
pub(crate) fn inset(ctx: &egui::Context) -> f32 {
    ctx.data(|data| data.get_temp(id()).unwrap_or(0.0))
}

/// Records how far a pinned row reaches below the viewport's top, replacing what
/// this frame already held.
///
/// The page's bar writes first and overwrites, so a frame that draws no bar
/// starts from its own height rather than the last page's. A band under the bar
/// adds to it with [`raise`] instead.
pub(crate) fn set(ctx: &egui::Context, inset: f32) {
    ctx.data_mut(|data| data.insert_temp(id(), inset.max(0.0)));
}

/// Adds a band's own reach to this frame's inset, keeping the lowest bottom.
///
/// The bar overwrites the seam because it is the page's first band and because a
/// page that draws no band must not inherit one. A band under it adds its reach
/// instead: the strip a reveal has to clear is the lowest of the page's bands,
/// not whichever one wrote last.
pub(crate) fn raise(ctx: &egui::Context, reach: f32) {
    set(ctx, inset(ctx).max(reach));
}

/// Forgets the inset, so a page without a pinned row never inherits one.
///
/// Called before any region draws, so a page that pins nothing leaves the seam at
/// zero and a row revealed on it lands where egui puts it; and again before a
/// region whose rows are not under the page's bar, so a stuck page cannot shift
/// them.
pub(crate) fn clear(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove_temp::<f32>(id()));
}

/// Brings `rect` into view, clear of whatever pins above it.
///
/// This is the only way a row asks to be seen: a reveal that does not know about
/// the inset leaves the row's top under the pinned row. Only the top edge is
/// shifted, though: the inset is what covers the viewport's top, so it must not
/// make a row below the viewport look shorter than it is.
pub(crate) fn reveal(ui: &mut Ui, rect: Rect) {
    let inset = inset(ui.ctx());
    let asked = Rect::from_min_max(rect.min - vec2(0.0, inset), rect.max);
    ui.scroll_to_rect(asked, None);
}

#[cfg(test)]
mod tests;
