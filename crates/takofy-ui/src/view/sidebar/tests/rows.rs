use super::super::*;
use super::test_support::*;

use egui::{Color32, Event};

use crate::message::Update;

use crate::view::test_support::{click_events, rings_in};

/// One press of `key`, fresh or as a held key's auto-repeat.
///
/// egui decides the repeat against the pass before it, so `repeat: true` only
/// says "held" once the same key's press came in an earlier pass with no
/// release between them.
fn press(key: egui::Key, repeat: bool) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn a_clicked_rail_row_wears_no_ring_and_keeps_the_keyboard() {
    let ctx = context();
    let mut library = movable_playlists(&["A", "B"], 7, true, None);
    let mut art = Art::default();
    let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);
    let at = name_line(&output, "A");
    let _ = click(&ctx, at, &mut library, &mut art);

    let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);
    let focused = ctx
        .memory(|memory| memory.focused())
        .expect("the click took the keyboard");
    let row = ctx
        .read_response(focused)
        .expect("the row was laid out")
        .rect;
    assert!(
        row.contains(at),
        "the keyboard went to another row: {row:?}"
    );
    assert!(
        rings_in(&output).is_empty(),
        "a clicked rail row wears no ring: {:?}",
        rings_in(&output)
    );
}

#[test]
fn tab_rings_the_rail_row_inside_it() {
    let ctx = context();
    let mut library = movable_playlists(&["A", "B"], 7, true, None);
    let mut art = Art::default();
    let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);
    let at = name_line(&output, "A");
    let _ = click(&ctx, at, &mut library, &mut art);

    let _ = run(
        &ctx,
        vec![press(egui::Key::Tab, false)],
        &mut library,
        &mut art,
    );
    let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);

    let focused = ctx
        .memory(|memory| memory.focused())
        .expect("Tab put the keyboard somewhere");
    let row = ctx
        .read_response(focused)
        .expect("the row was laid out")
        .rect;
    let rings = rings_in(&output);
    assert_eq!(rings.len(), 1, "Tab rings one rail row: {rings:?}");
    assert_eq!(rings[0], row, "the ring lies on the row's own edge");
    assert!(
        rail().contains_rect(row),
        "the ringed row left the rail: {row:?}"
    );
}

#[test]
fn a_held_enter_on_the_focused_row_does_not_open_it_again() {
    let ctx = context();
    let mut library = movable_playlists(&["A", "B"], 7, true, None);
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    let at = name_line(&output, "A");
    let _ = click(&ctx, at, &mut library, &mut art);
    assert!(
        ctx.memory(|memory| memory.focused()).is_some(),
        "the row took no keyboard to hold Enter on"
    );

    // The first press is the user's, and opens the row once.
    let (_, fresh) = run_column(
        &ctx,
        vec![press(egui::Key::Enter, false)],
        &mut library,
        &mut art,
    );
    assert!(fresh.is_some(), "a first press of Enter opened nothing");

    // The repeats of the same held key are not presses of the user's.
    let (_, held) = run_column(
        &ctx,
        vec![press(egui::Key::Enter, true)],
        &mut library,
        &mut art,
    );
    assert_eq!(held, None, "a held Enter opened the row it just opened");
}

#[test]
fn a_playlist_row_click_loads_its_tracks() {
    let ctx = context();
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist(
        "spotify:playlist:a",
        "A list",
    )]));
    let mut art = Art::default();
    let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);

    let (name, name_size) = painted_text(&output, "A list");
    let (_, kind_size) = painted_text(&output, "Playlist");
    let command = click(
        &ctx,
        Pos2::new(name.x + 10.0, name.y + (name_size.y + kind_size.y) / 2.0),
        &mut library,
        &mut art,
    );

    assert_eq!(
        command,
        Some(Command::LoadTracks {
            playlist: "spotify:playlist:a".to_owned()
        })
    );
    assert_eq!(library.selected(), Some("spotify:playlist:a"));
}

