use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};
use takofy_model::TrackEntry;

use super::super::*;
use crate::layout::Table;
use crate::layout::actionbar::{Form, Parts, Placed, solve};
use crate::theme::{Tokens, fonts, style};
use crate::track_filter::TrackFilter;

pub(super) const VIEWPORT: Vec2 = Vec2::new(900.0, 400.0);
/// How much list there is above the row, so scrolling can pin it.
pub(super) const FILLER: f32 = 200.0;
/// The playlist the fixture's query belongs to.
const PLAYLIST: &str = "spotify:playlist:a";

pub(super) fn context() -> Context {
    let tokens = Tokens::DARK;
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&tokens));
    ctx
}

pub(super) fn input(size: Vec2, events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
        events,
        ..Default::default()
    }
}

/// The kind a playlist page has in a window of `size`.
pub(super) fn kind(size: Vec2) -> Kind {
    Kind::Playlist(Form::at(size.x, Table::Full))
}

/// The row the tests draw, named and saved unless a test says otherwise.
pub(super) fn bar(kind: Kind) -> Bar<'static> {
    Bar {
        title: Some("Late Night Drive"),
        playable: true,
        shuffle: true,
        more: Some(true),
        saved: Some(true),
        kind,
        text: None,
    }
}

/// Three rows a query can narrow, and the query state that narrows them.
pub(super) struct Rows {
    state: TrackFilter,
    entries: Vec<TrackEntry>,
}

impl Rows {
    pub(super) fn new() -> Rows {
        Rows {
            state: TrackFilter::default(),
            entries: (0..3)
                .map(|index| TrackEntry {
                    uri: format!("t{index}"),
                    title: format!("t{index}"),
                    artists: if index == 1 {
                        "Midnight Artist".to_owned()
                    } else {
                        "Vela Moreno".to_owned()
                    },
                    album: None,
                    album_uri: None,
                    artist_uris: Vec::new(),
                    artist_names: Vec::new(),
                    added_at: None,
                    duration_ms: None,
                    named: true,
                    cover: None,
                })
                .collect(),
        }
    }

    pub(super) fn narrowed() -> Rows {
        let mut rows = Rows::new();
        rows.state.edit(PLAYLIST).push_str("midnight");
        rows
    }

    pub(super) fn filter(&mut self) -> playlist_filter::Filter<'_> {
        playlist_filter::Filter {
            state: &mut self.state,
            playlist: PLAYLIST,
            entries: &self.entries,
        }
    }
}

/// One frame of the row.
pub(super) struct Frame {
    pub(super) reply: Reply,
    pub(super) output: egui::FullOutput,
    /// The slot the row holds in the page's flow.
    pub(super) flow: Rect,
}

impl Frame {
    /// The row's drawn box: the fill it paints when it pins, and its place in
    /// the flow when it did not.
    pub(super) fn rect(&self) -> Rect {
        fills(&self.output, Tokens::DARK.palette.surface)
            .into_iter()
            .find(|(_, rect)| (rect.height() - actionbar::STUCK_H).abs() < 0.01)
            .map_or(self.flow, |(_, rect)| rect)
    }
}

pub(super) fn frame_in(
    ctx: &Context,
    size: Vec2,
    offset: f32,
    events: Vec<Event>,
    filter: Option<playlist_filter::Filter<'_>>,
) -> Frame {
    frame_of(ctx, &bar(kind(size)), size, offset, events, filter)
}

pub(super) fn frame(ctx: &Context, offset: f32, events: Vec<Event>) -> Frame {
    frame_in(ctx, VIEWPORT, offset, events, None)
}

/// One frame of a row of the caller's own making.
pub(super) fn frame_of(
    ctx: &Context,
    bar: &Bar<'_>,
    size: Vec2,
    offset: f32,
    events: Vec<Event>,
    filter: Option<playlist_filter::Filter<'_>>,
) -> Frame {
    frame_of_probed(ctx, bar, size, offset, events, filter, |_| ()).0
}

