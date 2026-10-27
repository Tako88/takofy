use egui::{Pos2, Rect};

use super::super::*;
use super::test_support::*;

use crate::view::test_support::click_events;

#[test]
fn play_shuffle_heart_more_report_their_presses() {
    let ctx = context();
    let rested = frame(&ctx, 0.0, Vec::new());
    let tiles = placed(&rested, VIEWPORT);
    let shuffle = tiles.shuffle.expect("a playlist's row has a shuffle");

    let at = |target: Pos2| {
        let mut last = Reply::quiet();
        for event in click_events(target) {
            last = frame(&ctx, 0.0, vec![event]).reply;
        }
        last
    };

    assert!(at(tiles.play.center()).played, "play reports its press");
    assert!(at(shuffle.center()).shuffled, "shuffle reports its press");
    assert_eq!(
        at(tiles.heart.expect("a wide row keeps its heart").center()).saved,
        Some(false),
        "a saved context's heart asks to unsave it"
    );
    assert!(
        at(tiles.more.center()).more_door.is_some(),
        "the menu reports its door"
    );
    assert!(
        !at(rested.flow.center()).played,
        "the row's middle is inert"
    );
}

#[test]
fn the_heart_says_what_it_would_do_and_only_acts_on_a_known_answer() {
    let saved = heart(Some(true));
    assert_eq!(saved.label, "Remove from your library");
    assert!(saved.lit, "a saved context's heart is filled");
    assert!(saved.live);
    assert_eq!(saved.icon, Icon::HeartFilled);

    let unsaved = heart(Some(false));
    assert_eq!(unsaved.label, "Save to your library");
    assert!(!unsaved.lit);
    assert!(unsaved.live);
    assert_eq!(unsaved.icon, Icon::Heart);

    let unknown = heart(None);
    assert!(
        !unknown.live,
        "no list could say, so the heart offers nothing"
    );
    assert!(!unknown.lit);
    assert_eq!(unknown.icon, Icon::Heart);
}

/// Whether an accent-filled path lies inside `rect`.
///
/// The filled glyph is polygons and the outline is strokes, so a fill is the
/// only thing that tells the two hearts apart.
fn accent_path_in(output: &egui::FullOutput, tokens: &Tokens, rect: Rect) -> bool {
    output.shapes.iter().any(|clipped| match &clipped.shape {
        egui::epaint::Shape::Path(path) if path.fill == tokens.palette.accent => {
            rect.contains(clipped.shape.visual_bounding_rect().center())
        }
        _ => false,
    })
}

#[test]
fn a_saved_contexts_heart_paints_an_accent_filled_path() {
    let tokens = Tokens::DARK;
    let ctx = context();
    let kind = kind(VIEWPORT);
    let mut saved = bar(kind);
    saved.saved = Some(true);
    let heart_rect = drawn(&ctx, &saved, VIEWPORT, 0.0)
        .heart
        .expect("the row draws its heart");
    let frame = frame_of(&ctx, &saved, VIEWPORT, 0.0, Vec::new(), None);
    assert!(
        accent_path_in(&frame.output, &tokens, heart_rect),
        "a saved heart is a filled accent glyph"
    );

    for state in [Some(false), None] {
        let mut unsaved = bar(kind);
        unsaved.saved = state;
        let frame = frame_of(&ctx, &unsaved, VIEWPORT, 0.0, Vec::new(), None);
        assert!(
            !accent_path_in(&frame.output, &tokens, heart_rect),
            "an unsaved or unknown heart is an outline, not a filled path"
        );
    }
}
#[test]
fn the_playlist_bar_without_an_entry_draws_with_inert_heart_and_more() {
    let ctx = context();
    let mut rows = Rows::narrowed();
    // A playlist the library cannot name: nothing to call it, nothing to say
    // about the heart, and no menu to open.
    let unnamed = Bar {
        title: None,
        playable: true,
        shuffle: true,
        more: None,
        saved: None,
        kind: kind(VIEWPORT),
        text: None,
    };

    let rested = frame_of(
        &ctx,
        &unnamed,
        VIEWPORT,
        0.0,
        Vec::new(),
        Some(rows.filter()),
    );
    let tiles = placed(&rested, VIEWPORT);
    assert_eq!(
        text_at(&rested.output, "midnight").len(),
        1,
        "the field is drawn for a page with no name"
    );
    assert_eq!(
        rested.reply.hits.as_deref(),
        Some(&[1][..]),
        "and its query still narrows the list"
    );
    assert!(
        text_at(&rested.output, "Late Night Drive").is_empty(),
        "there is no name to paint"
    );
    let heart = tiles.heart.expect("the heart is present");
    let more = tiles.more;

    // Both are present and both are inert: no list could say what the
    // context's saved state is, and the menu has nothing to offer.
    let mut last = Reply::quiet();
    for event in click_events(heart.center()) {
        last = frame_of(
            &ctx,
            &unnamed,
            VIEWPORT,
            0.0,
            vec![event],
            Some(rows.filter()),
        )
        .reply;
    }
    assert_eq!(last.saved, None, "an unknown heart asks for nothing");
    assert!(!last.played, "a press on the heart is not a press on play");

    let mut last = Reply::quiet();
    for event in click_events(more.center()) {
        last = frame_of(
            &ctx,
            &unnamed,
            VIEWPORT,
            0.0,
            vec![event],
            Some(rows.filter()),
        )
        .reply;
    }
    assert!(last.more_door.is_none(), "an inert menu posts no door");
    assert!(!last.played, "a press on the menu is not a press on play");

    // The play tile still works on the tracks the page has.
    let mut last = Reply::quiet();
    for event in click_events(tiles.play.center()) {
        last = frame_of(
            &ctx,
            &unnamed,
            VIEWPORT,
            0.0,
            vec![event],
            Some(rows.filter()),
        )
        .reply;
    }
    assert!(last.played, "a page with no name still plays its tracks");
}
