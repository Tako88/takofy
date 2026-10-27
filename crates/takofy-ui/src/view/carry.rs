//! The block a drag carries, and the list that owns one.
//!
//! A carried block is egui's drag payload: one `Carry`, put there by the list the
//! drag began in and read by the shell until the pointer is released. It is owned
//! data with no borrows, because the payload outlives the frame that made it.
//!
//! A page list opens one [`List`] for the frame, tells it which row is selected
//! and which was lifted, and finishes it after its rows are drawn. The list hands
//! the stored selection back to the frame's temp memory, so a selection survives
//! the frames a click does not touch and is forgotten when the page changes. A
//! page that may reorder also offers its rows, so the shell can land a drop in the
//! list they came from.
//!
//! None of this draws or moves anything: the shell is the only side that resolves
//! the payload and the offer, paints its ghost, applies the drop and clears it.

use std::ops::Range;
use std::sync::Arc;

use egui::{Context, DragAndDrop, Id, Vec2};
use takofy_model::TrackEntry;

use crate::landing::Zone;
use crate::likes::{Likes, Standing};
pub use crate::menu::Listing;
use crate::menu::{Cut, Door, Gate, Request};
use crate::queue::Queue;
use crate::route::Page;
use crate::selection::{Picks, Press, Selection};
use crate::verbs;

use super::context_menu;
use super::queue_pane::Lift;
use super::queue_row;
use super::rows::Look;
use super::track_row;

/// Where a carried block came from, which is what a drop will be asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The queue, which moves its own rows.
    Queue,
    /// A page list, which inserts its rows into the queue.
    List(Id),
    /// A sidebar row, which reorders the playlists.
    Playlist,
}

/// What a carried block's ghost draws.
#[derive(Clone, Debug, PartialEq)]
pub enum Ghost {
    /// A track row's own snapshot.
    Table(track_row::Lifted),
    /// A queue row's own snapshot, which is also an episode's.
    Queue(queue_row::Lifted),
}

/// The block a drag carries: where it came from, what travels, and how it draws.
#[derive(Clone, Debug, PartialEq)]
pub struct Carry {
    pub source: Source,
    /// The rows' indices in the list they came from.
    pub rows: Vec<usize>,
    /// The entries themselves, parallel to `rows`, so a drop needs nothing from
    /// the source list.
    pub entries: Vec<TrackEntry>,
    /// The size the ghost is drawn at.
    pub size: Vec2,
    pub ghost: Ghost,
    /// The playlist a sidebar row carries, which has no entries to travel.
    pub playlist: Option<String>,
}

impl Carry {
    /// Every track this carry would add, in the order the rows were taken.
    ///
    /// A block that holds anything else is refused whole: the pick is every row
    /// the drag took or nothing, so the caller never has to look inside.
    pub fn pick(&self) -> Option<Vec<String>> {
        if self.source == Source::Playlist || self.rows.len() != self.entries.len() {
            return None;
        }
        verbs::pick(&self.entries)
    }

    /// Whether this carry holds a pick an add can take.
    pub fn addable(&self) -> bool {
        self.pick().is_some()
    }
}

// The payload egui accepts is `Any + Send + Sync`; this makes a `Carry` that
// stops qualifying a compile error rather than a runtime one.
const _: fn() = || {
    fn ok<T: Send + Sync + 'static>() {}
    ok::<Carry>();
};

/// The Playlists column's rows, offered as a target while something is carried.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnOffer {
    /// The rows' box, which a reorder and an add are both resolved against.
    pub zone: Zone,
    /// The height a row is drawn at, which is the fill's height.
    pub drawn: f32,
    /// The true row each drawn slot stands for, when a query narrowed the list.
    ///
    /// `None` when the drawn set is the whole list, so an unnarrowed column
    /// costs no lookup and a reorder reads the same index it always did.
    pub rows: Option<Vec<usize>>,
}

