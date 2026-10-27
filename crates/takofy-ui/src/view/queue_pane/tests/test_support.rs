use egui::{Context, Event, Modifiers, Pos2, RawInput, Shape, Ui, Vec2};
use takofy_model::{CoverSet, CoverUrl};

use super::super::*;
use crate::cover::Cover;
use crate::player::Transport;
use crate::theme::{fonts, style};
use crate::view::carry::Ghost;

/// A pane wide enough for the reference's row and tall enough for five.
pub(super) const PANE: Rect = Rect {
    min: Pos2::new(0.0, 0.0),
    max: Pos2::new(272.0, 302.0),
};

pub(super) fn context() -> Context {
    let tokens = Tokens::DARK;
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&tokens));
    ctx
}

pub(super) fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 400.0))),
        events,
        ..Default::default()
    }
}

pub(super) fn run(ctx: &Context, input: RawInput, draw: impl FnMut(&mut Ui)) {
    let mut output = ctx.run_ui(input, draw);
    output.textures_delta.clear();
}

pub(super) fn drawn_texts(output: &egui::FullOutput) -> Vec<&str> {
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
pub(super) fn painted_at(output: &egui::FullOutput, label: &str) -> Option<Pos2> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(text) if text.galley.text() == label => Some(text.pos),
            _ => None,
        })
}

/// The rows whose title was painted, in queue order.
pub(super) fn painted_rows(output: &egui::FullOutput) -> Vec<usize> {
    drawn_texts(output)
        .into_iter()
        .filter_map(|text| text.strip_prefix('t'))
        .filter_map(|tail| tail.parse::<usize>().ok())
        .collect()
}

/// Puts a queue carry over `rows` in the frame's payload.
pub(super) fn carrying_rows(ctx: &Context, rows: Vec<usize>) {
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

pub(super) fn track(index: usize) -> TrackEntry {
    TrackEntry {
        uri: format!("t{index}"),
        title: format!("t{index}"),
        artists: format!("a{index}"),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: Some(1_000 * (index as u32 + 1)),
        named: true,
        cover: None,
    }
}

pub(super) fn track_with_cover(index: usize) -> TrackEntry {
    TrackEntry {
        cover: Some(CoverSet::single(
            CoverUrl::from_wire(&format!("https://i.scdn.co/image/c{index}")).expect("a cover url"),
        )),
        ..track(index)
    }
}

/// The row a cover URL names, from the tail the test wrote into the URL.
pub(super) fn cover_row(url: &CoverUrl) -> usize {
    url.as_str()
        .rsplit('c')
        .next()
        .and_then(|tail| tail.parse().ok())
        .expect("a cover URL names its row")
}

/// A queue of `count` tracks on `current`.
pub(super) fn queue_of(count: usize, current: usize) -> Queue {
    let tracks: Vec<TrackEntry> = (0..count).map(track).collect();
    let mut queue = Queue::default();
    queue.start(&tracks, current, None, &mut Transport::default());
    queue
}

/// One pass over the pane, returning the pane's answer and the frame.
pub(super) fn pass(
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
pub(super) fn feed(
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
            if next.play.is_some()
                || next.press.is_some()
                || next.open.is_some()
                || next.lift.is_some()
            {
                *reply = next;
            }
        });
    }
    reply
}

/// Clicks `at`, holding `modifiers`, and returns the reply that asked for
/// something.
pub(super) fn click_with(
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
pub(super) fn click(ctx: &Context, pane: Rect, queue: &Queue, at: Pos2) -> Reply {
    click_with(ctx, pane, queue, at, Modifiers::NONE, 1)
}

/// Runs `events` one pass each and keeps the reply that reported a door or a
/// key.
pub(super) fn feed_doors(
    ctx: &Context,
    pane: Rect,
    queue: &Queue,
    events: Vec<Event>,
    mut reply: Reply,
) -> Reply {
    let tokens = Tokens::DARK;
    let now = Instant::now();
    let mut art = Art::default();
    for event in events {
        art.begin_frame();
        let reply = &mut reply;
        run(ctx, input(vec![event]), |ui| {
            let next = show(ui, pane, queue, &mut art, now, &tokens);
            if next.menu.is_some() || next.remove.is_some() || next.press.is_some() {
                *reply = next;
            }
        });
    }
    reply
}

/// A queue row's position, from the row it painted.
pub(super) fn row_at(ctx: &Context, queue: &Queue, label: &str) -> Pos2 {
    let mut art = Art::default();
    let now = Instant::now();
    let (_, output) = pass(ctx, PANE, queue, &mut art, now, Vec::new());
    painted_at(&output, label).expect("the row is painted") + Vec2::new(4.0, 4.0)
}
