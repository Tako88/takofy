use std::time::Instant;

use egui::{Context, Event, Id, Pos2, RawInput, Rect, Vec2};
use takofy_model::{Episode, PublishedAt};

use super::*;
use crate::cover::Art;
use crate::home::episodes::List;
use crate::library::Library;
use crate::route::Page;
use crate::sidebar_kind::SidebarKind;
use crate::theme::{Tokens, extent, fonts, space, style};
use crate::view::carry;
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
    let mut play = None;
    draw_with(ctx, art, open, &mut play, rows, events)
}

/// One pass over the shelf alone, with the presses it took left in `open` and
/// `play`.
fn draw_with(
    ctx: &Context,
    art: &mut Art,
    open: &mut Option<Page>,
    play: &mut Option<(String, SidebarKind)>,
    rows: &[&Episode],
    events: Vec<Event>,
) -> egui::FullOutput {
    let tokens = Tokens::DARK;
    let library = Library::default();
    let list = List {
        rows: rows.to_vec(),
    };
    let mut select = None;
    let mut out = Pressed::default();
    let mut output = ctx.run_ui(input(events), |ui| {
        let mut draw = MediaDraw {
            art,
            now: Instant::now(),
            tokens: &tokens,
            library: &library,
            select: &mut select,
            open,
            play,
        };
        show(ui, &list, NOW_MS, &tokens, &mut draw, &mut out);
    });
    output.textures_delta.clear();
    output
}

/// Every text one frame painted, with the box it filled, in paint order.
fn words(output: &egui::FullOutput) -> Vec<(&str, Rect)> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => Some((
                text.galley.text(),
                Rect::from_min_size(text.pos, text.galley.size()),
            )),
            _ => None,
        })
        .collect()
}

/// The text one frame actually painted, in paint order.
fn painted(output: &egui::FullOutput) -> Vec<&str> {
    words(output).into_iter().map(|(text, _)| text).collect()
}

/// Where the galley whose text is exactly `label` was painted.
fn text_rect(output: &egui::FullOutput, label: &str) -> Option<Rect> {
    words(output)
        .into_iter()
        .find(|(text, _)| *text == label)
        .map(|(_, rect)| rect)
}

/// What the head painted on the title's own line, past the title's end.
fn aside<'a>(output: &'a egui::FullOutput, title: &str) -> Vec<&'a str> {
    let Some(head) = text_rect(output, title) else {
        return Vec::new();
    };
    words(output)
        .into_iter()
        .filter(|(text, rect)| {
            *text != title
                && rect.left() > head.right()
                && (rect.center().y - head.center().y).abs() < head.height() / 2.0
        })
        .map(|(text, _)| text)
        .collect()
}

/// Whether painted words read as a count of what the shelf drew.
fn looks_like_a_count(text: &str) -> bool {
    text.chars().any(|character| character.is_ascii_digit())
}

/// Every label the frame gave an accessibility node, in node order.
fn announced(output: &egui::FullOutput) -> Vec<String> {
    output
        .platform_output
        .accesskit_update
        .as_ref()
        .map(|update| {
            update
                .nodes
                .iter()
                .filter_map(|(_, node)| node.label().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// The centre of the square a play toggle would sit in, on the row whose title
/// is `title`.
fn toggle_centre(output: &egui::FullOutput, title: &str) -> Pos2 {
    let title = text_rect(output, title).expect("the row paints its title");
    let side = extent::EPISODE_TOGGLE;
    Pos2::new(
        title.left() - space::S4 - side / 2.0,
        title.top() + side / 2.0,
    )
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
    let aside = aside(&output, HOME_EPISODES_TITLE);
    assert!(
        !aside.iter().any(|text| looks_like_a_count(text)),
        "the head counted the rows it drew: {aside:?}"
    );
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

#[test]
fn a_press_where_the_toggle_would_be_plays_nothing() {
    let ctx = context();
    let mut art = Art::default();
    let one = episode(0, 1);
    let rows = [&one];

    let first = draw(&ctx, &mut art, &mut None, &rows, Vec::new());
    let at = toggle_centre(&first, "Episode 0");

    let mut open = None;
    let mut play = None;
    for event in click_events(at) {
        draw_with(&ctx, &mut art, &mut open, &mut play, &rows, vec![event]);
    }

    assert_eq!(
        play, None,
        "a row that only opens its show played from the toggle's place"
    );
    assert_eq!(open, None, "one press on the body is a pick, not an open");
    let picked = carry::List::open(&ctx, Id::unique("home-episodes"), rows.len())
        .look(0)
        .selected;
    assert!(picked, "the press was swallowed where the toggle would be");
}

#[test]
fn the_shelf_announces_no_play_it_cannot_give() {
    let ctx = context();
    ctx.enable_accesskit();
    let mut art = Art::default();
    let one = episode(0, 1);
    let rows = [&one];

    let output = draw(&ctx, &mut art, &mut None, &rows, Vec::new());
    let labels = announced(&output);

    assert!(
        !labels.iter().any(|label| label.starts_with("Play ")),
        "the shelf announced a play button: {labels:?}"
    );
    assert!(
        labels.iter().any(|label| label.contains("Episode 0")),
        "the row announced nothing: {labels:?}"
    );
}