impl ColumnOffer {
    /// The true row behind a drawn slot, or `None` when the slot names no row.
    ///
    /// Every write a drop makes goes through this: a narrowed column's slot is
    /// not the playlist's index, and adding to, or moving, the slot's number
    /// would name another playlist.
    pub fn source_row(&self, slot_row: usize) -> Option<usize> {
        match &self.rows {
            None => Some(slot_row),
            Some(rows) => rows.get(slot_row).copied(),
        }
    }
}

/// The rows an editable page offers as a drop target this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    /// The list that offered, which is the id its rows lift under.
    pub list: Id,
    /// The playlist those rows belong to.
    pub playlist: String,
    /// The rows' box, which a drop is resolved against.
    pub zone: Zone,
}

/// The id the frame's selections live under.
fn picks() -> Id {
    Id::unique("picks")
}

/// The id the frame's offer lives under.
fn offer_id() -> Id {
    Id::unique("reorder-offer")
}

/// The id the column's offer lives under.
fn column_offer_id() -> Id {
    Id::unique("column-offer")
}

/// Puts `carry` in the frame's payload: what the shell resolves, paints and
/// applies.
pub fn lift(ctx: &Context, carry: Carry) {
    DragAndDrop::set_payload(ctx, carry);
}

/// The block being carried, if one is.
pub fn carried(ctx: &Context) -> Option<Arc<Carry>> {
    DragAndDrop::payload(ctx)
}

/// Whether a block is carried at all, from any list.
pub fn carrying(ctx: &Context) -> bool {
    DragAndDrop::has_any_payload(ctx)
}

/// The offer a page made this frame, read once so it cannot outlive the frame
/// that drew it: a page that did not draw offers nothing.
pub fn take_offer(ctx: &Context) -> Option<Offer> {
    ctx.data_mut(|data| {
        let offer = data.get_temp::<Offer>(offer_id());
        data.remove::<Offer>(offer_id());
        offer
    })
}

/// Offers the Playlists column's rows to the shell, read once like a page's.
pub fn offer_column(ctx: &Context, offer: ColumnOffer) {
    ctx.data_mut(|data| data.insert_temp(column_offer_id(), offer));
}

/// The column a frame drew as a target, read once so it cannot outlive it.
pub fn take_column_offer(ctx: &Context) -> Option<ColumnOffer> {
    ctx.data_mut(|data| {
        let offer = data.get_temp::<ColumnOffer>(column_offer_id());
        data.remove::<ColumnOffer>(column_offer_id());
        offer
    })
}

/// Replaces `list`'s selection with `rows`, so the next frame draws what landed
/// as the picked rows.
pub fn select_rows(ctx: &Context, list: Id, rows: Range<usize>) {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Picks>(picks())
            .get_mut(list)
            .set(rows);
    });
}

/// Forgets every list's selection when the page changes.
pub fn follow_page(ctx: &Context, page: Option<&Page>) {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Picks>(picks()).follow(page);
    });
}

/// What a list posts a request with: the list it is, the page it is on, and the
/// set its library row reads.
#[derive(Clone, Debug, PartialEq)]
pub struct Open<'a> {
    pub listing: Listing,
    pub here: Option<&'a Page>,
    pub likes: &'a Likes,
}

/// What these targets have in common, as the library row reads it.
fn standing_of(likes: &Likes, targets: &[TrackEntry]) -> Standing {
    let uris: Vec<&str> = targets.iter().map(|entry| entry.uri.as_str()).collect();
    likes.standing(&uris)
}

/// Forgets `rows` from `list`'s selection, the shift a removal leaves behind.
pub fn forget_rows(ctx: &Context, list: Id, rows: &[usize]) {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Picks>(picks())
            .get_mut(list)
            .without(rows);
    });
}

/// The block a queue lift carries, or `None` when the queue will not lift it or
/// cannot name an entry for every row it picked.
pub fn from_queue(queue: &mut Queue, lift: Lift) -> Option<Carry> {
    let rows = queue.lift(lift.index)?;
    let entries = queue.entries_at(&rows);
    // `rows` keeps a picked row the queue has since lost while `entries` drops it,
    // and a carry whose two lists disagree is a drop nothing can apply.
    if entries.len() != rows.len() {
        return None;
    }
    Some(Carry {
        source: Source::Queue,
        rows,
        entries,
        size: lift.size,
        ghost: Ghost::Queue(lift.ghost),
        playlist: None,
    })
}

