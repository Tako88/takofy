//! The Queue tab's pane: the caption, the rows and what a frame asks for.
//!
//! The pane is the panel's only scroller, so the head, the art, the meta and the
//! tabs stay where they are while the list moves under them. It is handed the
//! queue by reference and never moves it: a play, a press and a lift are all
//! reported, and applying them is the caller's job after the drawing is done,
//! because the row loop must not move the index it is iterating.
//!
//! Only the rows the viewport shows are laid out, and only those ask the art
//! store for a cover: a queue of a thousand entries looks at the dozen on screen.
//!
//! The pane is the view's one drop target, so it also reports the zone a carried
//! block would land in. The zone is built here because only the pane
//! knows where its rows were solved and how far the list had scrolled.

use std::time::Instant;

use egui::{Align, Layout, Rect, ScrollArea, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use takofy_model::TrackEntry;

use crate::cover::{Art, want_px};
use crate::format::format_position;
use crate::landing::Zone;
use crate::layout::panel::CAPTION_H;
use crate::layout::queue::ROW_H;
use crate::layout::table;
use crate::menu::{Anchor, Device, Door};
use crate::queue::Queue;
use crate::route::Page;
use crate::scroll;
use crate::selection::Press;
use crate::theme::{Tokens, space};

use super::carry::{self, Carry, Source};
use super::context_menu;
use super::queue_row::{self, Row};
use super::rows::Look;
use super::text as recipe;
use super::track_row::Click;

/// The caption over the list, as the reference writes it.
pub const CAPTION: &str = "Next in queue";

/// What an empty list says. The reference draws nothing at all, which cannot be
/// told apart from a pane that failed to draw.
pub const EMPTY: &str = "Nothing up next.";

/// A row the pointer began carrying, and the ghost that follows it.
///
/// The pane builds this because the pane is the only side that holds the row's
/// snapshot, its cover look and its solved rect.
#[derive(Clone, Debug, PartialEq)]
pub struct Lift {
    /// The row the drag began on.
    pub index: usize,
    /// The rect the row was drawn in, which is the ghost's size.
    pub size: Vec2,
    /// The row as it is carried.
    pub ghost: queue_row::Lifted,
}

/// What a frame of the pane asks the shell to do.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    /// The row a double click or Enter asked to play.
    pub play: Option<usize>,
    /// The row a single click picked, and how.
    pub press: Option<(usize, Press)>,
    /// The row a drag began on.
    pub lift: Option<Lift>,
    /// The row that holds the keyboard.
    pub focused: Option<usize>,
    /// The page a click on a row's album or artist name asked for.
    pub open: Option<Page>,
    /// The door that opened the row's menu: the row, and where it anchors.
    pub menu: Option<(usize, Door)>,
    /// The row a Delete press acted on.
    pub remove: Option<usize>,
    /// The drop target the pane presents, where it presents one.
    pub zone: Option<Zone>,
}

/// Draws the queue pane inside `pane` and reports what it asked for.
pub fn show(
    ui: &mut Ui,
    pane: Rect,
    queue: &Queue,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Reply {
    let mut reply = Reply::default();
    if pane.width() <= 0.0 || pane.height() <= 0.0 {
        return reply;
    }
    let carry = carry::carried(ui.ctx());

    ui.scope_builder(
        UiBuilder::new()
            .id_salt("queue-pane")
            .max_rect(pane)
            .layout(Layout::top_down(Align::Min)),
        |ui| {
            ui.set_clip_rect(pane);
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // First, so this frame's rows are laid out at the offset the
                    // frame draws and a drop marker resolves against it.
                    scroll::follow_drag(ui);

                    let caption = recipe::caption(ui, CAPTION, tokens);
                    let band =
                        Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), CAPTION_H));
                    ui.allocate_rect(band, Sense::hover());
                    ui.painter().galley(
                        pos2(
                            band.left() + space::S2,
                            band.center().y - caption.size().y / 2.0,
                        ),
                        caption,
                        tokens.palette.text_subtle,
                    );

                    let count = queue.upcoming().len();
                    let list = Rect::from_min_size(
                        pos2(band.left() + space::S3, band.bottom()),
                        vec2(
                            (band.width() - 2.0 * space::S3).max(0.0),
                            ROW_H * count as f32,
                        ),
                    );
                    // The whole list is reserved at once so the scrollbar is
                    // right, while only the rows on screen are laid out.
                    ui.allocate_rect(list, Sense::hover());
                    let visible = ui.clip_rect();

                    // The caption band is part of the target and means the first
                    // upcoming row; the empty column below the list means
                    // appending to the end, which is where an armed queue takes
                    // the rows a drag brings it.
                    reply.zone = Some(Zone {
                        hit: Rect::from_min_max(band.min, pos2(band.right(), pane.bottom()))
                            .intersect(pane),
                        list,
                        row_h: ROW_H,
                        first: queue.first_upcoming(),
                        count,
                    });

                    if count == 0 {
                        empty(ui, list, tokens);
                    } else {
                        // Read before the rows draw, so the one that holds the
                        // keyboard can answer the menu key. The reply reports the
                        // row that holds it after the click, so that is read below.
                        let focused = focused_row(ui, queue);
                        for (row_index, (index, entry)) in queue.upcoming().enumerate() {
                            let rect = table::row_rect(list, row_index, ROW_H);
                            if !table::on_screen(visible, rect) {
                                continue;
                            }
                            // A queue row draws no `⋯`, so a right-click and the
                            // context-menu key are its only two doors, and both
                            // win over the row's own click.
                            let door = super::context_menu::pointer_door(
                                ui,
                                rect,
                                queue_row::id(ui, index),
                                rect,
                            )
                            .or_else(|| key_door(ui, index, focused, rect));
                            let look = standing(queue, carry.as_deref(), index);
                            let report = row(ui, rect, index, entry, queue, look, art, now, tokens);
                            if let Some(door) = door {
                                reply.menu = Some((index, door));
                                continue;
                            }
                            match report {
                                Some(Report::Play) => reply.play = Some(index),
                                Some(Report::Press(press)) => reply.press = Some((index, press)),
                                Some(Report::Open(page)) => reply.open = Some(page),
                                Some(Report::Lift(ghost)) => {
                                    reply.lift = Some(Lift {
                                        index,
                                        size: rect.size(),
                                        ghost,
                                    });
                                }
                                None => {}
                            }
                        }
                        // Del belongs to a focused row, and only while the menu
                        // is not holding the key: a consumed event is gone, so
                        // the menu's own Del would starve behind it.
                        if let Some(index) = focused
                            && !context_menu::is_open(ui.ctx())
                            && context_menu::take_delete(ui)
                        {
                            reply.remove = Some(index);
                        }
                        reply.focused = focused_row(ui, queue);
                    }

                    // The reference gives the pane a bottom pad so the last row
                    // does not touch the panel's edge when the list is short.
                    ui.add_space(space::S4);
                });
        },
    );

    reply
}

