use std::time::Instant;

use egui::{Pos2, RawInput, Rect, Shape, vec2};
use takofy_model::{ArtistPortrait, CoverSet, CoverUrl, TrackEntry};

use super::*;
use crate::cover::Art;
use crate::message::{Failure, Update};
use crate::player::{NowPlaying, Transport, TransportState};
use crate::search::{KeyInput, Search};
use crate::search_index::{Kind, Row};
use crate::theme::{extent, fonts, style};

fn field() -> Rect {
    Rect::from_min_size(Pos2::new(40.0, 12.0), vec2(320.0, extent::SEARCH_H))
}

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx.enable_accesskit();
    ctx
}

fn track(index: usize) -> TrackEntry {
    TrackEntry {
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
    }
}

fn cover(tail: &str) -> CoverSet {
    CoverSet::single(
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url"),
    )
}

fn rich(index: usize, cover: Option<CoverSet>) -> TrackEntry {
    TrackEntry {
        uri: format!("t{index}"),
        title: format!("t{index}"),
        artists: format!("Artist{index}"),
        album: Some(format!("Album{index}")),
        album_uri: Some(format!("spotify:album:{index}")),
        artist_uris: vec![format!("spotify:artist:{index}")],
        artist_names: vec![format!("Artist{index}")],
        added_at: None,
        duration_ms: None,
        named: true,
        cover,
    }
}

fn open(search: &mut Search) {
    assert_eq!(
        search.step(KeyInput {
            focused: true,
            clicked_field: true,
            ..KeyInput::default()
        }),
        Act::None
    );
    assert!(search.is_open(), "the menu is drawn");
}

/// A search state with `count` plain tracks answered for "a", open the way
/// a click on the field opens it: the query kept and the menu open.
fn answered(count: usize) -> Search {
    let mut search = Search::default();
    search.text = "a".to_owned();
    search.text_changed();
    search.apply(&Update::Search {
        query: "a".to_owned(),
        tracks: (0..count).map(track).collect(),
    });
    open(&mut search);
    search
}

/// Three bands: 8 tracks, each on its own album and by its own artist.
fn banded() -> Search {
    let mut search = Search::default();
    search.text = "a".to_owned();
    search.text_changed();
    search.apply(&Update::Search {
        query: "a".to_owned(),
        tracks: (0..8).map(|index| rich(index, None)).collect(),
    });
    open(&mut search);
    search
}

fn failed() -> Search {
    let mut search = Search::default();
    search.text = "a".to_owned();
    search.text_changed();
    search.apply(&Update::SearchFailed {
        query: "a".to_owned(),
        failure: Failure::Search,
    });
    open(&mut search);
    search
}

/// Draws the menu twice — an `Area`'s first frame is its sizing pass — and
/// runs `read` inside the second pass, where egui keeps both the widget
/// rects and the AccessKit state.
fn draw_with<R>(
    ctx: &egui::Context,
    search: &mut Search,
    transport: &Transport,
    read: impl Fn(&egui::Ui, egui::Id, Menu) -> R,
) -> R {
    let tokens = Tokens::DARK;
    let mut art = Art::default();
    let now = Instant::now();
    let mut result = None;
    for pass in 0..2 {
        art.begin_frame();
        let mut output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                ..Default::default()
            },
            |ui| {
                let base = ui.make_persistent_id("search-menu-row");
                let menu = show(ui, field(), search, transport, &mut art, now, &tokens);
                if pass == 1 {
                    result = Some(read(ui, base, menu));
                }
            },
        );
        output.textures_delta.clear();
    }
    result.expect("two passes")
}

fn draw<R>(
    ctx: &egui::Context,
    search: &mut Search,
    read: impl Fn(&egui::Ui, egui::Id, Menu) -> R,
) -> R {
    draw_with(ctx, search, &Transport::default(), read)
}

/// Draws the menu to completion and returns the frame that painted it.
fn frame(
    ctx: &egui::Context,
    search: &mut Search,
    transport: &Transport,
) -> (egui::FullOutput, Menu) {
    let tokens = Tokens::DARK;
    let mut art = Art::default();
    let now = Instant::now();
    let mut second = None;
    for pass in 0..2 {
        art.begin_frame();
        let mut menu = None;
        let mut output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                ..Default::default()
            },
            |ui| {
                menu = Some(show(ui, field(), search, transport, &mut art, now, &tokens));
            },
        );
        output.textures_delta.clear();
        if pass == 1 {
            second = Some((output, menu.expect("the menu was drawn")));
        }
    }
    second.expect("two passes")
}

fn texts(output: &egui::FullOutput) -> Vec<String> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}

fn text_in(output: &egui::FullOutput, label: &str, menu: Rect) -> bool {
    output.shapes.iter().any(|clipped| {
        matches!(&clipped.shape, Shape::Text(text)
            if text.galley.text() == label && menu.contains_rect(text.visual_bounding_rect()))
    })
}