/// Opens the queue's menu on `row`, acting on the whole pick it sits in.
///
/// The gate is checked before the pick changes, so a refused open leaves the
/// queue exactly as it was, and a row the queue will not edit opens nothing.
pub fn queue_menu(
    queue: &mut Queue,
    ctx: &Context,
    door: Door,
    row: usize,
    gate: Gate,
    likes: &Likes,
    here: Option<&Page>,
) {
    if !gate.allows() {
        return;
    }
    let rows = queue.act_on(row);
    let entries = queue.entries_at(&rows);
    if rows.is_empty() || rows.len() != entries.len() {
        return;
    }
    let items = verbs::track_items(
        &entries,
        here,
        &Listing::Queue,
        standing_of(likes, &entries),
    );
    context_menu::post(
        ctx,
        Request {
            door,
            items,
            playlists: Vec::new(),
            targets: entries,
            rows,
            listing: Listing::Queue,
            page: here.cloned(),
        },
    );
}

/// Posts the cut a queue `Del` makes on `row`, acting on the pick it sits in.
///
/// A refused gate leaves the pick alone, so a dropped intent changes nothing.
pub fn queue_cut(queue: &mut Queue, ctx: &Context, row: usize, gate: Gate) {
    if !gate.allows() {
        return;
    }
    let rows = queue.act_on(row);
    let entries = queue.entries_at(&rows);
    if rows.is_empty() || rows.len() != entries.len() {
        return;
    }
    context_menu::post_cut(
        ctx,
        Cut {
            listing: Listing::Queue,
            rows,
            entries,
        },
    );
}

/// Opens Now Playing's menu on the entry the pane draws.
///
/// `entry` is the one the pane is drawing, handed in by the shell, and it is
/// never removable: the menu lists what can be done with the track, not with a
/// list it sits in. Nothing drawn means no menu.
pub fn current_menu(
    entry: Option<&TrackEntry>,
    ctx: &Context,
    door: Door,
    gate: Gate,
    likes: &Likes,
    here: Option<&Page>,
) {
    if !gate.allows() {
        return;
    }
    let Some(entry) = entry.cloned() else {
        return;
    };
    let targets = std::slice::from_ref(&entry);
    let items = verbs::track_items(
        targets,
        here,
        &Listing::Current,
        standing_of(likes, targets),
    );
    if items.is_empty() {
        return;
    }
    context_menu::post(
        ctx,
        Request {
            door,
            items,
            playlists: Vec::new(),
            targets: vec![entry],
            rows: vec![0],
            listing: Listing::Current,
            page: here.cloned(),
        },
    );
}

/// One list's selection and its drag, for the frame that draws it.
#[must_use]
pub struct List {
    id: Id,
    selection: Selection,
    changed: bool,
    lift: Option<Carry>,
    offer: Option<Offer>,
    carry: Option<Arc<Carry>>,
}

impl List {
    /// Opens the list's selection, keeping only the rows `count` still names.
    pub fn open(ctx: &Context, id: Id, count: usize) -> Self {
        let stored = ctx.data_mut(|data| {
            data.get_temp_mut_or_default::<Picks>(picks())
                .get_mut(id)
                .clone()
        });
        let mut selection = stored.clone();
        selection.keep(0..count);
        Self {
            id,
            changed: selection != stored,
            selection,
            lift: None,
            offer: None,
            carry: carried(ctx),
        }
    }

    /// How `row` stands in this list this frame.
    pub fn look(&self, row: usize) -> Look {
        let lifted = self.carry.as_ref().is_some_and(|carry| {
            carry.rows.contains(&row)
                && matches!(carry.source, Source::List(list) if list == self.id)
        });
        Look {
            selected: self.selection.contains(row),
            lifted,
            carrying: self.carry.is_some(),
        }
    }

