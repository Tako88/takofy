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

use std::sync::Arc;

use egui::{Context, DragAndDrop, Id, Vec2};
use takofy_model::TrackEntry;

use crate::landing::Zone;
use crate::queue::Queue;
use crate::route::Page;
use crate::selection::{Picks, Press, Selection};

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
}

// The payload egui accepts is `Any + Send + Sync`; this makes a `Carry` that
// stops qualifying a compile error rather than a runtime one.
const _: fn() = || {
    fn ok<T: Send + Sync + 'static>() {}
    ok::<Carry>();
};

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

/// Replaces `list`'s selection with `row` alone, so the next frame draws the
/// row that landed as the picked one.
pub fn select_only(ctx: &Context, list: Id, row: usize) {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Picks>(picks())
            .get_mut(list)
            .set([row]);
    });
}

/// Forgets every list's selection when the page changes.
pub fn follow_page(ctx: &Context, page: Option<&Page>) {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Picks>(picks()).follow(page);
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
    })
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

    /// Applies a press to `row`.
    pub fn press(&mut self, row: usize, press: Press) {
        self.selection.press(row, press);
        self.changed = true;
    }

    /// Picks `row` for a carry: a page list lifts exactly one row, so the
    /// selection collapses to it and the payload is set when the list finishes.
    pub fn lift(&mut self, row: usize, entry: TrackEntry, size: Vec2, ghost: Ghost) {
        self.selection.set([row]);
        self.changed = true;
        self.lift = Some(Carry {
            source: Source::List(self.id),
            rows: vec![row],
            entries: vec![entry],
            size,
            ghost,
        });
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
mod tests {
    use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};

    use super::*;
    use crate::cover::Cover;
    use crate::player::Transport;
    use crate::selection::Press;
    use crate::theme::extent;

    fn context() -> Context {
        Context::default()
    }

    fn entry(title: &str) -> TrackEntry {
        TrackEntry {
            uri: format!("u{title}"),
            title: title.to_owned(),
            artists: "a".to_owned(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            added_at: None,
            duration_ms: Some(1_000),
            named: true,
            cover: None,
        }
    }

    fn lifted() -> queue_row::Lifted {
        queue_row::Row {
            index: 0,
            title: "A Title",
            artist: "A Band",
            duration: None,
            cover: Cover::Missing,
            look: Look::default(),
            refocus: false,
        }
        .lifted()
    }

    fn ghost() -> Ghost {
        Ghost::Queue(lifted())
    }

    fn carry(source: Source, rows: Vec<usize>) -> Carry {
        Carry {
            source,
            rows,
            entries: Vec::new(),
            size: vec2(100.0, 44.0),
            ghost: ghost(),
        }
    }

    fn zone() -> Zone {
        let rows = Rect::from_min_size(pos2(0.0, 10.0), vec2(300.0, 4.0 * extent::ROW_H));
        Zone {
            hit: rows,
            list: rows,
            row_h: extent::ROW_H,
            first: 0,
            count: 4,
        }
    }

    #[test]
    fn an_offer_is_read_once() {
        let ctx = context();
        let id = Id::unique("list");
        let mut list = List::open(&ctx, id, 4);
        list.offer("spotify:playlist:a", zone());
        assert_eq!(
            take_offer(&ctx),
            None,
            "the frame's offer is set when the list finishes"
        );

        list.finish(&ctx);
        assert_eq!(
            take_offer(&ctx),
            Some(Offer {
                list: id,
                playlist: "spotify:playlist:a".to_owned(),
                zone: zone(),
            }),
            "the frame's offer names the list, the playlist and the rows"
        );
        assert_eq!(take_offer(&ctx), None, "an offer is read once");

        // A page that draws but offers nothing leaves nothing behind, so the
        // frame after it cannot act on the frame before.
        let list = List::open(&ctx, id, 4);
        list.finish(&ctx);
        assert_eq!(
            take_offer(&ctx),
            None,
            "a page that offers nothing offers nothing"
        );
    }

    #[test]
    fn select_only_replaces_a_lists_selection() {
        let ctx = context();
        let first = Id::unique("first");
        let second = Id::unique("second");
        let mut list = List::open(&ctx, first, 6);
        list.press(1, Press::Replace);
        list.press(4, Press::Toggle);
        list.finish(&ctx);
        let mut other = List::open(&ctx, second, 6);
        other.press(2, Press::Replace);
        other.finish(&ctx);

        select_only(&ctx, first, 3);

        let list = List::open(&ctx, first, 6);
        assert!(list.look(3).selected, "the row that landed is the pick");
        assert!(
            !list.look(1).selected && !list.look(4).selected,
            "every pick before it is gone"
        );
        assert!(
            List::open(&ctx, second, 6).look(2).selected,
            "another list keeps its own selection"
        );
    }

    fn queue_of(count: usize, current: usize) -> Queue {
        let tracks: Vec<TrackEntry> = (0..count).map(|index| entry(&index.to_string())).collect();
        let mut queue = Queue::default();
        queue.start(&tracks, current, &mut Transport::default());
        queue
    }

    fn input(events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 400.0))),
            events,
            ..Default::default()
        }
    }

    #[test]
    fn look_marks_picked_and_lifted_rows() {
        let ctx = context();
        let id = Id::unique("list");
        let mut list = List::open(&ctx, id, 5);
        list.press(1, Press::Replace);
        list.finish(&ctx);

        let list = List::open(&ctx, id, 5);
        assert_eq!(
            list.look(1),
            Look {
                selected: true,
                ..Look::default()
            }
        );
        assert_eq!(list.look(2), Look::default());

        // A carry marks the rows that travel and tells the whole list it is
        // carrying.
        lift(&ctx, carry(Source::List(id), vec![1]));
        let list = List::open(&ctx, id, 5);
        assert_eq!(
            list.look(1),
            Look {
                selected: true,
                lifted: true,
                carrying: true
            }
        );
        assert_eq!(
            list.look(2),
            Look {
                carrying: true,
                ..Look::default()
            }
        );

        // Another list sees the carry but none of the rows.
        let other = List::open(&ctx, Id::unique("other"), 5);
        assert_eq!(
            other.look(1),
            Look {
                carrying: true,
                ..Look::default()
            }
        );
    }

    #[test]
    fn a_press_is_stored_on_finish_and_seen_by_the_next_open() {
        let ctx = context();
        let id = Id::unique("list");
        let mut list = List::open(&ctx, id, 5);
        assert_eq!(list.look(3), Look::default());

        list.press(3, Press::Replace);
        list.finish(&ctx);

        assert!(List::open(&ctx, id, 5).look(3).selected);
    }

    #[test]
    fn lifting_a_page_row_selects_only_it_and_sets_the_payload_on_finish() {
        let ctx = context();
        let id = Id::unique("list");
        let mut list = List::open(&ctx, id, 5);
        list.press(2, Press::Replace);
        list.lift(4, entry("x"), vec2(10.0, 20.0), ghost());
        assert!(
            !carrying(&ctx),
            "the payload is set when the list finishes, not while it draws"
        );
        list.finish(&ctx);

        let carry = carried(&ctx).expect("a payload");
        assert_eq!(carry.source, Source::List(id));
        assert_eq!(carry.rows, vec![4]);
        assert_eq!(carry.entries, vec![entry("x")]);
        assert_eq!(carry.size, vec2(10.0, 20.0));

        let list = List::open(&ctx, id, 5);
        assert!(list.look(4).selected, "the lifted row is the selection");
        assert!(!list.look(2).selected, "the rest of the selection is gone");
    }

    #[test]
    fn a_stored_selection_beyond_the_count_is_dropped() {
        let ctx = context();
        let id = Id::unique("list");
        let mut list = List::open(&ctx, id, 8);
        list.press(6, Press::Replace);
        list.press(1, Press::Toggle);
        list.finish(&ctx);

        // The short list keeps only the row it still names, and stores that.
        let list = List::open(&ctx, id, 3);
        assert!(list.look(1).selected);
        assert!(!list.look(6).selected);
        list.finish(&ctx);

        assert!(
            !List::open(&ctx, id, 8).look(6).selected,
            "the dropped row must not come back with the long list"
        );
    }

    #[test]
    fn lists_with_other_ids_keep_their_own_selection() {
        let ctx = context();
        let first = Id::unique("first");
        let second = Id::unique("second");
        let mut list = List::open(&ctx, first, 4);
        list.press(1, Press::Replace);
        list.finish(&ctx);
        let mut list = List::open(&ctx, second, 4);
        list.press(3, Press::Replace);
        list.finish(&ctx);

        assert!(List::open(&ctx, first, 4).look(1).selected);
        assert!(!List::open(&ctx, first, 4).look(3).selected);
        assert!(List::open(&ctx, second, 4).look(3).selected);
    }

    #[test]
    fn follow_page_forgets_on_navigation() {
        let ctx = context();
        let page = |name: &str| Page::Playlist(name.to_owned());
        follow_page(&ctx, Some(&page("a")));

        let id = Id::unique("list");
        let mut list = List::open(&ctx, id, 4);
        list.press(2, Press::Replace);
        list.finish(&ctx);
        assert!(List::open(&ctx, id, 4).look(2).selected);

        follow_page(&ctx, Some(&page("b")));
        assert!(
            !List::open(&ctx, id, 4).look(2).selected,
            "a page change forgets every selection"
        );
    }

    #[test]
    fn from_queue_carries_the_selected_block_or_just_the_lifted_row() {
        let mut queue = queue_of(6, 0);
        queue.press(2, Press::Replace);
        queue.press(4, Press::Toggle);
        let block = Lift {
            index: 4,
            size: vec2(272.0, 57.0),
            ghost: lifted(),
        };
        let carry = from_queue(&mut queue, block).expect("a carry");
        assert_eq!(carry.source, Source::Queue);
        assert_eq!(carry.rows, vec![2, 4]);
        assert_eq!(carry.entries.len(), 2);
        assert_eq!(carry.size, vec2(272.0, 57.0));

        // Lifting a row that was not picked travels alone.
        let mut queue = queue_of(6, 0);
        let carry = from_queue(
            &mut queue,
            Lift {
                index: 3,
                size: vec2(272.0, 57.0),
                ghost: lifted(),
            },
        )
        .expect("a carry");
        assert_eq!(carry.rows, vec![3]);
        assert_eq!(carry.entries.len(), 1);

        // The current entry is never lifted.
        let mut queue = queue_of(6, 2);
        assert_eq!(
            from_queue(
                &mut queue,
                Lift {
                    index: 2,
                    size: vec2(272.0, 57.0),
                    ghost: lifted(),
                },
            ),
            None
        );
    }

    #[test]
    fn nothing_is_carried_after_a_release() {
        let ctx = context();
        lift(&ctx, carry(Source::Queue, vec![1]));
        assert!(carrying(&ctx));

        // egui clears the payload at the end of the pass that sees the release.
        let at = pos2(10.0, 10.0);
        for event in [
            Event::PointerMoved(at),
            Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
            Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ] {
            let mut output = ctx.run_ui(input(vec![event]), |_ui| {});
            output.textures_delta.clear();
        }

        assert!(!carrying(&ctx));
        assert!(carried(&ctx).is_none());
    }
}