/// Where a row stands in the queue's selection and in the carry.
fn standing(queue: &Queue, carry: Option<&Carry>, index: usize) -> Look {
    Look {
        selected: queue.selected(index),
        lifted: carry.is_some_and(|carry| {
            matches!(carry.source, Source::Queue) && carry.rows.contains(&index)
        }),
        carrying: carry.is_some(),
    }
}

/// The row that holds the keyboard, read from egui's memory so a row the pane
/// has scrolled out of view still names itself.
fn focused_row(ui: &Ui, queue: &Queue) -> Option<usize> {
    let focused = ui.memory(|memory| memory.focused())?;
    let first = queue.first_upcoming();
    (first..first + queue.upcoming().len()).find(|index| queue_row::id(ui, *index) == focused)
}

/// The queue's keyboard door: the context-menu key or Shift+F10 on the focused
/// row, anchored under the row the way the `⋯` anchors elsewhere.
fn key_door(ui: &Ui, index: usize, focused: Option<usize>, anchor: Rect) -> Option<Door> {
    if focused != Some(index) {
        return None;
    }
    context_menu::menu_key_pressed(ui).then_some(Door {
        anchor: Anchor::Button(anchor),
        opener: queue_row::id(ui, index),
        device: Device::Keyboard,
    })
}

/// What drawing one row reported.
enum Report {
    /// The row was asked to play.
    Play,
    /// The row was picked.
    Press(Press),
    /// A click on the row's album or artist name named a page.
    Open(Page),
    /// A drag began on the row, with the snapshot it carries.
    Lift(queue_row::Lifted),
}

/// Draws one entry and reports what it asked for.
#[expect(
    clippy::too_many_arguments,
    reason = "the rect, the entry, the queue it stands in, and the frame's cover store"
)]
fn row(
    ui: &mut Ui,
    rect: Rect,
    index: usize,
    entry: &TrackEntry,
    queue: &Queue,
    look: Look,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Option<Report> {
    let duration = entry.duration_ms.map(format_position);
    let row = Row {
        index,
        title: &entry.title,
        artist: &entry.artists,
        duration: duration.as_deref(),
        cover: art.look(
            entry.cover.as_ref(),
            want_px(crate::theme::extent::QUEUE_ART, ui.pixels_per_point()),
            now,
        ),
        title_target: entry.album_uri.as_deref(),
        artist_target: entry.artist_uris.first().map(String::as_str),
        look,
        refocus: queue.refocus() == Some(index),
    };
    match queue_row::show(ui, rect, &row, tokens) {
        Some(Click::Play) => Some(Report::Play),
        Some(Click::Select(press)) => Some(Report::Press(press)),
        Some(Click::Open(page)) => Some(Report::Open(page)),
        Some(Click::Lift) => Some(Report::Lift(row.lifted())),
        // A queue row never asks for a menu: its `⋯` is not drawn, and it has no
        // heart.
        Some(Click::Menu(_)) | Some(Click::Like(_)) | None => None,
    }
}

/// The one muted line an empty list draws.
fn empty(ui: &Ui, list: Rect, tokens: &Tokens) {
    let line = recipe::line(
        ui,
        EMPTY,
        recipe::queue_artist(),
        tokens.palette.text_muted,
        0.0,
        list.width(),
    );
    ui.painter().galley(
        pos2(list.left(), list.top() + (ROW_H - line.size().y) / 2.0),
        line,
        tokens.palette.text_muted,
    );
}

#[cfg(test)]
mod tests;