/// The drawer draws after every region, so a frame an earlier region already
/// took has no room for the drawer's command. The row must be refused
/// *before* it arms its load: a page marked loading with no request behind it
/// never recovers, because `settle` sees the want and the in-flight read and
/// refuses to ask again.
#[test]
fn a_row_pick_is_refused_when_the_frame_cannot_take_its_command() {
    let ctx = context();
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist(
        "spotify:playlist:a",
        "A list",
    )]));
    let mut art = Art::default();
    let (output, _) = run_gated(&ctx, Vec::new(), &mut library, &mut art, false);

    let (name, name_size) = painted_text(&output, "A list");
    let (_, kind_size) = painted_text(&output, "Playlist");
    let at = Pos2::new(name.x + 10.0, name.y + (name_size.y + kind_size.y) / 2.0);
    let mut command = None;
    for event in click_events(at) {
        let (_, next) = run_gated(&ctx, vec![event], &mut library, &mut art, false);
        if next.is_some() {
            command = next;
        }
    }

    assert_eq!(command, None, "a command with no room was produced");
    assert_eq!(library.selected(), None, "the refused row still selected");
    assert_eq!(library.page(), None, "the refused row opened a page");
}

#[test]
fn a_playlist_name_starts_past_the_art_slot() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(254.0, 800.0))),
            ..Default::default()
        },
        |ui| {
            let rect = Rect::from_min_size(Pos2::new(20.0, 0.0), Vec2::new(200.0, ROW_H));
            let row = LibraryRow {
                id: PLAYLIST_IDS.id(ui, "spotify:playlist:a"),
                name: "A list",
                kind: "Playlist",
                cover: Cover::Missing,
                selected: false,
                lifted: false,
                edit: false,
            };
            one(ui, rect, row, &tokens, false, false, None);
        },
    );
    output.textures_delta.clear();

    let (name, _) = painted_text(&output, "A list");
    let (kind, _) = painted_text(&output, "Playlist");
    let expected = 20.0 + space::S2 + extent::LIBRARY_ART + space::S3;
    assert!((name.x - expected).abs() < 0.01, "name at {}", name.x);
    assert!((kind.x - expected).abs() < 0.01, "kind at {}", kind.x);
}

#[test]
fn a_playlist_row_announces_its_selection() {
    let ctx = context();
    ctx.enable_accesskit();
    let tokens = Tokens::DARK;
    let uri = "spotify:playlist:a";
    let mut announced = None;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(254.0, 800.0))),
            ..Default::default()
        },
        |ui| {
            let id = PLAYLIST_IDS.id(ui, uri);
            let rect = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(200.0, ROW_H));
            let row = LibraryRow {
                id,
                name: "A list",
                kind: "Playlist",
                cover: Cover::Missing,
                selected: true,
                lifted: false,
                edit: false,
            };
            one(ui, rect, row, &tokens, false, false, None);
            announced = ui
                .ctx()
                .accesskit_node_builder(id, |node| node.is_selected())
                .expect("the row has an AccessKit node");
        },
    );
    output.textures_delta.clear();

    assert_eq!(announced, Some(true), "a selected row says so");
}

/// The fills the row chrome paints with the pointer at `at`.
fn chrome_fills(at: Option<Pos2>) -> Vec<Color32> {
    let ctx = context();
    let tokens = Tokens::DARK;
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(254.0, ROW_H));
    let mut fills = Vec::new();
    for _ in 0..2 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(254.0, 800.0))),
                events: at.map(Event::PointerMoved).into_iter().collect(),
                ..Default::default()
            },
            |ui| {
                let id = egui::Id::unique("library-row");
                let look = Look {
                    selected: false,
                    ..Look::default()
                };
                let _ = row_chrome(ui, rect, id, "A", look, &tokens, false);
            },
        );
        output.textures_delta.clear();
        fills = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape) if shape.rect == rect => Some(shape.fill),
                _ => None,
            })
            .collect();
    }
    fills
}

#[test]
fn a_hovered_library_row_washes_with_row_hover() {
    let tokens = Tokens::DARK;
    assert_eq!(
        chrome_fills(Some(Pos2::new(120.0, ROW_H / 2.0))),
        vec![tokens.palette.row_hover],
        "a hovered library row washed with something other than the row hover"
    );
    assert!(
        chrome_fills(None).is_empty(),
        "a library row with the pointer away washed"
    );
}