/// Finding 1: every state of the menu is one click-swallowing surface, so a
/// click on the empty line cannot fall through to a track row or a hero
/// button underneath it.
#[test]
fn every_state_of_the_menu_swallows_clicks() {
    for mut search in [answered(3), answered(0), failed()] {
        let ctx = context();
        let (takes_click, focusable, surface_rect, card) =
            draw(&ctx, &mut search, |ui, base, menu| {
                let surface = ui
                    .ctx()
                    .read_response(base.with("surface"))
                    .expect("the card is a widget");
                (
                    surface.sense.senses_click(),
                    surface.sense.is_focusable(),
                    surface.rect,
                    menu.rect.expect("the menu was drawn"),
                )
            });
        assert!(takes_click, "the card takes the click");
        assert!(!focusable, "and is no Tab stop");
        assert_eq!(surface_rect, card, "and covers exactly the card");
    }
}

#[test]
fn the_bands_are_drawn_in_order_with_their_headings_and_shares() {
    let ctx = context();
    let mut search = banded();
    let (output, menu) = frame(&ctx, &mut search, &Transport::default());
    let menu = menu.rect.expect("drawn");
    let labels = texts(&output);
    for label in [
        "TRACKS",
        "3 OF 8",
        "ALBUMS",
        "2 OF 8",
        "ARTISTS",
        "See more results for “a”",
    ] {
        assert!(
            text_in(&output, label, menu),
            "{label} is drawn in {labels:?}"
        );
    }
    let position = |label: &str| {
        labels
            .iter()
            .position(|found| found == label)
            .unwrap_or_else(|| panic!("{label} is drawn"))
    };
    assert!(position("TRACKS") < position("ALBUMS"));
    assert!(position("ALBUMS") < position("ARTISTS"));
}

#[test]
fn an_absent_band_is_not_drawn() {
    let ctx = context();
    let mut search = answered(3);
    let missing = draw(&ctx, &mut search, |ui, base, _menu| {
        let albums = ui
            .ctx()
            .read_response(base.with(("head", Kind::Albums)))
            .is_none();
        let artists = ui
            .ctx()
            .read_response(base.with(("head", Kind::Artists)))
            .is_none();
        (albums, artists)
    });
    assert_eq!(missing, (true, true), "an empty band has no heading node");
}

#[test]
fn a_floor_exists_exactly_when_a_band_is_capped() {
    for (count, floor) in [(3, false), (4, true)] {
        let ctx = context();
        let mut search = answered(count);
        let has_floor = draw(&ctx, &mut search, |ui, base, _menu| {
            ui.ctx().read_response(base.with("floor")).is_some()
        });
        assert_eq!(has_floor, floor, "an answer of {count}");
    }
}

#[test]
fn headings_are_labels_and_rows_are_named_buttons_and_never_tab_stops() {
    let ctx = context();
    let mut search = banded();
    let (heads, rows) = draw(&ctx, &mut search, |ui, base, _menu| {
        let mut heads = Vec::new();
        for kind in [Kind::Tracks, Kind::Albums, Kind::Artists] {
            let (role, value) = ui
                .ctx()
                .accesskit_node_builder(base.with(("head", kind)), |node| {
                    (node.role(), node.value().map(str::to_owned))
                })
                .expect("the heading has a node");
            heads.push((role, value));
        }
        let ids = [
            base.with(("track", 0)),
            base.with(("album", 0)),
            base.with(("artist", 0)),
            base.with("floor"),
        ];
        let rows = ids
            .into_iter()
            .map(|id| {
                let response = ui.ctx().read_response(id).expect("a row");
                let (role, label) = ui
                    .ctx()
                    .accesskit_node_builder(id, |node| {
                        (node.role(), node.label().map(str::to_owned))
                    })
                    .expect("the row has an AccessKit node");
                (response.sense, role, label)
            })
            .collect::<Vec<_>>();
        (heads, rows)
    });

    let expected = ["Tracks, 3 of 8", "Albums, 2 of 8", "Artists, 2 of 8"];
    for ((role, value), expected) in heads.into_iter().zip(expected) {
        assert_eq!(role, egui::Role::Label);
        assert_eq!(value.as_deref(), Some(expected));
    }
    let expected = [
        "t0 by Artist0",
        "Album0, album by Artist0",
        "Artist0, artist, 1 track",
        "See more results for “a”",
    ];
    for ((sense, role, label), expected) in rows.into_iter().zip(expected) {
        assert!(!sense.is_focusable(), "{expected} is not a Tab stop");
        assert!(sense.senses_click(), "{expected} takes a click");
        assert_eq!(role, egui::Role::Button);
        assert_eq!(label.as_deref(), Some(expected));
    }
}

