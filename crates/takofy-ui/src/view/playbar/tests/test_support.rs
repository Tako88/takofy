//! Fixtures and probes shared by the playbar's tests.

use super::super::*;
use crate::theme::style;
use crate::view::test_support::click_events;
use egui::{Context, Event, Modifiers, RawInput};

pub(super) fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 200.0))),
        events,
        ..Default::default()
    }
}

/// Runs one pass and consumes the font-atlas delta it may have produced.
pub(super) fn run(ctx: &Context, input: RawInput, draw: impl FnMut(&mut Ui)) {
    let mut output = ctx.run_ui(input, draw);
    output.textures_delta.clear();
}

pub(super) fn key(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

pub(super) fn context(tokens: &Tokens) -> Context {
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(tokens));
    ctx
}

pub(super) fn playing(position_ms: u32) -> Transport {
    Transport {
        state: TransportState::Playing,
        position_ms,
        duration_ms: Some(222_000),
        ..Transport::default()
    }
}

/// One frame of the bar `width` wide, with the columns it solved.
pub(super) fn bar_at(
    ctx: &egui::Context,
    tokens: &Tokens,
    width: f32,
    form: Playbar,
    queue: &mut Queue,
) -> ([Rect; 3], egui::FullOutput) {
    bar_pass(ctx, tokens, width, form, queue, Vec::new(), 0.0)
}

/// One frame of the bar with `events` at `time`, so a hover can be read off
/// the fill it paints.
pub(super) fn bar_pass(
    ctx: &egui::Context,
    tokens: &Tokens,
    width: f32,
    form: Playbar,
    queue: &mut Queue,
    events: Vec<Event>,
    time: f64,
) -> ([Rect; 3], egui::FullOutput) {
    let bar = Rect::from_min_size(Pos2::new(0.0, 100.0), Vec2::new(width, extent::PLAYBAR_H));
    let cols = layout::playbar_for(bar, form).columns(bar);
    let mut transport = playing(0);
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 300.0))),
            events,
            time: Some(time),
            ..Default::default()
        },
        |ui| {
            let _ = show(
                ui,
                &mut transport,
                queue,
                0,
                form,
                Bar::default(),
                None,
                bar,
                Cover::Missing,
                tokens,
            );
        },
    );
    output.textures_delta.clear();
    (cols, output)
}

/// Where a painted word sits, by its own galley.
pub(super) fn painted_text(output: &egui::FullOutput, text: &str) -> Rect {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(shape) if shape.galley.text() == text => {
                Some(Rect::from_min_size(shape.pos, shape.galley.size()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{text} is not painted"))
}

/// Every label a pass gave a link node.
pub(super) fn link_labels(output: &egui::FullOutput) -> Vec<String> {
    output
        .platform_output
        .accesskit_update
        .as_ref()
        .map(|update| {
            update
                .nodes
                .iter()
                .filter_map(|(_, node)| {
                    (node.role() == egui::accesskit::Role::Link)
                        .then(|| node.label().map(str::to_owned))
                        .flatten()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One frame of a bar drawing `transport` and, when `remote` says so, the
/// holder's line.
pub(super) fn bar_of(
    ctx: &egui::Context,
    tokens: &Tokens,
    transport: &mut Transport,
    remote: Option<Remote<'_>>,
) -> egui::FullOutput {
    let (_, output) = remote_frame(
        ctx,
        tokens,
        transport,
        &mut Queue::default(),
        Playbar::Full,
        Bar::default(),
        remote,
        Vec::new(),
    );
    output
}

/// The album and credit a linked fixture names.
pub(super) const LINK_ALBUM: &str = "spotify:album:signal-decay";

pub(super) const LINK_ARTIST: &str = "spotify:artist:nils-aker";

/// A named track whose album and credit both have pages.
pub(super) fn linked_playing() -> Transport {
    let mut transport = named_playing();
    if let Some(now) = transport.now_playing.as_mut() {
        now.album_uri = Some(LINK_ALBUM.to_owned());
        now.artist_uri = Some(LINK_ARTIST.to_owned());
    }
    transport
}

/// The seek track a frame painted inside `column`.
pub(super) fn painted_track(output: &egui::FullOutput, column: Rect) -> Rect {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape)
                if (shape.rect.height() - extent::SLIDER_TRACK_H).abs() < 0.01
                    && shape.fill == Tokens::DARK.palette.border_strong
                    && column.contains(shape.rect.center()) =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
        .expect("the bar paints its seek track")
}

/// The bar rect the heart tests draw into: wide enough for the full form.
pub(super) fn heart_bar() -> Rect {
    Rect::from_min_size(Pos2::new(0.0, 100.0), Vec2::new(1200.0, extent::PLAYBAR_H))
}

/// One frame of the bar, with the playing track named.
pub(super) fn bar_frame(
    ctx: &egui::Context,
    tokens: &Tokens,
    transport: &mut Transport,
    form: Playbar,
    like: Option<bool>,
    events: Vec<Event>,
) -> (egui::FullOutput, [Rect; 3]) {
    let bar = heart_bar();
    let cols = layout::playbar_for(bar, form).columns(bar);
    let mut queue = Queue::default();
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 300.0))),
            events,
            ..Default::default()
        },
        |ui| {
            let _ = show(
                ui,
                transport,
                &mut queue,
                0,
                form,
                Bar {
                    like,
                    ..Bar::default()
                },
                None,
                bar,
                Cover::Missing,
                tokens,
            );
        },
    );
    output.textures_delta.clear();
    (output, cols)
}

pub(super) fn named_playing() -> Transport {
    let mut transport = playing(0);
    transport.now_playing = Some(crate::player::NowPlaying {
        uri: "spotify:track:x".to_owned(),
        title: "Signal Decay".to_owned(),
        artists: "Nils Aker".to_owned(),
        ..Default::default()
    });
    transport
}

pub(super) fn queue_of(count: usize) -> Queue {
    let tracks: Vec<takofy_model::TrackEntry> = (0..count)
        .map(|index| takofy_model::TrackEntry {
            uri: format!("t{index}"),
            title: format!("t{index}"),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            artist_names: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
        })
        .collect();
    let mut queue = Queue::seeded(5);
    queue.start(&tracks, 0, None, &mut Transport::default());
    queue
}

/// One frame of the bar over `queue`: what it reported and what it painted.
pub(super) fn queue_frame(
    ctx: &egui::Context,
    tokens: &Tokens,
    queue: &mut Queue,
    form: Playbar,
    bar: Bar,
    events: Vec<Event>,
) -> (Reply, egui::FullOutput) {
    let rect = heart_bar();
    let mut transport = playing(0);
    let mut reply = Reply::default();
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 300.0))),
            events,
            ..Default::default()
        },
        |ui| {
            reply = show(
                ui,
                &mut transport,
                queue,
                0,
                form,
                bar,
                None,
                rect,
                Cover::Missing,
                tokens,
            );
        },
    );
    output.textures_delta.clear();
    (reply, output)
}

