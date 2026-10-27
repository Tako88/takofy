use egui::{Context, Event, Id, Pos2, RawInput, Rect, Vec2};
use takofy_model::{Release, ReleaseKind, TrackEntry};

use super::*;
use crate::cover::Cover;
use crate::releases::Labels;
use crate::theme::{Tokens, fonts, style};
use crate::view::test_support::click_events;

fn context() -> Context {
    let tokens = Tokens::DARK;
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&tokens));
    ctx
}

fn release(count: usize) -> Release {
    Release {
        uri: "spotify:album:r1".to_owned(),
        name: "One".to_owned(),
        kind: ReleaseKind::Album,
        year: Some(2024),
        cover: None,
        tracks: (0..count)
            .map(|index| format!("spotify:track:{index}"))
            .collect(),
    }
}

/// The cursor's advance across one block, less the gap the parent adds
/// between its widgets: the block's own extent, measured at `width`.
fn advance(width: f32, count: usize) -> f32 {
    let ctx = context();
    let tokens = Tokens::DARK;
    let transport = Transport::default();
    let release = release(count);
    let likes = Likes::default();
    let block = Block {
        release: &release,
        labels: Labels::Idle,
        cover: Cover::Missing,
        on_screen: false,
        playable: false,
        here: None,
        writing: false,
        likes: &likes,
        saved: None,
    };

    let mut measured = None;
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 2000.0))),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| {
        ui.set_max_width(width);
        let top = ui.cursor().min.y;
        let _ = show(ui, Table::Full, &block, &transport, &tokens);
        measured = Some(ui.cursor().min.y - top - ui.spacing().item_spacing.y);
    });
    output.textures_delta.clear();
    measured.expect("the block lays out")
}

/// The centre of the one glyph painted in the accent, if any.
///
/// The icons are stroked, not filled, so the accent is the stroke's colour.
fn accent_centre(output: &egui::FullOutput, tokens: &Tokens) -> Option<Pos2> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Path(path)
                if matches!(
                    path.stroke.color,
                    egui::epaint::ColorMode::Solid(color) if color == tokens.palette.accent
                ) =>
            {
                Some(clipped.shape.visual_bounding_rect().center())
            }
            _ => None,
        })
}

/// One block drawn at a known saved state, as the frame painted it.
fn drawn(saved: bool, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = context();
    let tokens = Tokens::DARK;
    let transport = Transport::default();
    let release = release(0);
    let likes = Likes::default();
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 2000.0))),
            events,
            ..Default::default()
        },
        |ui| {
            let block = Block {
                release: &release,
                labels: Labels::Idle,
                cover: Cover::Missing,
                on_screen: true,
                playable: false,
                here: None,
                writing: false,
                likes: &likes,
                saved: Some(saved),
            };
            let _ = show(ui, Table::Full, &block, &transport, &tokens);
        },
    );
    output.textures_delta.clear();
    output
}

#[test]
fn a_saved_release_paints_its_heart_in_the_accent_and_an_unsaved_one_does_not() {
    let tokens = Tokens::DARK;
    let saved = drawn(true, Vec::new());
    assert!(
        accent_centre(&saved, &tokens).is_some(),
        "a saved release's heart is not filled"
    );

    let unsaved = drawn(false, Vec::new());
    assert!(
        accent_centre(&unsaved, &tokens).is_none(),
        "an unsaved release's heart took the accent"
    );
}

#[test]
fn a_press_on_the_blocks_heart_asks_for_the_other_state() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let at = accent_centre(&drawn(true, Vec::new()), &tokens).expect("the saved heart is painted");
    let transport = Transport::default();
    let release = release(0);
    let likes = Likes::default();
    let mut clicked = Clicked {
        played: false,
        saved: None,
        more: None,
        row: None,
        open: None,
    };

    let input = |events| RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 2000.0))),
        events,
        ..Default::default()
    };
    let mut warm = ctx.run_ui(input(Vec::new()), |_ui| {});
    warm.textures_delta.clear();
    let mut arrived = ctx.run_ui(input(vec![egui::Event::PointerMoved(at)]), |_ui| {});
    arrived.textures_delta.clear();
    for event in crate::view::test_support::click_events(at) {
        let mut output = ctx.run_ui(input(vec![event]), |ui| {
            let block = Block {
                release: &release,
                labels: Labels::Idle,
                cover: Cover::Missing,
                on_screen: true,
                playable: false,
                here: None,
                writing: false,
                likes: &likes,
                saved: Some(true),
            };
            clicked = show(ui, Table::Full, &block, &transport, &tokens);
        });
        output.textures_delta.clear();
    }

    assert_eq!(
        clicked.saved,
        Some(false),
        "a saved release's heart asks to take it out"
    );
}

/// The page culls with [`height`] and then draws at the cursor, so the two
/// numbers must be the same one or a block blanks before it leaves the top.
#[test]
fn the_cursor_advance_is_the_height_the_culling_uses() {
    for width in [420.0, 900.0] {
        assert_eq!(advance(width, 3), height(3), "three rows at {width} px");
        assert_eq!(advance(width, 0), height(0), "no rows at {width} px");
    }
}