fn marker_rects(output: &egui::FullOutput, menu: Rect, fill: egui::Color32) -> usize {
    output
        .shapes
        .iter()
        .filter(|clipped| match &clipped.shape {
            Shape::Rect(shape) => {
                shape.fill == fill
                    && (shape.rect.width() - stroke::RULE).abs() < 0.01
                    && (shape.rect.height() - extent::ROW_H).abs() < 0.01
                    && menu.contains_rect(shape.rect)
            }
            _ => false,
        })
        .count()
}

#[test]
fn only_one_row_wears_the_rule() {
    let ctx = context();
    let mut search = banded();
    assert_eq!(
        search.step(KeyInput {
            down: true,
            focused: true,
            ..KeyInput::default()
        }),
        Act::None
    );
    assert_eq!(search.marked(), Some(Row::Track(0)));
    let (output, menu) = frame(&ctx, &mut search, &Transport::default());
    assert_eq!(
        marker_rects(
            &output,
            menu.rect.expect("drawn"),
            Tokens::DARK.palette.text
        ),
        1,
        "only the marked row wears the rule"
    );
}

#[test]
fn every_band_and_the_floor_can_wear_the_rule() {
    let ctx = context();
    for ordinal in 0..8 {
        let mut search = banded();
        for _ in 0..=ordinal {
            search.step(KeyInput {
                down: true,
                focused: true,
                ..KeyInput::default()
            });
        }
        assert_eq!(
            search.mark(),
            Some(ordinal),
            "the walk reached ordinal {ordinal}"
        );
        let (output, menu) = frame(&ctx, &mut search, &Transport::default());
        assert_eq!(
            marker_rects(
                &output,
                menu.rect.expect("drawn"),
                Tokens::DARK.palette.text
            ),
            1,
            "ordinal {ordinal} is the only row wearing the rule"
        );
    }
}

#[test]
fn the_pointer_never_takes_the_rule_from_the_keyboard() {
    let ctx = context();
    let mut search = banded();
    search.step(KeyInput {
        down: true,
        focused: true,
        ..KeyInput::default()
    });
    search.pointer_moved();
    let (output, menu) = frame(&ctx, &mut search, &Transport::default());
    assert_eq!(
        marker_rects(
            &output,
            menu.rect.expect("drawn"),
            Tokens::DARK.palette.text
        ),
        1,
        "the mark keeps the rule: Enter fires the mark, so the rule must not move"
    );
}

#[test]
fn a_playing_track_keeps_the_accent_rule_while_marked() {
    let ctx = context();
    let mut search = banded();
    assert_eq!(
        search.step(KeyInput {
            down: true,
            focused: true,
            ..KeyInput::default()
        }),
        Act::None
    );
    let mut transport = Transport::default();
    transport.play(NowPlaying::named("t0", "t0", "Artist0"));
    transport.state = TransportState::Playing;
    let (output, menu) = frame(&ctx, &mut search, &transport);
    assert_eq!(
        marker_rects(
            &output,
            menu.rect.expect("drawn"),
            Tokens::DARK.palette.accent
        ),
        1,
        "identity beats focus"
    );
}

#[test]
fn a_known_portrait_is_what_the_drawn_artist_row_fetches() {
    let ctx = context();
    let mut search = banded();
    let portrait = cover("portrait");
    search.apply(&Update::Portraits {
        query: "a".to_owned(),
        portraits: vec![ArtistPortrait {
            artist: "spotify:artist:0".to_owned(),
            portrait: Some(portrait.clone()),
        }],
    });

    let tokens = Tokens::DARK;
    let mut art = Art::default();
    let now = Instant::now();
    let mut requested = Vec::new();
    for pass in 0..2 {
        art.begin_frame();
        let mut output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                ..Default::default()
            },
            |ui| {
                show(
                    ui,
                    field(),
                    &mut search,
                    &Transport::default(),
                    &mut art,
                    now,
                    &tokens,
                );
            },
        );
        output.textures_delta.clear();
        if pass == 1 {
            requested = art.requests(now);
        }
    }
    assert!(
        requested.contains(&crate::message::Command::Fetch {
            url: portrait.variants()[0].url.clone()
        }),
        "the drawn artist row asks for its portrait, not the record cover: {requested:?}"
    );
}

/// Finding 9: the empty line is a labelled node too, or the card reads as an
/// empty region. egui puts a `Role::Label`'s text in the node's value.
#[test]
fn the_empty_line_announces_why_the_card_is_empty() {
    let ctx = context();
    let mut search = answered(0);
    let (role, label) = draw(&ctx, &mut search, |ui, base, _menu| {
        ui.ctx()
            .accesskit_node_builder(base.with("line"), |node| {
                (node.role(), node.value().map(str::to_owned))
            })
            .expect("the line has an AccessKit node")
    });
    assert_eq!(role, egui::Role::Label);
    assert_eq!(label.as_deref(), Some("No matches for “a”"));
}
