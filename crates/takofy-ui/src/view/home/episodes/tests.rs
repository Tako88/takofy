use std::time::Instant;

use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};
use takofy_model::{Episode, PublishedAt};

use super::*;
use crate::cover::Art;
use crate::home::episodes::List;
use crate::library::Library;
use crate::route::Page;
use crate::theme::{Tokens, fonts, style};
use crate::view::playlist::root::MediaDraw;
use crate::view::test_support::click_events;

use super::super::Pressed;

/// A window tall enough for the head and the four rows.
const PROBE: Vec2 = Vec2::new(900.0, 700.0);
const NOW_MS: u64 = 1_700_000_000_000;
const DAY_MS: u64 = 86_400_000;

fn context() -> Context {
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, PROBE)),
        events,
        ..Default::default()
    }
}

fn episode(at: usize, days_ago: u64) -> Episode {
    Episode {
        uri: format!("spotify:episode:{at}"),
        title: format!("Episode {at}"),
        show: "Field Notes".to_owned(),
        show_uri: Some("spotify:show:field-notes".to_owned()),
        description: Some("A walk to the lighthouse.".to_owned()),
        published: PublishedAt::from_millis((NOW_MS - days_ago * DAY_MS) as i64),
        duration_ms: Some(2_773_461),
        cover: None,
        named: true,
    }
}

/// One pass over the shelf alone, with the press it took left in `open`.
fn draw(
    ctx: &Context,
    art: &mut Art,
    open: &mut Option<Page>,
    rows: &[&Episode],
    events: Vec<Event>,
) -> egui::FullOutput {
    let tokens = Tokens::DARK;
    let library = Library::default();
    let list = List {
        rows: rows.to_vec(),
    };
    let mut select = None;
    let mut play = None;
    let mut out = Pressed::default();
    let mut output = ctx.run_ui(input(events), |ui| {
        let mut draw = MediaDraw {
            art,
            now: Instant::now(),
            tokens: &tokens,
            library: &library,
            select: &mut select,
            open,
            play: &mut play,
        };
        show(ui, &list, NOW_MS, &tokens, &mut draw, &mut out);
    });
    output.textures_delta.clear();
    output
}

/// The text one frame actually painted, in paint order.
fn painted(output: &egui::FullOutput) -> Vec<&str> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect()
}

/// Where the galley whose text is exactly `label` was painted.
fn text_rect(output: &egui::FullOutput, label: &str) -> Option<Rect> {
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

#[test]
fn the_head_names_the_shelf_and_counts_nothing() {
    let ctx = context();
    let mut art = Art::default();
    let mut open = None;
    let one = episode(0, 1);
    let two = episode(1, 2);
    let rows = [&one, &two];

    let output = draw(&ctx, &mut art, &mut open, &rows, Vec::new());
    let painted = painted(&output);

    assert!(
        painted.contains(&HOME_EPISODES_TITLE),
        "the head did not name the shelf"
    );
    assert!(!painted.contains(&"2"), "the head counted the rows it drew");
    assert!(
        painted.contains(&"Episode 0") && painted.contains(&"Episode 1"),
        "a drawn row was missing: {painted:?}"
    );
}

#[test]
fn a_row_prints_how_long_ago_and_how_long() {
    let ctx = context();
    let mut art = Art::default();
    let mut open = None;
    let one = episode(0, 3);
    let rows = [&one];

    let output = draw(&ctx, &mut art, &mut open, &rows, Vec::new());
    let painted = painted(&output);

    assert!(
        painted.iter().any(|text| text.contains("3 DAYS AGO")),
        "the row did not say how long ago: {painted:?}"
    );
    assert!(
        painted.iter().any(|text| text.contains("46 MIN")),
        "the row did not say how long: {painted:?}"
    );
}

#[test]
fn a_row_byline_opens_its_show() {
    let ctx = context();
    let mut art = Art::default();
    let mut open = None;
    let one = episode(0, 1);
    let rows = [&one];

    let first = draw(&ctx, &mut art, &mut open, &rows, Vec::new());
    let pos = text_rect(&first, "FIELD NOTES")
        .expect("the byline paints the show's name")
        .center();

    let mut opened = None;
    for event in click_events(pos) {
        draw(&ctx, &mut art, &mut open, &rows, vec![event]);
        opened = opened.or(open.take());
    }

    assert_eq!(
        opened,
        Some(Page::Show("spotify:show:field-notes".to_owned()))
    );
}

#[test]
fn an_empty_list_draws_nothing() {
    let ctx = context();
    let mut art = Art::default();
    let mut open = None;

    let output = draw(&ctx, &mut art, &mut open, &[], Vec::new());

    assert!(
        painted(&output).is_empty(),
        "the shelf drew a head with no rows under it"
    );
}
