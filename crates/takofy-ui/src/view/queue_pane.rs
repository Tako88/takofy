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

use crate::cover::Art;
use crate::format::format_position;
use crate::landing::Zone;
use crate::layout::panel::CAPTION_H;
use crate::layout::queue::ROW_H;
use crate::layout::table;
use crate::queue::Queue;
use crate::scroll;
use crate::selection::Press;
use crate::theme::{Tokens, space};

use super::carry::{self, Carry, Source};
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
                        for (row_index, (index, entry)) in queue.upcoming().enumerate() {
                            let rect = table::row_rect(list, row_index, ROW_H);
                            if !on_screen(visible, rect) {
                                continue;
                            }
                            let look = standing(queue, carry.as_deref(), index);
                            match row(ui, rect, index, entry, queue, look, art, now, tokens) {
                                Some(Report::Play) => reply.play = Some(index),
                                Some(Report::Press(press)) => reply.press = Some((index, press)),
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

/// Whether any of `rect` is inside `visible`.
///
/// `Rect::intersects` counts a shared edge as an overlap, which would lay out the
/// row just past the fold and fetch its cover for nothing.
fn on_screen(visible: Rect, rect: Rect) -> bool {
    rect.left() < visible.right()
        && visible.left() < rect.right()
        && rect.top() < visible.bottom()
        && visible.top() < rect.bottom()
}

/// What drawing one row reported.
enum Report {
    /// The row was asked to play.
    Play,
    /// The row was picked.
    Press(Press),
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
        cover: art.look(entry.cover.as_ref(), now),
        look,
        refocus: queue.refocus() == Some(index),
    };
    match queue_row::show(ui, rect, &row, tokens) {
        Some(Click::Play) => Some(Report::Play),
        Some(Click::Select(press)) => Some(Report::Press(press)),
        Some(Click::Lift) => Some(Report::Lift(row.lifted())),
        Some(Click::Open(_)) | None => None,
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
mod tests {
    use std::collections::HashSet;

    use egui::{Context, DragAndDrop, Event, Modifiers, Pos2, RawInput, Shape, Ui, Vec2};
    use takofy_model::{CoverUrl, TrackEntry};

    use super::*;
    use crate::cover::Cover;
    use crate::landing::resolve;
    use crate::message::Command;
    use crate::player::Transport;
    use crate::theme::{fonts, opacity, style};
    use crate::view::carry::Ghost;

    /// A pane wide enough for the reference's row and tall enough for five.
    const PANE: Rect = Rect {
        min: Pos2::new(0.0, 0.0),
        max: Pos2::new(272.0, 302.0),
    };

    fn context() -> Context {
        let tokens = Tokens::DARK;
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));
        ctx
    }

    fn input(events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 400.0))),
            events,
            ..Default::default()
        }
    }

    fn timed(events: Vec<Event>, time: f64) -> RawInput {
        RawInput {
            time: Some(time),
            ..input(events)
        }
    }

    fn run(ctx: &Context, input: RawInput, draw: impl FnMut(&mut Ui)) {
        let mut output = ctx.run_ui(input, draw);
        output.textures_delta.clear();
    }

    fn drawn_texts(output: &egui::FullOutput) -> Vec<&str> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            })
            .collect()
    }

    /// Where a label was painted, so a click lands on the row the pane drew
    /// rather than on a coordinate a test guessed.
    fn painted_at(output: &egui::FullOutput, label: &str) -> Option<Pos2> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                Shape::Text(text) if text.galley.text() == label => Some(text.pos),
                _ => None,
            })
    }

    /// The rows whose title was painted, in queue order.
    fn painted_rows(output: &egui::FullOutput) -> Vec<usize> {
        drawn_texts(output)
            .into_iter()
            .filter_map(|text| text.strip_prefix('t'))
            .filter_map(|tail| tail.parse::<usize>().ok())
            .collect()
    }

    /// The alpha the first text painted as `label` was drawn at.
    fn painted_alpha(output: &egui::FullOutput, label: &str) -> Option<u8> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                Shape::Text(text) if text.galley.text() == label => Some(text.fallback_color.a()),
                _ => None,
            })
    }

    /// Puts a queue carry over `rows` in the frame's payload.
    fn carrying_rows(ctx: &Context, rows: Vec<usize>) {
        carry::lift(
            ctx,
            Carry {
                source: Source::Queue,
                entries: rows.iter().copied().map(track).collect(),
                rows,
                size: Vec2::new(PANE.width(), ROW_H),
                ghost: Ghost::Queue(queue_row::Lifted::new(
                    "carried".to_owned(),
                    String::new(),
                    None,
                    Cover::Missing,
                )),
                playlist: None,
            },
        );
    }

    fn track(index: usize) -> TrackEntry {
        TrackEntry {
            uri: format!("t{index}"),
            title: format!("t{index}"),
            artists: format!("a{index}"),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            added_at: None,
            duration_ms: Some(1_000 * (index as u32 + 1)),
            named: true,
            cover: None,
        }
    }

    fn track_with_cover(index: usize) -> TrackEntry {
        TrackEntry {
            cover: Some(
                CoverUrl::from_wire(&format!("https://i.scdn.co/image/c{index}"))
                    .expect("a cover url"),
            ),
            ..track(index)
        }
    }

    /// The row a cover URL names, from the tail the test wrote into the URL.
    fn cover_row(url: &CoverUrl) -> usize {
        url.as_str()
            .rsplit('c')
            .next()
            .and_then(|tail| tail.parse().ok())
            .expect("a cover URL names its row")
    }

    /// A queue of `count` tracks on `current`.
    fn queue_of(count: usize, current: usize) -> Queue {
        let tracks: Vec<TrackEntry> = (0..count).map(track).collect();
        let mut queue = Queue::default();
        queue.start(&tracks, current, &mut Transport::default());
        queue
    }

    /// One pass over the pane, returning the pane's answer and the frame.
    fn pass(
        ctx: &Context,
        pane: Rect,
        queue: &Queue,
        art: &mut Art,
        now: Instant,
        events: Vec<Event>,
    ) -> (Reply, egui::FullOutput) {
        let tokens = Tokens::DARK;
        let mut reply = Reply::default();
        art.begin_frame();
        let mut output = ctx.run_ui(input(events), |ui| {
            reply = show(ui, pane, queue, art, now, &tokens);
        });
        output.textures_delta.clear();
        (reply, output)
    }

    /// Runs `events` over the pane, one pass each, and keeps the reply that asked
    /// for something.
    fn feed(
        ctx: &Context,
        pane: Rect,
        queue: &Queue,
        mut art: Art,
        now: Instant,
        events: Vec<Event>,
        mut reply: Reply,
    ) -> Reply {
        let tokens = Tokens::DARK;
        for event in events {
            art.begin_frame();
            let reply = &mut reply;
            run(ctx, input(vec![event]), |ui| {
                let next = show(ui, pane, queue, &mut art, now, &tokens);
                if next.play.is_some() || next.press.is_some() || next.lift.is_some() {
                    *reply = next;
                }
            });
        }
        reply
    }

    /// Clicks `at`, holding `modifiers`, and returns the reply that asked for
    /// something.
    fn click_with(
        ctx: &Context,
        pane: Rect,
        queue: &Queue,
        at: Pos2,
        modifiers: Modifiers,
        rounds: usize,
    ) -> Reply {
        let tokens = Tokens::DARK;
        let now = Instant::now();
        let mut art = Art::default();
        run(ctx, input(Vec::new()), |ui| {
            let _ = show(ui, pane, queue, &mut art, now, &tokens);
        });

        let mut events = Vec::new();
        for _ in 0..rounds {
            if modifiers != Modifiers::NONE {
                events.push(Event::ModifiersChanged(modifiers));
            }
            events.extend(crate::view::test_support::click_events(at));
        }
        feed(ctx, pane, queue, art, now, events, Reply::default())
    }

    /// Clicks `at` over three passes, since egui's hit test reads the previous
    /// one, and returns the pane's reply.
    fn click(ctx: &Context, pane: Rect, queue: &Queue, at: Pos2) -> Reply {
        click_with(ctx, pane, queue, at, Modifiers::NONE, 1)
    }

    #[test]
    fn an_empty_queue_draws_the_caption_and_the_empty_line() {
        let ctx = context();
        let mut art = Art::default();
        let now = Instant::now();
        let (reply, output) = pass(&ctx, PANE, &Queue::default(), &mut art, now, Vec::new());

        assert_eq!(
            reply.zone.map(|zone| (zone.first, zone.count)),
            Some((0, 0))
        );
        assert_eq!(
            (reply.play, reply.press, reply.lift, reply.focused),
            (None, None, None, None)
        );
        let texts = drawn_texts(&output);
        assert!(texts.contains(&"NEXT IN QUEUE"), "{texts:?}");
        assert!(texts.contains(&EMPTY), "{texts:?}");
    }

    #[test]
    fn the_last_entry_draws_the_empty_line() {
        let ctx = context();
        let mut art = Art::default();
        let queue = queue_of(3, 2);
        let now = Instant::now();
        let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());

        let texts = drawn_texts(&output);
        assert!(texts.contains(&EMPTY), "{texts:?}");
        assert!(painted_rows(&output).is_empty(), "nothing follows it");
    }

    #[test]
    fn the_rows_are_drawn_in_order_starting_after_the_current_entry() {
        let ctx = context();
        let mut art = Art::default();
        let queue = queue_of(6, 1);
        let now = Instant::now();
        let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());

        assert_eq!(
            painted_rows(&output),
            vec![2, 3, 4, 5],
            "the pane must list exactly what follows the current entry"
        );
        assert!(
            !drawn_texts(&output).contains(&"t1"),
            "the current entry is the panel's own meta, not a row"
        );
    }

    #[test]
    fn a_click_reports_its_absolute_index() {
        let ctx = context();
        let mut art = Art::default();
        let queue = queue_of(6, 1);
        let now = Instant::now();
        let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());

        let at = painted_at(&output, "t4").expect("row t4 is painted");
        let reply = click(&ctx, PANE, &queue, at + Vec2::new(4.0, 4.0));
        assert_eq!(
            reply.press,
            Some((4, Press::Replace)),
            "the click must report the queue's own index, not the row's position"
        );
        assert_eq!(reply.play, None, "a single click is a press, not a play");
    }

    #[test]
    fn the_first_of_two_identical_entries_reports_its_own_index() {
        // Two entries with the same title and artist: keyed by anything but the
        // index the two rows would share one id, and the later one would win, so
        // clicking the first would report the second.
        let ctx = context();
        let mut queue = Queue::default();
        let same = track(0);
        let tracks = vec![track(1), same.clone(), same];
        queue.start(&tracks, 0, &mut Transport::default());

        let mut art = Art::default();
        let now = Instant::now();
        let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
        let at = painted_at(&output, "t0").expect("the first repeated row is painted");

        assert_eq!(
            click(&ctx, PANE, &queue, at + Vec2::new(4.0, 4.0)).press,
            Some((1, Press::Replace)),
            "the first of two identical entries is the one that answers"
        );
    }

    #[test]
    fn a_double_click_reports_the_row_to_play() {
        let ctx = context();
        let mut art = Art::default();
        let queue = queue_of(6, 1);
        let now = Instant::now();
        let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
        let at = painted_at(&output, "t4").expect("row t4 is painted");

        let reply = click_with(
            &ctx,
            PANE,
            &queue,
            at + Vec2::new(4.0, 4.0),
            Modifiers::NONE,
            2,
        );
        assert_eq!(reply.play, Some(4), "a double click asks for the row");
        assert_eq!(reply.press, None, "a double click is not a press");
    }

    #[test]
    fn a_double_click_starts_an_armed_queue() {
        let ctx = context();
        let mut art = Art::default();
        let mut armed = Queue::default();
        assert_eq!(armed.append((0..3).map(track).collect()), Some(0..3));
        let now = Instant::now();
        let (_, output) = pass(&ctx, PANE, &armed, &mut art, now, Vec::new());
        let at = painted_at(&output, "t1").expect("an armed row is painted");

        let reply = click_with(
            &ctx,
            PANE,
            &armed,
            at + Vec2::new(4.0, 4.0),
            Modifiers::NONE,
            2,
        );
        assert_eq!(
            reply.play,
            Some(1),
            "a double click picks the row out of the waiting list"
        );
        assert_eq!(reply.press, None, "a double click is not a press");
    }

    #[test]
    fn an_armed_row_is_pickable_without_starting_the_queue() {
        let ctx = context();
        let mut art = Art::default();
        let mut armed = Queue::default();
        assert_eq!(armed.append((0..3).map(track).collect()), Some(0..3));
        let now = Instant::now();
        let (_, output) = pass(&ctx, PANE, &armed, &mut art, now, Vec::new());
        let at = painted_at(&output, "t2").expect("an armed row is painted");

        let reply = click(&ctx, PANE, &armed, at + Vec2::new(4.0, 4.0));
        assert_eq!(reply.press, Some((2, Press::Replace)));
        assert_eq!(reply.play, None, "one click only picks the row");
    }

    #[test]
    fn shift_and_ctrl_clicks_report_their_press() {
        for (modifiers, press) in [
            (
                Modifiers {
                    shift: true,
                    ..Modifiers::NONE
                },
                Press::Extend,
            ),
            (
                Modifiers {
                    command: true,
                    ..Modifiers::NONE
                },
                Press::Toggle,
            ),
        ] {
            let ctx = context();
            let mut art = Art::default();
            let queue = queue_of(6, 0);
            let now = Instant::now();
            let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
            let at = painted_at(&output, "t4").expect("row t4 is painted");

            assert_eq!(
                click_with(&ctx, PANE, &queue, at + Vec2::new(4.0, 4.0), modifiers, 1).press,
                Some((4, press)),
                "{modifiers:?}"
            );
        }
    }

    #[test]
    fn a_seven_point_drag_reports_a_lift_with_the_row_size_and_its_ghost() {
        let ctx = context();
        let mut art = Art::default();
        let queue = queue_of(4, 0);
        let now = Instant::now();
        let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
        let at = painted_at(&output, "t3").expect("row t3 is painted");
        let start = at + Vec2::new(4.0, 4.0);

        let mut art = Art::default();
        run(&ctx, input(Vec::new()), |ui| {
            let _ = show(ui, PANE, &queue, &mut art, now, &Tokens::DARK);
        });
        let end = start + Vec2::new(7.0, 0.0);
        let mut lift = None;
        for event in [
            Event::PointerMoved(start),
            crate::view::test_support::button(start, true),
            Event::PointerMoved(end),
            crate::view::test_support::button(end, false),
        ] {
            let lift = &mut lift;
            art.begin_frame();
            run(&ctx, input(vec![event]), |ui| {
                let reply = show(ui, PANE, &queue, &mut art, now, &Tokens::DARK);
                if let Some(next) = reply.lift {
                    *lift = Some(next);
                }
            });
        }

        let lift = lift.expect("a seven-point drag lifts the row");
        assert_eq!(lift.index, 3);
        assert_eq!(
            lift.ghost,
            queue_row::Lifted::new(
                "t3".to_owned(),
                "a3".to_owned(),
                Some("0:04".to_owned()),
                Cover::Missing
            ),
            "the ghost is the row as it was drawn, cover look included"
        );
        assert_eq!(lift.size.y, ROW_H, "the ghost is one row tall");
        assert!(
            (lift.size.x - (PANE.width() - 2.0 * space::S3)).abs() < 0.5,
            "the ghost is the list's width: {:?}",
            lift.size
        );

        // A three-point twitch is a click, not a drag.
        let reply = click_with(
            &ctx,
            PANE,
            &queue,
            at + Vec2::new(4.0, 4.0),
            Modifiers::NONE,
            1,
        );
        assert_eq!(reply.lift, None, "a click does not lift the row");
    }

    #[test]
    fn the_zone_names_the_first_upcoming_row_and_the_count() {
        let ctx = context();
        let mut art = Art::default();
        let queue = queue_of(6, 1);
        let now = Instant::now();
        let (reply, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());

        let zone = reply.zone.expect("a non-empty queue is a target");
        assert_eq!(
            zone.first, 2,
            "the first upcoming row is the target's first"
        );
        assert_eq!(zone.count, 4, "only the upcoming rows are counted");
        assert_eq!(zone.row_h, ROW_H);
        let first = painted_at(&output, "t2").expect("row t2 is painted");
        assert!(
            zone.hit.contains(first),
            "the target must hold the row it names: {:?}",
            zone.hit
        );
    }

    #[test]
    fn the_pad_below_the_last_row_appends_to_the_end() {
        let ctx = context();
        let mut art = Art::default();
        let queue = queue_of(3, 0);
        let now = Instant::now();
        let (reply, _) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
        let zone = reply.zone.expect("a queue is a target");

        let at = Pos2::new(zone.list.center().x, zone.list.bottom() + space::S1);
        assert!(
            at.y < PANE.bottom() && at.y >= zone.list.bottom(),
            "the pointer must sit in the pad below the list: {at:?}"
        );
        let landing = resolve(Some(at), Some(&zone)).expect("the pad is part of the target");
        assert_eq!(
            landing.boundary,
            zone.first + zone.count,
            "a drop in the pad appends after the last row"
        );
    }

    #[test]
    fn an_empty_queue_is_a_target_that_takes_its_first_row() {
        let ctx = context();
        let mut art = Art::default();
        let now = Instant::now();
        let (reply, _) = pass(&ctx, PANE, &Queue::default(), &mut art, now, Vec::new());
        let zone = reply.zone.expect("an armed queue takes rows");

        assert_eq!((zone.first, zone.count), (0, 0), "there is no row yet");
        let at = Pos2::new(zone.list.center().x, PANE.center().y);
        let landing = resolve(Some(at), Some(&zone)).expect("the column is a target");
        assert_eq!(landing.boundary, 0, "the first row is the only place");
    }

    #[test]
    fn the_empty_column_below_a_short_list_appends() {
        let ctx = context();
        let mut art = Art::default();
        let queue = queue_of(2, 0);
        let now = Instant::now();
        let (reply, _) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
        let zone = reply.zone.expect("a queue is a target");

        let at = Pos2::new(zone.list.center().x, PANE.bottom() - space::S1);
        assert!(
            at.y > zone.list.bottom() + space::S4,
            "the pointer must sit well below the list and its pad: {at:?}"
        );
        let landing = resolve(Some(at), Some(&zone)).expect("the empty column is the target");
        assert_eq!(
            landing.boundary,
            zone.first + zone.count,
            "a drop in the empty column appends after the last row"
        );
    }

    #[test]
    fn the_focused_row_is_reported() {
        let ctx = context();
        let mut art = Art::default();
        let queue = queue_of(6, 0);
        let now = Instant::now();
        let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
        let at = painted_at(&output, "t3").expect("row t3 is painted");

        let reply = click(&ctx, PANE, &queue, at + Vec2::new(4.0, 4.0));
        assert_eq!(reply.focused, Some(3), "the picked row holds the keyboard");
    }

    #[test]
    fn a_focused_row_scrolled_out_of_view_is_still_reported() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let now = Instant::now();
        let mut art = Art::default();
        let mut queue = queue_of(40, 0);
        queue.refocus_at(5);
        let tall =
            Rect::from_min_size(Pos2::ZERO, Vec2::new(PANE.width(), CAPTION_H + 6.0 * ROW_H));
        let short =
            Rect::from_min_size(Pos2::ZERO, Vec2::new(PANE.width(), CAPTION_H + 2.0 * ROW_H));

        let (_, output) = pass(&ctx, tall, &queue, &mut art, now, Vec::new());
        assert!(
            painted_at(&output, "t5").is_some(),
            "the row asked for the keyboard must be drawn first"
        );

        // The pane shrinks until the focused row is below the fold. egui forgets a
        // widget it does not see twice, so the frame it leaves is the one pinned.
        let mut reply = Reply::default();
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            reply = show(ui, short, &queue, &mut art, now, &tokens);
        });
        output.textures_delta.clear();
        assert!(
            painted_rows(&output).iter().all(|row| *row < 5),
            "the focused row must be off screen: {:?}",
            painted_rows(&output)
        );
        assert_eq!(
            reply.focused,
            Some(5),
            "the keyboard stays on the row the pane no longer draws"
        );
    }

    #[test]
    fn enter_on_a_focused_row_reports_the_row_to_play() {
        let ctx = context();
        let mut art = Art::default();
        let queue = queue_of(6, 0);
        let now = Instant::now();
        let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
        let at = painted_at(&output, "t4").expect("row t4 is painted");

        // The click leaves the keyboard on the row it picked.
        let pressed = click(&ctx, PANE, &queue, at + Vec2::new(4.0, 4.0));
        assert_eq!(pressed.focused, Some(4));

        let enter = Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let (reply, _) = pass(&ctx, PANE, &queue, &mut art, now, vec![enter]);
        assert_eq!(reply.play, Some(4), "Enter plays the focused row");
        assert_eq!(reply.press, None, "Enter is not a press");
    }

    #[test]
    fn a_pending_refocus_takes_the_keyboard() {
        let ctx = context();
        let mut art = Art::default();
        let now = Instant::now();
        let mut queue = queue_of(6, 0);
        queue.refocus_at(3);

        // The frame that draws the row hands it the keyboard.
        let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
        assert!(
            painted_at(&output, "t3").is_some(),
            "the row asked for the keyboard must be drawn"
        );

        let enter = Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let (reply, _) = pass(&ctx, PANE, &queue, &mut art, now, vec![enter]);
        assert_eq!(
            reply.play,
            Some(3),
            "Enter plays the row the edit landed on"
        );
        assert_eq!(
            queue.refocus(),
            Some(3),
            "the pane reports and never mutates the queue"
        );
    }

    #[test]
    fn a_selected_row_wears_the_selection_fill() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut art = Art::default();
        let now = Instant::now();
        let mut queue = queue_of(4, 0);
        queue.press(2, Press::Replace);

        let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
        assert!(
            output.shapes.iter().any(|clipped| {
                matches!(&clipped.shape, Shape::Rect(shape) if shape.fill == tokens.palette.row_selected)
            }),
            "a picked row must wear the selection fill"
        );
    }

    #[test]
    fn a_carried_pane_scrolls_when_the_pointer_holds_its_bottom_edge() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let now = Instant::now();
        let mut art = Art::default();
        let queue = queue_of(40, 0);
        let at = Pos2::new(PANE.center().x, PANE.bottom() - 2.0);

        // The pointer arrives over the pane, then a block is carried.
        run(&ctx, timed(vec![Event::PointerMoved(at)], 1.0), |ui| {
            let _ = show(ui, PANE, &queue, &mut art, now, &tokens);
        });
        DragAndDrop::set_payload(&ctx, ());

        let mut first = Vec::new();
        let mut last = Vec::new();
        for frame in 0..12 {
            let mut output = ctx.run_ui(
                timed(vec![Event::PointerMoved(at)], 1.0 + 0.016 * frame as f64),
                |ui| {
                    let _ = show(ui, PANE, &queue, &mut art, now, &tokens);
                },
            );
            output.textures_delta.clear();
            if frame == 0 {
                first = painted_rows(&output);
            }
            last = painted_rows(&output);
        }

        assert!(
            !first.is_empty() && !last.is_empty(),
            "the pane must list rows: {first:?} {last:?}"
        );
        assert!(
            last[0] > first[0],
            "the carried pane must scroll under the pointer: {first:?} -> {last:?}"
        );
    }

    #[test]
    fn a_carried_pane_does_not_scroll_to_its_focused_row() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let now = Instant::now();
        let mut art = Art::default();
        // The sixth row hangs past the fold, so it is drawn and focused while
        // only part of it is on screen.
        let mut queue = queue_of(40, 0);
        queue.refocus_at(6);

        let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
        assert!(
            painted_at(&output, "t6").is_some(),
            "the focused row must be laid out"
        );
        let rest = painted_at(&output, "t1").expect("row t1 is painted");

        carrying_rows(&ctx, vec![6]);
        for frame in 0..8 {
            let mut output = ctx.run_ui(timed(Vec::new(), 1.0 + 0.016 * f64::from(frame)), |ui| {
                let _ = show(ui, PANE, &queue, &mut art, now, &tokens);
            });
            output.textures_delta.clear();
            assert_eq!(
                painted_at(&output, "t1"),
                Some(rest),
                "the pane scrolled to the focused row while carrying: frame {frame}"
            );
        }
    }

    #[test]
    fn a_carried_row_is_lifted_and_paints_dimmed() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let now = Instant::now();
        let mut art = Art::default();
        let queue = queue_of(6, 0);
        carrying_rows(&ctx, vec![3]);

        let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
        let payload = carry::carried(&ctx).expect("a carry");
        assert!(
            standing(&queue, Some(payload.as_ref()), 3).lifted,
            "the carried row is the lifted one"
        );
        assert!(
            standing(&queue, Some(payload.as_ref()), 4).carrying
                && !standing(&queue, Some(payload.as_ref()), 4).lifted,
            "the rest of the queue only knows a block is carried"
        );

        let ink = tokens.palette.text;
        assert_eq!(
            painted_alpha(&output, "t3"),
            Some(ink.gamma_multiply(opacity::LIFTED).a()),
            "the lifted row steps back"
        );
        assert_eq!(
            painted_alpha(&output, "t4"),
            Some(ink.a()),
            "the row beside it does not"
        );
    }

    #[test]
    fn a_row_that_only_touches_the_fold_is_not_laid_out() {
        // Two whole rows fit, so the third only shares the fold's edge. It is
        // not on screen, and a cover fetched for it would be a cover nothing
        // draws.
        let ctx = context();
        let tokens = Tokens::DARK;
        let now = Instant::now();
        let mut art = Art::default();
        let tracks: Vec<TrackEntry> = (0..20).map(track_with_cover).collect();
        let mut queue = Queue::default();
        queue.start(&tracks, 0, &mut Transport::default());

        let pane =
            Rect::from_min_size(Pos2::ZERO, Vec2::new(PANE.width(), CAPTION_H + 2.0 * ROW_H));
        art.begin_frame();
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = show(ui, pane, &queue, &mut art, now, &tokens);
        });
        output.textures_delta.clear();

        assert_eq!(painted_rows(&output), vec![1, 2]);
        let requested: HashSet<usize> = art
            .requests(now)
            .into_iter()
            .filter_map(|command| match command {
                Command::Fetch { url } => Some(cover_row(&url)),
                _ => None,
            })
            .collect();
        assert_eq!(requested, HashSet::from([1, 2]));
    }

    #[test]
    fn a_click_below_the_pane_picks_no_row() {
        // A culled row is by construction outside the pane's clip rect, so this
        // click is stopped by egui's hit testing and never reaches a row to
        // cull. That is the property pinned here; what pins the cull itself is
        // `a_row_that_only_touches_the_fold_is_not_laid_out`.
        let ctx = context();
        let queue = queue_of(40, 0);
        let at = Pos2::new(
            PANE.left() + space::S3 + 100.0,
            PANE.top() + CAPTION_H + 10.0 * ROW_H + ROW_H / 2.0,
        );
        assert!(
            at.y > PANE.bottom(),
            "the row under the click must be below the pane: {at:?}"
        );

        let reply = click(&ctx, PANE, &queue, at);
        assert_eq!(reply.press, None);
        assert_eq!(reply.play, None);
    }

    #[test]
    fn only_visible_rows_ask_for_a_cover() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let now = Instant::now();
        let mut art = Art::default();
        // Twenty entries with distinct covers in a pane showing two whole rows
        // and part of a third, fewer than the fetch cap: the requested set must
        // be exactly the visible entries' set, so a row laid out past the fold
        // shows up as an extra request instead of hiding under the cap.
        let tracks: Vec<TrackEntry> = (0..20).map(track_with_cover).collect();
        let mut queue = Queue::default();
        queue.start(&tracks, 0, &mut Transport::default());

        let pane = Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new(PANE.width(), CAPTION_H + 2.0 * ROW_H + ROW_H / 2.0),
        );
        art.begin_frame();
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = show(ui, pane, &queue, &mut art, now, &tokens);
        });
        output.textures_delta.clear();

        let visible: HashSet<usize> = painted_rows(&output).into_iter().collect();
        let requested: HashSet<usize> = art
            .requests(now)
            .into_iter()
            .filter_map(|command| match command {
                Command::Fetch { url } => Some(cover_row(&url)),
                _ => None,
            })
            .collect();

        assert_eq!(
            requested,
            HashSet::from([1, 2, 3]),
            "a row past the fold must not ask for a cover"
        );
        assert_eq!(
            visible,
            HashSet::from([1, 2, 3]),
            "the pane lays out exactly the rows that meet its fold"
        );
    }

    #[test]
    fn a_pane_too_short_for_a_row_or_with_no_height_still_draws() {
        let ctx = context();
        let now = Instant::now();
        let queue = queue_of(4, 0);

        // The caption alone no longer fits, then not even the pane.
        for height in [CAPTION_H / 2.0, 0.0] {
            let pane = Rect::from_min_size(Pos2::ZERO, Vec2::new(PANE.width(), height));
            let mut art = Art::default();
            let (reply, output) = pass(&ctx, pane, &queue, &mut art, now, Vec::new());

            assert_eq!(reply.play, None, "{height}");
            assert_eq!(reply.press, None, "{height}");
            assert!(
                painted_rows(&output).is_empty(),
                "no row fits a {height}-tall pane"
            );
            for clipped in &output.shapes {
                if let Shape::Rect(rect) = &clipped.shape {
                    assert!(
                        rect.rect.width() >= 0.0 && rect.rect.height() >= 0.0,
                        "an inverted rect in a {height}-tall pane: {:?}",
                        rect.rect
                    );
                }
            }
        }
    }

    #[test]
    fn a_row_hands_the_art_store_its_own_cover() {
        // A row with a cover it does not hold yet is `Loading`, not `Missing`,
        // and the answer is what puts the picture in the box.
        let ctx = context();
        let mut art = Art::default();
        let now = Instant::now();
        let mut queue = Queue::default();
        let tracks = vec![track(0), track_with_cover(1)];
        queue.start(&tracks, 0, &mut Transport::default());

        let _ = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
        let requested: Vec<CoverUrl> = art
            .requests(now)
            .into_iter()
            .filter_map(|command| match command {
                Command::Fetch { url } => Some(url),
                _ => None,
            })
            .collect();
        assert_eq!(requested.len(), 1, "only the row with a cover asks for one");
        assert_eq!(cover_row(&requested[0]), 1);
        assert_eq!(art.look(tracks[1].cover.as_ref(), now), Cover::Loading);
    }
}