/// Where one of the transport's buttons is drawn, solved here from the theme's
/// own constants. It must not call `transport_row`: a helper that did would
/// agree with the painter by construction and pin nothing.
pub(super) fn transport_slot(centre: Rect, control: Control) -> Rect {
    let stack = extent::BUTTON_LG + space::S1 + extent::SLIDER_H;
    let top = centre.center().y - stack / 2.0;
    let play = centre.center().x - extent::BUTTON_LG / 2.0;
    let start = play - (space::S1 + space::S2) - run_width(&DOWN);
    let up = play + extent::BUTTON_LG + space::S2 + space::S1;
    let at = |left: f32, index: usize| {
        Rect::from_min_size(
            Pos2::new(left + index as f32 * (extent::BUTTON + space::S1), top),
            Vec2::splat(extent::BUTTON),
        )
    };
    match control {
        Control::Shuffle => at(start, 0),
        Control::Previous => at(start, 1),
        Control::Next => at(up, 0),
        Control::Repeat => at(up, 1),
        other => panic!("{other:?} is not one of the transport's runs"),
    }
}

/// Clicks `at` over the passes egui's hit test needs, and reports every
/// frame.
pub(super) fn click_at(
    ctx: &egui::Context,
    tokens: &Tokens,
    queue: &mut Queue,
    form: Playbar,
    bar: Bar,
    at: Pos2,
) -> Vec<Reply> {
    let _ = queue_frame(ctx, tokens, queue, form, bar, Vec::new());
    let mut replies =
        vec![queue_frame(ctx, tokens, queue, form, bar, vec![Event::PointerMoved(at)]).0];
    for event in click_events(at) {
        replies.push(queue_frame(ctx, tokens, queue, form, bar, vec![event]).0);
    }
    replies
}

/// The colour a painted shape is drawn in, when it paints one.
pub(super) fn shape_colour(shape: &egui::Shape) -> Option<egui::Color32> {
    match shape {
        egui::Shape::Path(path) if path.fill != egui::Color32::TRANSPARENT => Some(path.fill),
        egui::Shape::Path(path) => match path.stroke.color {
            egui::epaint::ColorMode::Solid(color) => Some(color),
            egui::epaint::ColorMode::UV(_) => None,
        },
        egui::Shape::Circle(circle) => Some(circle.fill),
        egui::Shape::LineSegment { stroke, .. } => Some(stroke.color),
        _ => None,
    }
}

/// Whether anything inside `rect` is painted in the accent.
pub(super) fn accent_inside(output: &egui::FullOutput, tokens: &Tokens, rect: Rect) -> bool {
    output.shapes.iter().any(|clipped| {
        rect.contains(clipped.shape.visual_bounding_rect().center())
            && shape_colour(&clipped.shape) == Some(tokens.palette.accent)
    })
}

/// The text painted inside `rect`.
pub(super) fn text_inside(output: &egui::FullOutput, rect: Rect) -> Vec<String> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if rect.contains(text.pos) => {
                Some(text.galley.text().to_owned())
            }
            _ => None,
        })
        .collect()
}

/// Whether the node a frame announced with `label` reads as disabled.
pub(super) fn announced_disabled(output: &egui::FullOutput, label: &str) -> bool {
    output
        .platform_output
        .accesskit_update
        .as_ref()
        .and_then(|update| {
            update
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .map(|(_, node)| node.is_disabled())
        })
        .unwrap_or_else(|| panic!("{label} is not announced"))
}

/// One frame of the bar while another device holds the sound.
#[expect(
    clippy::too_many_arguments,
    reason = "one frame of the bar over a chosen transport, queue and state"
)]
pub(super) fn remote_frame(
    ctx: &egui::Context,
    tokens: &Tokens,
    transport: &mut Transport,
    queue: &mut Queue,
    form: Playbar,
    bar: Bar,
    remote: Option<Remote<'_>>,
    events: Vec<Event>,
) -> (Reply, egui::FullOutput) {
    let rect = heart_bar();
    let mut reply = Reply::default();
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 300.0))),
            events,
            ..Default::default()
        },
        |ui| {
            reply = show(
                ui,
                transport,
                queue,
                0,
                form,
                bar,
                remote,
                rect,
                Cover::Missing,
                tokens,
            );
        },
    );
    output.textures_delta.clear();
    (reply, output)
}

pub(super) fn remote(line: &str, steerable: bool) -> Option<Remote<'_>> {
    Some(Remote { line, steerable })
}
