use egui::{Event, Modifiers};

use super::super::*;
use crate::theme::Tokens;
use crate::view::test_support::button;

pub(super) fn input_with(events: Vec<Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 800.0))),
        events,
        ..Default::default()
    }
}

pub(super) fn input() -> egui::RawInput {
    input_with(Vec::new())
}

/// The rect the pointer tests draw their row into.
pub(super) fn body_rect() -> Rect {
    crate::layout::table::row_rect(
        Rect::from_min_size(Pos2::ZERO, vec2(900.0, crate::theme::extent::ROW_H)),
        0,
        crate::theme::extent::ROW_H,
    )
}

pub(super) fn columns() -> Columns {
    crate::layout::table::columns(900.0, crate::layout::Table::Full.columns())
}

/// A playlist row with both link cells live and every column filled.
pub(super) fn row_of(number: usize) -> Row<'static> {
    Row {
        uri: "spotify:track:a",
        number,
        title: "Harbour Wall",
        artist: "Slow Tide",
        artist_uri: Some("spotify:artist:b"),
        album: Some("Record"),
        album_uri: Some("spotify:album:c"),
        added: Some("2 Jan 2024"),
        duration: Some("3:42"),
        state: State::Resting,
        cover: Some(Cover::Missing),
        look: Look::default(),
        menu: false,
        liked: None,
    }
}

/// A point on the row that is the row's own: the index column carries neither
/// a link nor a control.
pub(super) fn body_point() -> Pos2 {
    number_box(columns().at(body_rect().min).index).center()
}

/// Runs one pass per event, and reports what each pass answered.
pub(super) fn run(
    ctx: &egui::Context,
    row: &Row<'_>,
    events: &[Event],
    modifiers: Modifiers,
) -> Vec<Option<Click>> {
    run_in(ctx, body_rect(), columns(), row, events, modifiers)
}

/// The same pass sequence, against a row solved into `rect` with `columns`.
pub(super) fn run_in(
    ctx: &egui::Context,
    rect: Rect,
    columns: Columns,
    row: &Row<'_>,
    events: &[Event],
    modifiers: Modifiers,
) -> Vec<Option<Click>> {
    let tokens = Tokens::DARK;
    // Warm up, so hit testing has a previous pass to read. The held
    // modifiers arrive as an event and stay held for the passes after it.
    let mut output = ctx.run_ui(input_with(vec![Event::ModifiersChanged(modifiers)]), |ui| {
        let _ = show(ui, rect, columns, row, &tokens);
    });
    output.textures_delta.clear();

    let mut answers = Vec::new();
    for event in events {
        let mut answer = None;
        let mut output = ctx.run_ui(input_with(vec![event.clone()]), |ui| {
            answer = show(ui, rect, columns, row, &tokens);
        });
        output.textures_delta.clear();
        answers.push(answer);
    }
    answers
}

/// Runs one pass with the held modifiers, returning what it painted.
pub(super) fn paint_row(
    ctx: &egui::Context,
    row: &Row<'_>,
    events: Vec<Event>,
    modifiers: Modifiers,
) -> egui::FullOutput {
    let tokens = Tokens::DARK;
    let rect = body_rect();
    let columns = columns();
    let mut events = events;
    events.insert(0, Event::ModifiersChanged(modifiers));
    let mut output = ctx.run_ui(input_with(events), |ui| {
        crate::view::focus::begin_frame(ui.ctx());
        let _ = show(ui, rect, columns, row, &tokens);
    });
    output.textures_delta.clear();
    output
}

/// One press at `from` released `by` points away.
pub(super) fn twitch(from: Pos2, by: f32) -> Vec<Event> {
    let to = from + vec2(by, 0.0);
    vec![
        Event::PointerMoved(from),
        button(from, true),
        Event::PointerMoved(to),
        button(to, false),
    ]
}

pub(super) fn context() -> egui::Context {
    let tokens = Tokens::DARK;
    let ctx = egui::Context::default();
    ctx.set_fonts(crate::theme::fonts::definitions());
    ctx.set_global_style(crate::theme::style(&tokens));
    ctx
}

/// The rect a painted line occupies, read back from a frame.
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

/// The box the painted words of `label` occupy in a frame of `row`.
pub(super) fn words(ctx: &egui::Context, row: &Row<'_>, label: &str) -> Rect {
    let output = paint_row(ctx, row, Vec::new(), Modifiers::NONE);
    text_rect(&output, label).unwrap_or_else(|| panic!("{label:?} was not painted"))
}

/// A point between the words and their cell's right edge.
pub(super) fn space_after(cell: Rect, words: Rect) -> Pos2 {
    Pos2::new(
        words.right() + (cell.right() - words.right()) / 2.0,
        cell.center().y,
    )
}

/// One frame of the row, with the shapes it painted.
pub(super) fn frame(ctx: &egui::Context, row: &Row<'_>, events: Vec<Event>) -> egui::FullOutput {
    let tokens = Tokens::DARK;
    let mut output = ctx.run_ui(input_with(events), |ui| {
        let _ = show(ui, body_rect(), columns(), row, &tokens);
    });
    output.textures_delta.clear();
    output
}

/// The one door a run reported, if it reported any.
pub(super) fn door(answers: &[Option<Click>]) -> Option<Door> {
    answers.iter().flatten().find_map(|click| match click {
        Click::Menu(door) => Some(*door),
        _ => None,
    })
}

/// The actions column of the row, which every door anchors to.
pub(super) fn actions() -> Rect {
    columns().at(body_rect().min).actions
}

/// The colours of the glyphs painted inside `rect`.
pub(super) fn glyph_colors(output: &egui::FullOutput, rect: Rect) -> Vec<Color32> {
    output
        .shapes
        .iter()
        .filter(|clipped| clipped.shape.visual_bounding_rect().intersects(rect))
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Path(path) => Some(path.fill),
            _ => None,
        })
        .collect()
}