/// One frame of a row, plus whatever `probe` reads from the row's own `Ui`
/// once every widget is drawn.
pub(super) fn frame_of_probed<R>(
    ctx: &Context,
    bar: &Bar<'_>,
    size: Vec2,
    offset: f32,
    events: Vec<Event>,
    mut filter: Option<playlist_filter::Filter<'_>>,
    probe: impl FnOnce(&egui::Ui) -> R,
) -> (Frame, R) {
    let tokens = Tokens::DARK;
    let mut reply = Reply::quiet();
    let mut flow = Rect::ZERO;
    let mut read = None;
    let mut probe = Some(probe);
    let mut output = ctx.run_ui(input(size, events), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("bar")
            .vertical_scroll_offset(offset)
            .show(ui, |ui| {
                // The probe reads the slot off the cursor, so the gap the
                // layout puts between two widgets would be counted as part
                // of the row. The row places its own children absolutely and
                // never reads it.
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.add_space(FILLER);
                let top = ui.cursor().min;
                reply = show(ui, bar, filter.take(), &tokens);
                let bottom = ui.cursor().min.y;
                flow = Rect::from_min_size(top, Vec2::new(ui.available_width(), bottom - top.y));
                read = Some(probe.take().expect("the probe reads once")(ui));
            });
    });
    output.textures_delta.clear();
    (
        Frame {
            reply,
            output,
            flow,
        },
        read.expect("the row's frame ran"),
    )
}

/// One frame of a row, plus whatever `probe` reads from its `Ui`.
pub(super) fn frame_probed<R>(
    ctx: &Context,
    size: Vec2,
    offset: f32,
    events: Vec<Event>,
    filter: Option<playlist_filter::Filter<'_>>,
    probe: impl FnOnce(&egui::Ui) -> R,
) -> (Frame, R) {
    frame_of_probed(ctx, &bar(kind(size)), size, offset, events, filter, probe)
}

/// The row the artist page draws: a name, the tools, and the word the page
/// owns in the heart's place.
pub(super) fn text_bar(kind: Kind) -> Bar<'static> {
    Bar {
        title: Some("Vela Moreno"),
        playable: true,
        shuffle: true,
        more: Some(true),
        saved: None,
        kind,
        text: Some(Text {
            label: "Follow",
            live: true,
            failed: None,
        }),
    }
}

/// The rects the row's controls registered, read back from the frame that
/// drew them, so a test presses what the page actually laid out.
pub(super) struct Drawn {
    pub(super) play: Rect,
    pub(super) follow: Option<Rect>,
    pub(super) shuffle: Option<Rect>,
    pub(super) heart: Option<Rect>,
}

pub(super) fn drawn(ctx: &Context, bar: &Bar<'_>, size: Vec2, offset: f32) -> Drawn {
    let (_, ids) = frame_of_probed(ctx, bar, size, offset, Vec::new(), None, |ui| {
        [
            Control::HeroPlay,
            Control::Follow,
            Control::HeroShuffle,
            Control::HeroLike,
            Control::HeroMore,
        ]
        .map(|control| (control, controls::slot_id(ui, control, ())))
    });
    let rect = |control: Control| {
        ids.iter()
            .find(|(each, _)| *each == control)
            .and_then(|(_, id)| ctx.read_response(*id))
            .map(|response| response.rect)
    };
    Drawn {
        play: rect(Control::HeroPlay).expect("the row draws its play tile"),
        follow: rect(Control::Follow),
        shuffle: rect(Control::HeroShuffle),
        heart: rect(Control::HeroLike),
    }
}

/// The rects the frame filled with `fill`.
pub(super) fn fills(output: &egui::FullOutput, fill: egui::Color32) -> Vec<(usize, Rect)> {
    output
        .shapes
        .iter()
        .enumerate()
        .filter_map(|(at, clipped)| match &clipped.shape {
            egui::epaint::Shape::Rect(shape) if shape.fill == fill => Some((at, shape.rect)),
            _ => None,
        })
        .collect()
}

/// The place a text shape holds in the paint order, and its box.
pub(super) fn text_at(output: &egui::FullOutput, label: &str) -> Vec<(usize, Rect)> {
    output
        .shapes
        .iter()
        .enumerate()
        .filter_map(|(at, clipped)| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some((at, Rect::from_min_size(text.pos, text.galley.size())))
            }
            _ => None,
        })
        .collect()
}

/// The one box a text shape with `label` was painted in.
pub(super) fn text_rect(output: &egui::FullOutput, label: &str) -> Option<Rect> {
    text_at(output, label).first().map(|(_, rect)| *rect)
}

/// The row's children, solved from the row's own drawn box.
pub(super) fn placed(frame: &Frame, size: Vec2) -> Placed {
    solve(
        frame.rect(),
        kind(size),
        Parts {
            shuffle: true,
            field: true,
            follow: None,
        },
    )
}
