use std::time::Instant;

use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};

use super::*;
use crate::cover::Art;
use crate::home::counted::Tile;
use crate::library::Library;
use crate::route::Page;
use crate::theme::{Tokens, fonts, style};
use crate::view::playlist::root::MediaDraw;
use crate::view::test_support::click_events;

/// The probe window: wide enough for four shelf tiles.
const PROBE: Vec2 = Vec2::new(900.0, 600.0);

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

/// One pass over the shelf, and what its presses asked for.
fn run(ctx: &Context, events: Vec<Event>, tiles: &Tiles<'_>) -> (Option<Page>, egui::FullOutput) {
    let tokens = Tokens::DARK;
    let mut art = Art::default();
    let library = Library::default();
    let mut select = None;
    let mut open = None;
    let mut play = None;
    let mut out = Pressed::default();
    let mut output = ctx.run_ui(input(events), |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                crate::view::inner(ui, |ui| {
                    let mut draw = MediaDraw {
                        art: &mut art,
                        now: Instant::now(),
                        tokens: &tokens,
                        library: &library,
                        select: &mut select,
                        open: &mut open,
                        play: &mut play,
                    };
                    show(ui, tiles, 0, &tokens, &mut draw, &mut out);
                });
            });
    });
    output.textures_delta.clear();
    (open, output)
}

/// One click, in the passes egui needs to report it, and what it opened.
fn click(ctx: &Context, pos: Pos2, tiles: &Tiles<'_>) -> Option<Page> {
    let mut opened = None;
    for event in click_events(pos) {
        let (next, _) = run(ctx, vec![event], tiles);
        opened = opened.or(next);
    }
    opened
}

/// Where the shelf's `index`-th tile is laid out: its head, then the grid.
fn tile_rect_for(ctx: &Context, index: usize) -> Rect {
    let tokens = Tokens::DARK;
    let mut found = None;
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                crate::view::inner(ui, |ui| {
                    section::head(ui, crate::format::HOME_COUNTED_TITLE, "", &tokens);
                    found = Some(root::media_grid(ui).card_rect(index));
                });
            });
    });
    output.textures_delta.clear();
    found.expect("the shelf lays out")
}

/// The text one frame painted, in paint order.
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

/// Two artists, borrowed from a list rather than the library for the test.
fn tiles<'a>(artists: &'a [(&'a str, &'a str)]) -> Tiles<'a> {
    Tiles {
        artists: artists
            .iter()
            .map(|(uri, name)| Tile {
                uri,
                name,
                portrait: None,
            })
            .collect(),
    }
}

#[test]
fn the_head_names_the_shelf_and_the_tiles_follow_it() {
    let ctx = context();
    let artists = [("spotify:artist:a", "A"), ("spotify:artist:b", "B")];
    let (_, output) = run(&ctx, Vec::new(), &tiles(&artists));
    let drawn = painted(&output);

    assert_eq!(
        drawn.first(),
        Some(&crate::format::HOME_COUNTED_TITLE),
        "the shelf's title names what the order is"
    );
    assert!(drawn.contains(&"A"), "the first tile draws its name");
    assert!(drawn.contains(&"B"), "the second tile draws its name");
}

#[test]
fn a_press_opens_the_artist() {
    let ctx = context();
    let artists = [("spotify:artist:a", "A"), ("spotify:artist:b", "B")];
    let tiles = tiles(&artists);
    let at = tile_rect_for(&ctx, 1).center();

    assert_eq!(
        click(&ctx, at, &tiles),
        Some(Page::Artist("spotify:artist:b".to_owned()))
    );
}

#[test]
fn an_empty_shelf_draws_nothing() {
    let ctx = context();
    let (opened, output) = run(&ctx, Vec::new(), &tiles(&[]));
    assert!(painted(&output).is_empty(), "there is no head to draw");
    assert_eq!(opened, None);
}
