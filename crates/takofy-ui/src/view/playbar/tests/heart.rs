use super::super::*;
use super::test_support::*;
use crate::view::test_support::click_events;
use egui::{Event, Modifiers};

/// The accent-filled paths inside `rect`, which is a liked heart and nothing
/// else in the now column.
fn accent_paths(output: &egui::FullOutput, tokens: &Tokens, rect: Rect) -> Vec<Rect> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Path(path) if path.fill == tokens.palette.accent => {
                Some(clipped.shape.visual_bounding_rect())
            }
            _ => None,
        })
        .filter(|found| rect.contains(found.center()))
        .collect()
}

#[test]
fn the_playbar_heart_reads_the_playing_tracks_state() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut transport = named_playing();

    let (liked, cols) = bar_frame(
        &ctx,
        &tokens,
        &mut transport,
        Playbar::Full,
        Some(true),
        Vec::new(),
    );
    assert!(
        !accent_paths(&liked, &tokens, cols[0]).is_empty(),
        "a liked track's playbar heart is not filled"
    );

    let (unliked, cols) = bar_frame(
        &ctx,
        &tokens,
        &mut transport,
        Playbar::Full,
        Some(false),
        Vec::new(),
    );
    assert!(
        accent_paths(&unliked, &tokens, cols[0]).is_empty(),
        "an unliked track's playbar heart took the accent"
    );
}

#[test]
fn a_click_on_the_playbar_heart_posts_the_opposite_for_the_playing_uri() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut transport = named_playing();

    let (output, cols) = bar_frame(
        &ctx,
        &tokens,
        &mut transport,
        Playbar::Full,
        Some(true),
        Vec::new(),
    );
    let heart = accent_paths(&output, &tokens, cols[0])
        .first()
        .copied()
        .expect("the liked heart is painted");
    let at = heart.center();

    // A warm frame and a move, so the hit test knows the pointer before the
    // press.
    let _ = bar_frame(
        &ctx,
        &tokens,
        &mut transport,
        Playbar::Full,
        Some(true),
        Vec::new(),
    );
    let _ = bar_frame(
        &ctx,
        &tokens,
        &mut transport,
        Playbar::Full,
        Some(true),
        vec![Event::PointerMoved(at)],
    );
    for event in click_events(at) {
        let _ = bar_frame(
            &ctx,
            &tokens,
            &mut transport,
            Playbar::Full,
            Some(true),
            vec![event],
        );
    }

    assert_eq!(
        crate::view::heart::take(&ctx),
        Some(crate::view::heart::Ask {
            tracks: vec!["spotify:track:x".to_owned()],
            liked: false,
        })
    );
}

#[test]
fn a_held_enter_on_the_playbar_heart_toggles_once() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut transport = named_playing();

    // A warm pass registers the controls, then Tab takes the keyboard to
    // the now column's heart, which is the first control drawn.
    let _ = bar_frame(
        &ctx,
        &tokens,
        &mut transport,
        Playbar::Full,
        Some(true),
        Vec::new(),
    );
    let _ = bar_frame(
        &ctx,
        &tokens,
        &mut transport,
        Playbar::Full,
        Some(true),
        vec![key(Key::Tab, Modifiers::NONE)],
    );
    let _ = crate::view::heart::take(&ctx);

    let enter = |repeat| Event::Key {
        key: Key::Enter,
        physical_key: None,
        pressed: true,
        repeat,
        modifiers: Modifiers::NONE,
    };
    // The set flips after the first ask, as the shell would flip it, so a
    // repeat that reached the heart would flip it straight back.
    let mut asks = Vec::new();
    for repeat in [false, true, true] {
        let _ = bar_frame(
            &ctx,
            &tokens,
            &mut transport,
            Playbar::Full,
            Some(!repeat),
            vec![enter(repeat)],
        );
        asks.push(crate::view::heart::take(&ctx));
    }

    assert_eq!(
        asks,
        vec![
            Some(crate::view::heart::Ask {
                tracks: vec!["spotify:track:x".to_owned()],
                liked: false,
            }),
            None,
            None,
        ],
        "a held Enter posted more than one ask: {asks:?}"
    );
}

#[test]
fn the_playbar_and_a_row_of_the_same_track_agree() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut likes = crate::likes::Likes::default();
    assert!(likes.request(true));
    likes.apply(&crate::likes::Event::Set(vec![takofy_model::LikedTrack {
        uri: "spotify:track:x".to_owned(),
        saved_at: None,
    }]));
    let state = likes.state("spotify:track:x");
    assert_eq!(state, Some(true), "the set did not answer");

    let mut transport = named_playing();
    let (output, cols) = bar_frame(
        &ctx,
        &tokens,
        &mut transport,
        Playbar::Full,
        state,
        Vec::new(),
    );
    assert!(
        !accent_paths(&output, &tokens, cols[0]).is_empty(),
        "the playbar disagreed with the row's own answer"
    );
}

#[test]
fn the_reduced_bar_keeps_the_heart() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut transport = named_playing();
    let (output, cols) = bar_frame(
        &ctx,
        &tokens,
        &mut transport,
        Playbar::Reduced,
        Some(true),
        Vec::new(),
    );
    assert!(
        !accent_paths(&output, &tokens, cols[0]).is_empty(),
        "the reduced bar shed its heart"
    );
}
