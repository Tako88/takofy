use std::time::Instant;

use egui::{
    Context, Event, Modifiers, MouseWheelSource, MouseWheelUnit, Pos2, RawInput, Rect, TouchPhase,
    Vec2,
};
use takofy_model::{Episode, PublishedAt, Show, ShowDetail};

use super::super::*;
use crate::message::Update;
use crate::theme::{extent, fonts, style};
use crate::view::test_support::click_events;

pub(super) const SHOW: &str = "spotify:show:a";
pub(super) const NOW_MS: u64 = 1_761_134_400_000;
pub(super) const DAY_MS: u64 = 86_400_000;
/// The probe window: tall enough for the band, the head and about three rows.
pub(super) const PROBE: Vec2 = Vec2::new(900.0, 800.0);

pub(super) fn context() -> Context {
    let tokens = Tokens::DARK;
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&tokens));
    ctx
}

pub(super) fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, PROBE)),
        events,
        ..Default::default()
    }
}

pub(super) fn run(
    ctx: &Context,
    input: RawInput,
    draw: impl FnMut(&mut egui::Ui),
) -> egui::FullOutput {
    let mut output = ctx.run_ui(input, draw);
    output.textures_delta.clear();
    output
}

pub(super) fn drawn_texts(output: &egui::FullOutput) -> Vec<&str> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect()
}

pub(super) fn text_rect(output: &egui::FullOutput, label: &str) -> Option<Rect> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some(Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
}

pub(super) fn episode(n: usize) -> Episode {
    Episode {
        uri: format!("spotify:episode:{n}"),
        title: format!("Episode {n}"),
        show: "Lateral".to_owned(),
        show_uri: Some(SHOW.to_owned()),
        description: Some(format!("The long way round, part {n}.")),
        published: PublishedAt::from_millis((NOW_MS - 2 * DAY_MS) as i64),
        duration_ms: Some(2_773_461),
        cover: None,
        named: true,
    }
}

/// An episode published `days_ago` days before the probe's clock, so a
/// reordering has dates to read.
pub(super) fn dated(n: usize, days_ago: u64) -> Episode {
    Episode {
        published: PublishedAt::from_millis((NOW_MS - days_ago * DAY_MS) as i64),
        ..episode(n)
    }
}

pub(super) fn detail(episodes: Vec<Episode>) -> ShowDetail {
    ShowDetail {
        show: Show {
            uri: SHOW.to_owned(),
            name: "Lateral".to_owned(),
            publisher: "Tom Scott Media".to_owned(),
            description: "A show about the long way round.".to_owned(),
            cover: None,
        },
        episodes,
    }
}

pub(super) fn library_with(detail: ShowDetail) -> Library {
    let mut library = Library::default();
    let _ = library.open(Page::Show(SHOW.to_owned()));
    library.apply(&Update::Show(detail));
    library
}

pub(super) fn frame(
    ctx: &Context,
    events: Vec<Event>,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
) -> (Option<Command>, egui::FullOutput) {
    let tokens = Tokens::DARK;
    let mut command = None;
    let output = run(ctx, input(events), |ui| {
        // A notch lands in the frame it arrives, as the window's own draw
        // pass makes it: egui's spread of one notch over several frames
        // would still be moving the list while a probe reads a row's place.
        crate::scroll::instant(ui.ctx());
        if let Some(next) = show_view(
            ui,
            library,
            transport,
            queue,
            art,
            Instant::now(),
            NOW_MS,
            &tokens,
        ) {
            command = Some(next);
        }
    });
    (command, output)
}

/// One frame with the wheel moved by `points` at the foot of the list, which
/// is how the page scrolls.
pub(super) fn wheel(
    ctx: &Context,
    points: f32,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
) -> egui::FullOutput {
    let at = Pos2::new(PROBE.x / 2.0, PROBE.y - extent::EPISODE_ROW_H);
    frame(
        ctx,
        vec![
            Event::PointerMoved(at),
            Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta: Vec2::new(0.0, points),
                phase: TouchPhase::Move,
                source: MouseWheelSource::Unknown,
                modifiers: Modifiers::NONE,
            },
        ],
        library,
        transport,
        queue,
        art,
    )
    .1
}

/// Clicks `pos` once, one event per pass, and returns the last command.
pub(super) fn click(
    ctx: &Context,
    pos: Pos2,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
) -> Option<Command> {
    let mut last = None;
    for event in click_events(pos) {
        last = frame(ctx, vec![event], library, transport, queue, art).0;
    }
    last
}