    /// Drops every pick and the anchor that `keep` rejects.
    ///
    /// A filtered list calls this so a hidden row can never reach the Delete
    /// gate or a menu. The stored selection is only rewritten when one fell.
    pub fn restrict(&mut self, keep: impl Fn(usize) -> bool) {
        let before = self.selection.clone();
        self.selection.retain(keep);
        if self.selection != before {
            self.changed = true;
        }
    }

    /// Applies a press to `row`.
    pub fn press(&mut self, row: usize, press: Press) {
        self.selection.press(row, press);
        self.changed = true;
    }

    /// Picks `row` for a carry, keeping the pick it already sits in.
    ///
    /// The entries are fetched by index through `entry_at` because a frame draws
    /// only the rows its viewport shows; a picked row off screen still travels.
    /// A pick the source can no longer answer for is refused whole.
    pub fn lift(
        &mut self,
        row: usize,
        size: Vec2,
        ghost: Ghost,
        entry_at: impl Fn(usize) -> Option<TrackEntry>,
    ) {
        let mut selection = self.selection.clone();
        selection.lift(row);
        let rows: Vec<usize> = selection.rows().collect();
        let Some(entries) = rows
            .iter()
            .map(|row| entry_at(*row))
            .collect::<Option<Vec<TrackEntry>>>()
        else {
            return;
        };
        self.selection = selection;
        self.changed = true;
        self.lift = Some(Carry {
            source: Source::List(self.id),
            rows,
            entries,
            size,
            ghost,
            playlist: None,
        });
    }

    /// The gate a menu on this list's rows opens under.
    ///
    /// A carried block is this list's own payload; a write in flight is the
    /// caller's, because only the library knows about it.
    pub fn gate(&self, writing: bool) -> Gate {
        Gate {
            carrying: self.carry.is_some(),
            writing,
        }
    }

    /// Opens the context menu on `row`, acting on the whole pick it sits in.
    ///
    /// A row inside the pick acts on all of it and leaves the pick alone; a row
    /// outside it is picked first, so a menu about one row never stands over
    /// another row that looks picked. A refused gate changes nothing, not even
    /// the pick, and a pick this list cannot answer for is refused whole.
    pub fn menu(
        &mut self,
        ctx: &Context,
        door: Door,
        row: usize,
        gate: Gate,
        open: Open<'_>,
        entry_at: impl Fn(usize) -> Option<TrackEntry>,
    ) {
        if !gate.allows() {
            return;
        }
        let mut selection = self.selection.clone();
        let rows = selection.act_on(row);
        let Some(targets) = rows
            .iter()
            .map(|row| entry_at(*row))
            .collect::<Option<Vec<TrackEntry>>>()
        else {
            return;
        };
        self.selection = selection;
        self.changed = true;
        let items = verbs::track_items(
            &targets,
            open.here,
            &open.listing,
            standing_of(open.likes, &targets),
        );
        context_menu::post(
            ctx,
            Request {
                door,
                items,
                playlists: Vec::new(),
                targets,
                rows,
                listing: open.listing,
                page: open.here.cloned(),
            },
        );
    }

    /// The rows a key acts on: the whole pick when `row` sits in it, otherwise
    /// `row` alone, which the pick then becomes.
    pub fn acting_rows(&mut self, row: usize) -> Vec<usize> {
        self.changed = true;
        self.selection.act_on(row)
    }

    /// Offers this list's rows to the shell as a target a reorder can land in.
    pub fn offer(&mut self, playlist: &str, zone: Zone) {
        self.offer = Some(Offer {
            list: self.id,
            playlist: playlist.to_owned(),
            zone,
        });
    }

    /// Stores the selection for the next frame and sets this frame's payload and
    /// offer.
    pub fn finish(self, ctx: &Context) {
        if self.changed {
            ctx.data_mut(|data| {
                data.get_temp_mut_or_default::<Picks>(picks())
                    .get_mut(self.id)
                    .clone_from(&self.selection);
            });
        }
        if let Some(carry) = self.lift {
            lift(ctx, carry);
        }
        if let Some(offer) = self.offer {
            ctx.data_mut(|data| data.insert_temp(offer_id(), offer));
        }
    }
}

#[cfg(test)]
mod tests;