fn labelled(uri: &str, title: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: title.to_owned(),
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

/// One frame of a labelled block, and what its rows reported.
fn painted(
    ctx: &Context,
    release: &Release,
    tracks: &[TrackEntry],
    events: Vec<Event>,
) -> (Clicked, egui::FullOutput) {
    let tokens = Tokens::DARK;
    let transport = Transport::default();
    let likes = Likes::default();
    let block = Block {
        release,
        labels: Labels::Ready(tracks),
        cover: Cover::Missing,
        on_screen: true,
        playable: true,
        here: None,
        writing: false,
        likes: &likes,
        saved: None,
    };
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 800.0))),
        events,
        ..Default::default()
    };
    let mut clicked = Clicked::default();
    let mut output = ctx.run_ui(input, |ui| {
        clicked = show(ui, Table::Full, &block, &transport, &tokens);
    });
    output.textures_delta.clear();
    (clicked, output)
}

/// The rect a painted label occupies, read back from the frame.
fn text_rect(output: &egui::FullOutput, label: &str) -> Option<Rect> {
    text_runs(output, label).first().copied()
}

/// Every run of a painted label, so a test can tell once from twice.
fn text_runs(output: &egui::FullOutput, label: &str) -> Vec<Rect> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some(Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
        .collect()
}

/// Every label a pass gave a link node.
fn link_labels(output: &egui::FullOutput) -> Vec<String> {
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

#[test]
fn the_release_name_is_a_link_to_its_album() {
    let ctx = context();
    let release = release(0);
    let (_, output) = painted(&ctx, &release, &[], Vec::new());
    let name = text_rect(&output, &release.name).expect("the release's name is painted");

    let mut clicked = Clicked::default();
    for event in click_events(name.center()) {
        clicked = painted(&ctx, &release, &[], vec![event]).0;
    }

    assert_eq!(
        clicked.open,
        Some(Page::Album(release.uri.clone())),
        "the release's name does not open its album"
    );
}

#[test]
fn a_click_beside_the_name_opens_nothing() {
    let ctx = context();
    let release = release(0);
    let (_, output) = painted(&ctx, &release, &[], Vec::new());
    let name = text_rect(&output, &release.name).expect("the release's name is painted");
    // The head is mostly empty between the name and the controls.
    let beside = pos2(name.right() + 20.0, name.center().y);

    let mut clicked = Clicked::default();
    for event in click_events(beside) {
        clicked = painted(&ctx, &release, &[], vec![event]).0;
    }

    assert_eq!(clicked.open, None, "the head opened a page beside the name");
    assert!(
        !clicked.played,
        "a press beside the name started the release"
    );
}

#[test]
fn the_head_lays_the_name_once_over_its_facts() {
    let ctx = context();
    let release = release(0);
    let (_, output) = painted(&ctx, &release, &[], Vec::new());

    let names = text_runs(&output, &release.name);
    assert_eq!(
        names.len(),
        1,
        "the head laid its name {} times",
        names.len()
    );
    let name = names[0];

    let facts = release_meta(release.kind, release.year, (release.tracks.len(), 0))
        .join(" · ")
        .to_uppercase();
    let facts = text_rect(&output, &facts).expect("the head paints its facts");

    // The head's own box, from its hairline, the art's height and the gap.
    let head = Rect::from_min_size(
        pos2(0.0, stroke::HAIRLINE + space::S5),
        vec2(900.0, extent::RELEASE_ART),
    );
    assert!(
        name.bottom() < facts.top(),
        "the name {name:?} sits on its facts {facts:?}"
    );
    for (what, rect) in [("name", name), ("facts", facts)] {
        assert!(
            head.contains_rect(rect),
            "the head's {what} {rect:?} leaves the head {head:?}"
        );
    }

    for width in [420.0, 900.0] {
        assert_eq!(advance(width, 2), height(2), "the head grew at {width} px");
    }
}

/// One pass of an idle block drawn inside `clip`.
fn clipped(ctx: &Context, release: &Release, clip: Rect) -> (Clicked, egui::FullOutput) {
    let tokens = Tokens::DARK;
    let transport = Transport::default();
    let likes = Likes::default();
    let block = Block {
        release,
        labels: Labels::Idle,
        cover: Cover::Missing,
        on_screen: false,
        playable: false,
        here: None,
        writing: false,
        likes: &likes,
        saved: None,
    };
    let mut clicked = Clicked::default();
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 2000.0))),
            ..Default::default()
        },
        |ui| {
            ui.set_clip_rect(clip);
            clicked = show(ui, Table::Full, &block, &transport, &tokens);
        },
    );
    output.textures_delta.clear();
    (clicked, output)
}

