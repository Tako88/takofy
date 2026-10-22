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
    /// The one track this carry would add, when it is exactly one track.
    ///
    /// A block of two rows, an episode and a local file are all refused, so the
    /// caller never has to look at the entries to know what it holds.
    pub fn track(&self) -> Option<&TrackEntry> {
        if self.source == Source::Playlist || self.rows.len() != 1 || self.entries.len() != 1 {
            return None;
        }
        self.entries.first().filter(|entry| entry.is_track())
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

/// What a list posts a request with: the list it is, and the page it is on.
#[derive(Clone, Debug, PartialEq)]
pub struct Open<'a> {
    pub listing: Listing,
    pub here: Option<&'a Page>,
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
    context_menu::post(
        ctx,
        Request {
            door,
            items: verbs::track_items(&entries, here, &Listing::Queue),
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

/// Opens Now Playing's menu on the entry it is playing.
///
/// The current entry is the queue's, and it is never removable: the menu lists
/// what can be done with the track, not with a list it sits in.
pub fn current_menu(queue: &Queue, ctx: &Context, door: Door, gate: Gate, here: Option<&Page>) {
    if !gate.allows() {
        return;
    }
    let Some(entry) = queue.current().cloned() else {
        return;
    };
    let items = verbs::track_items(std::slice::from_ref(&entry), here, &Listing::Current);
    if items.is_empty() {
        return;
    }
    context_menu::post(
        ctx,
        Request {
            door,
            items,
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
        let items = verbs::track_items(&targets, open.here, &open.listing);
        context_menu::post(
            ctx,
            Request {
                door,
                items,
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
            playlist: None,
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

    fn door() -> Door {
        Door {
            anchor: crate::menu::Anchor::Button(Rect::from_min_size(
                pos2(0.0, 0.0),
                vec2(28.0, 28.0),
            )),
            opener: Id::unique("more"),
            device: crate::menu::Device::Pointer,
        }
    }

    fn entry_at(row: usize) -> Option<TrackEntry> {
        Some(entry(&row.to_string()))
    }

    fn opened() -> Open<'static> {
        Open {
            listing: Listing::Page,
            here: None,
        }
    }

    fn uris(request: &Request) -> Vec<&str> {
        request
            .targets
            .iter()
            .map(|entry| entry.uri.as_str())
            .collect()
    }

    #[test]
    fn menu_inside_a_pick_acts_on_the_pick_and_keeps_it() {
        let ctx = context();
        let id = Id::unique("list");
        let mut list = List::open(&ctx, id, 4);
        list.press(1, Press::Replace);
        list.press(3, Press::Toggle);
        list.menu(&ctx, door(), 3, Gate::default(), opened(), entry_at);
        list.finish(&ctx);

        let request = context_menu::take_request(&ctx).expect("a request was posted");
        assert_eq!(
            uris(&request),
            vec!["u1", "u3"],
            "the whole pick, ascending"
        );
        let kept = List::open(&ctx, id, 4);
        assert!(
            kept.look(1).selected && kept.look(3).selected,
            "the pick was collapsed"
        );
    }

    #[test]
    fn menu_outside_selects_first() {
        let ctx = context();
        let id = Id::unique("list");
        let mut list = List::open(&ctx, id, 4);
        list.press(1, Press::Replace);
        list.press(3, Press::Toggle);
        list.menu(&ctx, door(), 2, Gate::default(), opened(), entry_at);
        list.finish(&ctx);

        let request = context_menu::take_request(&ctx).expect("a request was posted");
        assert_eq!(uris(&request), vec!["u2"], "the row under the menu, alone");
        let kept = List::open(&ctx, id, 4);
        assert!(kept.look(2).selected, "the row is picked");
        assert!(!kept.look(1).selected, "and nothing else is");
    }

    #[test]
    fn menu_is_refused_while_carrying_or_writing_and_selects_nothing() {
        for gate in [
            Gate {
                carrying: true,
                writing: false,
            },
            Gate {
                carrying: false,
                writing: true,
            },
        ] {
            let ctx = context();
            let id = Id::unique("list");
            let mut list = List::open(&ctx, id, 4);
            list.press(1, Press::Replace);
            list.menu(&ctx, door(), 3, gate, opened(), entry_at);
            list.finish(&ctx);

            assert_eq!(context_menu::take_request(&ctx), None, "{gate:?}");
            let kept = List::open(&ctx, id, 4);
            assert!(kept.look(1).selected, "{gate:?}");
            assert!(
                !kept.look(3).selected,
                "{gate:?}: the refused open picked a row"
            );
        }
    }

    #[test]
    fn menu_refuses_a_pick_it_cannot_answer_for() {
        let ctx = context();
        let id = Id::unique("list");
        let mut list = List::open(&ctx, id, 4);
        list.press(1, Press::Replace);
        list.press(3, Press::Toggle);
        list.menu(&ctx, door(), 3, Gate::default(), opened(), |row| {
            (row != 3).then(|| entry(&row.to_string()))
        });
        list.finish(&ctx);

        assert_eq!(context_menu::take_request(&ctx), None);
        let kept = List::open(&ctx, id, 4);
        assert!(
            kept.look(1).selected && kept.look(3).selected,
            "a pick refused whole still changed the pick"
        );
    }

    #[test]
    fn a_posted_request_is_taken_once() {
        let ctx = context();
        let request = Request {
            door: door(),
            items: crate::verbs::track_items(&[entry("0")], None, &Listing::Page),
            targets: vec![entry("0")],
            rows: vec![0],
            listing: Listing::Page,
            page: Some(Page::Playlist("spotify:playlist:a".to_owned())),
        };
        context_menu::post(&ctx, request.clone());
        assert_eq!(context_menu::take_request(&ctx), Some(request));
        assert_eq!(
            context_menu::take_request(&ctx),
            None,
            "a request is read once"
        );
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
    fn a_column_offer_is_read_once() {
        let ctx = context();
        offer_column(
            &ctx,
            ColumnOffer {
                zone: zone(),
                drawn: extent::ROW_H,
            },
        );

        assert_eq!(
            take_column_offer(&ctx),
            Some(ColumnOffer {
                zone: zone(),
                drawn: extent::ROW_H,
            })
        );
        assert_eq!(take_column_offer(&ctx), None, "an offer is read once");
    }

    #[test]
    fn a_frame_that_offers_no_column_leaves_none_behind() {
        let ctx = context();
        offer_column(
            &ctx,
            ColumnOffer {
                zone: zone(),
                drawn: extent::ROW_H,
            },
        );
        let _ = take_column_offer(&ctx);

        assert_eq!(
            take_column_offer(&ctx),
            None,
            "a frame that offers nothing offers nothing"
        );
    }

    #[test]
    fn only_one_track_is_a_track_to_add() {
        let one = entry("x");
        let named = TrackEntry {
            uri: "spotify:track:4uLU6hMCjMI75M1A2tKUQC".to_owned(),
            ..one.clone()
        };

        let two_rows = Carry {
            entries: vec![named.clone()],
            rows: vec![1, 2],
            ..carry(Source::Queue, Vec::new())
        };
        assert_eq!(two_rows.track(), None, "a block of two rows");

        let no_entries = carry(Source::Queue, vec![3]);
        assert_eq!(no_entries.track(), None, "no entries travel");

        let playlist = Carry {
            rows: vec![1],
            playlist: Some("spotify:playlist:a".to_owned()),
            ..carry(Source::Playlist, Vec::new())
        };
        assert_eq!(playlist.track(), None, "a playlist is no track");

        let episode = Carry {
            rows: vec![1],
            entries: vec![TrackEntry {
                uri: "spotify:episode:1".to_owned(),
                ..named.clone()
            }],
            ..carry(Source::Queue, Vec::new())
        };
        assert_eq!(episode.track(), None, "an episode is no track");

        let track = Carry {
            rows: vec![1],
            entries: vec![named.clone()],
            ..carry(Source::Queue, Vec::new())
        };
        assert_eq!(track.track(), Some(&named));
    }

    #[test]
    fn select_rows_replaces_a_lists_selection() {
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

        select_rows(&ctx, first, 3..4);

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
        list.lift(4, vec2(10.0, 20.0), ghost(), |_| Some(entry("x")));
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

    /// A page whose every row still holds an entry named for its index.
    fn every_row(row: usize) -> Option<TrackEntry> {
        Some(entry(&row.to_string()))
    }

    #[test]
    fn lifting_a_picked_row_carries_the_whole_pick() {
        let ctx = context();
        let id = Id::unique("list");
        let mut list = List::open(&ctx, id, 6);
        list.press(4, Press::Replace);
        list.press(1, Press::Toggle);
        list.press(3, Press::Toggle);
        list.finish(&ctx);

        let mut list = List::open(&ctx, id, 6);
        list.lift(4, vec2(10.0, 20.0), ghost(), every_row);
        list.finish(&ctx);

        let carry = carried(&ctx).expect("a payload");
        assert_eq!(
            carry.rows,
            vec![1, 3, 4],
            "the whole pick travels, in order"
        );
        assert_eq!(
            carry.entries,
            vec![entry("1"), entry("3"), entry("4")],
            "each picked row brings its own entry"
        );
        let list = List::open(&ctx, id, 6);
        assert!(
            list.look(1).selected && list.look(3).selected && list.look(4).selected,
            "the pick is not collapsed by the lift"
        );
    }

    #[test]
    fn lifting_an_unpicked_row_carries_only_it() {
        let ctx = context();
        let id = Id::unique("list");
        let mut list = List::open(&ctx, id, 6);
        list.press(1, Press::Replace);
        list.press(3, Press::Toggle);
        list.finish(&ctx);

        let mut list = List::open(&ctx, id, 6);
        list.lift(5, vec2(10.0, 20.0), ghost(), every_row);
        list.finish(&ctx);

        let carry = carried(&ctx).expect("a payload");
        assert_eq!(carry.rows, vec![5]);
        assert_eq!(carry.entries, vec![entry("5")]);
        let list = List::open(&ctx, id, 6);
        assert!(
            !list.look(1).selected && !list.look(3).selected && list.look(5).selected,
            "a press on an unpicked row replaces the pick"
        );
    }

    #[test]
    fn a_lift_whose_entry_is_missing_carries_nothing() {
        let ctx = context();
        let id = Id::unique("list");
        let mut list = List::open(&ctx, id, 6);
        list.press(1, Press::Replace);
        list.press(4, Press::Toggle);
        list.finish(&ctx);

        let mut list = List::open(&ctx, id, 6);
        list.lift(4, vec2(10.0, 20.0), ghost(), |row| {
            (row == 1).then(|| entry("1"))
        });
        list.finish(&ctx);

        assert!(!carrying(&ctx), "a block with a missing row is no carry");
        let list = List::open(&ctx, id, 6);
        assert!(
            list.look(1).selected && list.look(4).selected,
            "the pick is unchanged"
        );
    }

    #[test]
    fn a_pick_beyond_the_count_is_not_carried() {
        let ctx = context();
        let id = Id::unique("list");
        let mut list = List::open(&ctx, id, 6);
        list.press(2, Press::Replace);
        list.press(5, Press::Toggle);
        list.finish(&ctx);

        let mut list = List::open(&ctx, id, 6);
        list.lift(2, vec2(10.0, 20.0), ghost(), |row| {
            (row < 3).then(|| entry(&row.to_string()))
        });
        list.finish(&ctx);

        assert!(
            !carrying(&ctx),
            "a picked row the list can no longer name is refused"
        );
        let list = List::open(&ctx, id, 6);
        assert!(list.look(2).selected && list.look(5).selected);
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
    #[test]
    fn request_rows_and_targets_stay_parallel() {
        let ctx = context();
        let id = Id::unique("list");
        let mut list = List::open(&ctx, id, 6);
        list.press(1, Press::Replace);
        list.press(3, Press::Toggle);
        list.press(4, Press::Toggle);
        list.menu(
            &ctx,
            door(),
            3,
            Gate::default(),
            Open {
                listing: Listing::Queue,
                here: None,
            },
            entry_at,
        );
        list.finish(&ctx);

        let request = context_menu::take_request(&ctx).expect("a request was posted");
        assert_eq!(request.rows, vec![1, 3, 4]);
        assert_eq!(uris(&request), vec!["u1", "u3", "u4"]);
        assert_eq!(request.listing, Listing::Queue);
    }

    #[test]
    fn forget_rows_shifts_the_lists_selection() {
        let ctx = context();
        let id = Id::unique("list");
        let mut list = List::open(&ctx, id, 6);
        list.press(1, Press::Replace);
        list.press(3, Press::Toggle);
        list.press(5, Press::Toggle);
        list.finish(&ctx);

        forget_rows(&ctx, id, &[1, 3]);

        let list = List::open(&ctx, id, 6);
        assert!(
            list.look(3).selected,
            "the row below the pick did not shift"
        );
        assert!(!list.look(1).selected && !list.look(5).selected);
    }
    #[test]
    fn queue_menu_posts_a_queue_request_and_keeps_the_pick() {
        let ctx = context();
        let mut queue = queue_of(6, 0);
        queue.press(2, Press::Replace);
        queue.press(4, Press::Toggle);

        queue_menu(&mut queue, &ctx, door(), 2, Gate::default(), None);

        let request = context_menu::take_request(&ctx).expect("a request was posted");
        assert_eq!(request.rows, vec![2, 4]);
        assert_eq!(uris(&request), vec!["u2", "u4"]);
        assert_eq!(request.listing, Listing::Queue);
        assert_eq!(
            request.items.last().map(|item| item.verb),
            Some(crate::menu::Verb::Remove)
        );
        assert!(
            request
                .items
                .iter()
                .all(|item| item.verb != crate::menu::Verb::AddToQueue),
            "the queue offered to add what is already there"
        );
        assert_eq!(
            queue.acting_rows(None),
            vec![2, 4],
            "the pick was collapsed"
        );
    }

    #[test]
    fn queue_menu_is_refused_by_the_gate_and_changes_no_pick() {
        for gate in [
            Gate {
                carrying: true,
                writing: false,
            },
            Gate {
                carrying: false,
                writing: true,
            },
        ] {
            let ctx = context();
            let mut queue = queue_of(6, 0);
            queue.press(2, Press::Replace);

            queue_menu(&mut queue, &ctx, door(), 4, gate, None);

            assert_eq!(context_menu::take_request(&ctx), None, "{gate:?}");
            assert_eq!(queue.acting_rows(None), vec![2], "{gate:?}");
        }
    }

    #[test]
    fn queue_menu_on_the_current_entry_posts_nothing() {
        let ctx = context();
        let mut queue = queue_of(4, 1);

        queue_menu(&mut queue, &ctx, door(), 1, Gate::default(), None);

        assert_eq!(context_menu::take_request(&ctx), None);
    }

    #[test]
    fn queue_menu_on_a_pick_the_queue_has_passed_posts_nothing() {
        let ctx = context();
        let mut queue = queue_of(6, 1);
        queue.press(2, Press::Replace);
        let _ = queue.next(&mut Transport::default());

        queue_menu(&mut queue, &ctx, door(), 2, Gate::default(), None);

        assert_eq!(
            context_menu::take_request(&ctx),
            None,
            "a pick the queue has passed was offered"
        );
    }

    #[test]
    fn queue_cut_posts_the_pick_for_the_queue() {
        let ctx = context();
        let mut queue = queue_of(5, 0);
        queue.press(1, Press::Replace);
        queue.press(3, Press::Toggle);

        queue_cut(&mut queue, &ctx, 3, Gate::default());

        let cut = context_menu::take_cut(&ctx).expect("a cut was posted");
        assert_eq!(cut.listing, Listing::Queue);
        assert_eq!(cut.rows, vec![1, 3]);
        assert_eq!(cut.entries.len(), 2);
    }

    #[test]
    fn queue_cut_is_refused_by_the_gate() {
        let ctx = context();
        let mut queue = queue_of(4, 0);
        queue.press(1, Press::Replace);

        queue_cut(
            &mut queue,
            &ctx,
            2,
            Gate {
                carrying: false,
                writing: true,
            },
        );

        assert_eq!(context_menu::take_cut(&ctx), None);
        assert_eq!(queue.acting_rows(None), vec![1], "the pick changed");
    }
    /// A track with a page to open and a link to copy, which is an entry the
    /// menu has verbs for.
    fn named_entry(uri: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            album_uri: Some("spotify:album:a".to_owned()),
            artist_uris: vec!["spotify:artist:b".to_owned()],
            ..entry("named")
        }
    }

    #[test]
    fn current_menu_posts_the_playing_entry_with_no_removal() {
        let ctx = context();
        let mut transport = Transport::default();
        let named = named_entry("spotify:track:a");
        let mut queue = Queue::default();
        queue.start(std::slice::from_ref(&named), 0, &mut transport);

        current_menu(&queue, &ctx, door(), Gate::default(), None);

        let request = context_menu::take_request(&ctx).expect("a request was posted");
        assert_eq!(request.listing, Listing::Current);
        assert_eq!(request.targets, vec![named]);
        assert_eq!(request.rows.len(), request.targets.len(), "not parallel");
        assert!(
            request.items.iter().all(|item| {
                item.verb != crate::menu::Verb::AddToQueue && item.verb != crate::menu::Verb::Remove
            }),
            "the current entry offered a list command"
        );
        assert!(
            request
                .items
                .iter()
                .any(|item| item.verb == crate::menu::Verb::GoToAlbum)
        );
    }

    #[test]
    fn current_menu_with_no_current_entry_or_no_verbs_posts_nothing() {
        let ctx = context();
        current_menu(&Queue::default(), &ctx, door(), Gate::default(), None);
        assert_eq!(context_menu::take_request(&ctx), None, "an empty queue");

        let mut transport = Transport::default();
        let mut queue = Queue::default();
        queue.start(&[entry("0")], 0, &mut transport);
        current_menu(&queue, &ctx, door(), Gate::default(), None);
        assert_eq!(
            context_menu::take_request(&ctx),
            None,
            "an entry with no verbs"
        );
    }

    #[test]
    fn current_menu_is_refused_by_the_gate() {
        let ctx = context();
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        queue.start(&[named_entry("spotify:track:a")], 0, &mut transport);

        current_menu(
            &queue,
            &ctx,
            door(),
            Gate {
                carrying: true,
                writing: false,
            },
            None,
        );

        assert_eq!(context_menu::take_request(&ctx), None);
    }
}
