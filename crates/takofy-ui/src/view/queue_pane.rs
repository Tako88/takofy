//! The Queue tab's pane: the caption, the rows and the click it reports.
//!
//! The pane is the panel's only scroller, so the head, the art, the meta and the
//! tabs stay where they are while the list moves under them. It is handed the
//! queue by reference and returns the absolute index of a clicked row; applying
//! the jump is the caller's job, after the drawing is done, because the row loop
//! must not move the index it is iterating.
//!
//! Only the rows the viewport shows are laid out, and only those ask the art
//! store for a cover: a queue of a thousand entries looks at the dozen on screen.

use std::time::Instant;

use egui::{Align, Layout, Rect, ScrollArea, Sense, Ui, UiBuilder, pos2, vec2};
use takofy_model::TrackEntry;

use crate::cover::Art;
use crate::format::format_position;
use crate::layout::panel::CAPTION_H;
use crate::layout::queue::ROW_H;
use crate::layout::table;
use crate::queue::Queue;
use crate::theme::{Tokens, space};

use super::queue_row::{self, Row};
use super::text as recipe;

/// The caption over the list, as the reference writes it.
pub const CAPTION: &str = "Next in queue";

/// What an empty list says. The reference draws nothing at all, which cannot be
/// told apart from a pane that failed to draw.
pub const EMPTY: &str = "Nothing up next.";

/// Draws the queue pane inside `pane` and reports the absolute index a click
/// picked.
pub fn show(
    ui: &mut Ui,
    pane: Rect,
    queue: &Queue,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Option<usize> {
    if pane.width() <= 0.0 || pane.height() <= 0.0 {
        return None;
    }
    let mut clicked = None;

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

                    if count == 0 {
                        empty(ui, list, tokens);
                    } else {
                        for (row_index, (index, entry)) in queue.upcoming().enumerate() {
                            let rect = table::row_rect(list, row_index, ROW_H);
                            if !on_screen(visible, rect) {
                                continue;
                            }
                            if row(ui, rect, index, entry, art, now, tokens) {
                                clicked = Some(index);
                            }
                        }
                    }

                    // The reference gives the pane a bottom pad so the last row
                    // does not touch the panel's edge when the list is short.
                    ui.add_space(space::S4);
                });
        },
    );

    clicked
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

/// Draws one entry and reports a click on it.
fn row(
    ui: &mut Ui,
    rect: Rect,
    index: usize,
    entry: &TrackEntry,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> bool {
    let duration = entry.duration_ms.map(format_position);
    let row = Row {
        index,
        title: &entry.title,
        artist: &entry.artists,
        duration: duration.as_deref(),
        cover: art.look(entry.cover.as_ref(), now),
    };
    queue_row::show(ui, rect, &row, tokens)
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

    use egui::{Context, Event, Pos2, RawInput, Shape, Ui, Vec2};
    use takofy_model::{CoverUrl, TrackEntry};

    use super::*;
    use crate::cover::Cover;
    use crate::message::Command;
    use crate::player::Transport;
    use crate::theme::{fonts, style};

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
    ) -> (Option<usize>, egui::FullOutput) {
        let tokens = Tokens::DARK;
        let mut picked = None;
        art.begin_frame();
        let mut output = ctx.run_ui(input(events), |ui| {
            picked = show(ui, pane, queue, art, now, &tokens);
        });
        output.textures_delta.clear();
        (picked, output)
    }

    /// Clicks `at` over three passes, since egui's hit test reads the previous
    /// one, and returns the index the pane reported.
    fn click(ctx: &Context, pane: Rect, queue: &Queue, at: Pos2) -> Option<usize> {
        let tokens = Tokens::DARK;
        let now = Instant::now();
        let mut art = Art::default();
        let mut picked = None;
        run(ctx, input(Vec::new()), |ui| {
            let _ = show(ui, pane, queue, &mut art, now, &tokens);
        });
        for event in crate::view::test_support::click_events(at) {
            let picked = &mut picked;
            art.begin_frame();
            run(ctx, input(vec![event]), |ui| {
                if let Some(index) = show(ui, pane, queue, &mut art, now, &tokens) {
                    *picked = Some(index);
                }
            });
        }
        picked
    }

    #[test]
    fn an_empty_queue_draws_the_caption_and_the_empty_line() {
        let ctx = context();
        let mut art = Art::default();
        let now = Instant::now();
        let (picked, output) = pass(&ctx, PANE, &Queue::default(), &mut art, now, Vec::new());

        assert_eq!(picked, None);
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
        assert_eq!(
            click(&ctx, PANE, &queue, at + Vec2::new(4.0, 4.0)),
            Some(4),
            "the click must report the queue's own index, not the row's position"
        );
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
            click(&ctx, PANE, &queue, at + Vec2::new(4.0, 4.0)),
            Some(1),
            "the first of two identical entries is the one that answers"
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

        assert_eq!(click(&ctx, PANE, &queue, at), None);
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
            let (picked, output) = pass(&ctx, pane, &queue, &mut art, now, Vec::new());

            assert_eq!(picked, None, "{height}");
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