#[test]
fn a_culled_block_adds_no_link_node() {
    let ctx = context();
    ctx.enable_accesskit();
    let release = release(0);
    let want = format!("Open album {}", release.name);

    // The control: a clip that covers the block does give its name a link
    // node, so an empty list below is the cull and not a missing harness.
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 2000.0));
    let (_, visible) = clipped(&ctx, &release, screen);
    assert!(
        link_labels(&visible).iter().any(|label| label == &want),
        "the visible block's name is no link: {:?}",
        link_labels(&visible)
    );

    // A block scrolled clear of the pane still draws its head, but none of
    // it is visible, so none of it is a link.
    let clear = Rect::from_min_size(Pos2::new(0.0, 1500.0), Vec2::new(900.0, 400.0));
    let (clicked, culled) = clipped(&ctx, &release, clear);
    assert!(
        link_labels(&culled).is_empty(),
        "a culled block registered a link"
    );
    assert_eq!(clicked.open, None, "a culled block opened a page");
}

/// The menu names the release alone, so the `…` must open whether or not a
/// list has answered about the release yet.
#[test]
fn the_blocks_more_button_opens_a_menu() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let transport = Transport::default();
    let release = release(0);
    let likes = Likes::default();
    let mut clicked = Clicked::default();
    let mut at = Pos2::ZERO;

    let input = |events| RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 2000.0))),
        events,
        ..Default::default()
    };
    let mut warm = ctx.run_ui(input(Vec::new()), |_ui| {});
    warm.textures_delta.clear();

    let block = Block {
        release: &release,
        labels: Labels::Idle,
        cover: Cover::Missing,
        on_screen: true,
        playable: true,
        here: None,
        writing: false,
        likes: &likes,
        saved: None,
    };
    let mut found = ctx.run_ui(input(Vec::new()), |ui| {
        let head_top = ui.cursor().min.y + stroke::HAIRLINE + space::S5;
        at = pos2(
            ui.max_rect().right() - extent::BUTTON_LG / 2.0,
            head_top + extent::RELEASE_ART / 2.0,
        );
        let _ = show(ui, Table::Full, &block, &transport, &tokens);
    });
    found.textures_delta.clear();
    let mut arrived = ctx.run_ui(input(vec![egui::Event::PointerMoved(at)]), |_ui| {});
    arrived.textures_delta.clear();

    for event in crate::view::test_support::click_events(at) {
        let mut output = ctx.run_ui(input(vec![event]), |ui| {
            clicked = show(ui, Table::Full, &block, &transport, &tokens);
        });
        output.textures_delta.clear();
    }

    assert!(
        clicked.more.is_some(),
        "the block's `…` refused a press, so its menu cannot be reached"
    );
}

#[test]
fn a_right_click_posts_the_blocks_menu() {
    let ctx = context();
    let release = release(2);
    let tracks = vec![
        labelled("spotify:track:1", "First"),
        labelled("spotify:track:2", "Second"),
    ];
    let here = crate::route::Page::Artist("spotify:artist:a".to_owned());
    let tokens = Tokens::DARK;
    let transport = Transport::default();
    let likes = crate::likes::Likes::default();
    let (_, output) = painted(&ctx, &release, &tracks, Vec::new());
    let at = text_rect(&output, "Second")
        .expect("the second row is painted")
        .center();

    let mut posted = None;
    for event in crate::view::test_support::secondary_click_events(at) {
        let block = Block {
            release: &release,
            labels: Labels::Ready(&tracks),
            cover: Cover::Missing,
            on_screen: true,
            playable: true,
            here: Some(&here),
            writing: false,
            likes: &likes,
            saved: None,
        };
        let mut drawn = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 800.0))),
                events: vec![event],
                ..Default::default()
            },
            |ui| {
                let _ = show(ui, Table::Full, &block, &transport, &tokens);
            },
        );
        drawn.textures_delta.clear();
        posted = crate::view::context_menu::take_request(&ctx);
    }

    let request = posted.expect("a right-click on a block row posted a request");
    assert_eq!(request.targets.len(), 1);
    assert_eq!(request.targets[0].uri, "spotify:track:2");
    assert_eq!(
        request.page,
        Some(crate::route::Page::Artist("spotify:artist:a".to_owned()))
    );
}

#[test]
fn a_single_click_selects_and_a_double_click_plays_it() {
    let ctx = context();
    let release = release(2);
    let tracks = vec![
        labelled("spotify:track:1", "First"),
        labelled("spotify:track:2", "Second"),
    ];
    let (_, output) = painted(&ctx, &release, &tracks, Vec::new());
    let row = text_rect(&output, "Second").expect("the row's title");
    let id = Id::unique(("release-tracks", &release.uri));

    let mut clicked = Clicked::default();
    for event in click_events(row.center()) {
        clicked = painted(&ctx, &release, &tracks, vec![event]).0;
    }

    assert_eq!(clicked.row, None, "a single click plays nothing");
    assert!(
        crate::view::carry::List::open(&ctx, id, 2).look(1).selected,
        "the row under the click is picked"
    );
    assert!(
        !crate::view::carry::List::open(&ctx, id, 2).look(0).selected,
        "and only that row"
    );

    let mut clicked = Clicked::default();
    // A fresh block for the double click: a third click inside the window is
    // a triple click, and that plays nothing.
    let ctx = context();
    for _ in 0..2 {
        for event in click_events(row.center()) {
            clicked = painted(&ctx, &release, &tracks, vec![event]).0;
        }
    }

    assert_eq!(clicked.row, Some(1), "a double click plays the row");
}
