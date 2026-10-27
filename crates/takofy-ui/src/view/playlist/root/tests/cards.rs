use super::super::*;
use super::test_support::*;

use std::time::Instant;

use takofy_model::{CoverSet, CoverUrl, FollowedShow};

use crate::route::Page;

#[test]
fn a_show_card_click_opens_the_show_and_returns_load_show() {
    let ctx = context();
    let mut library = library(vec![playlist("a", "A")], vec![show("s", "Show One")]);
    let mut art = Art::default();
    let _ = frame(&ctx, &mut library, &mut art);

    let card = card_rect_for(&ctx, 1, 2);
    let command = click(&ctx, card.center(), &mut library, &mut art);

    assert_eq!(
        command,
        Some(Command::LoadShow {
            show: "s".to_owned()
        }),
        "a show card opens its page"
    );
    assert_eq!(library.page(), Some(&Page::Show("s".to_owned())));
}

#[test]
fn a_playlist_card_click_still_selects() {
    let ctx = context();
    let mut library = library(vec![playlist("a", "A")], vec![show("s", "Show One")]);
    let mut art = Art::default();
    let _ = frame(&ctx, &mut library, &mut art);

    let card = card_rect_for(&ctx, 0, 2);
    let command = click(&ctx, card.center(), &mut library, &mut art);

    assert_eq!(
        command,
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        })
    );
}

#[test]
fn choosing_podcasts_draws_only_show_cards() {
    let ctx = context();
    let mut library = library(vec![playlist("a", "List One")], vec![show("s", "Show One")]);
    let mut art = Art::default();

    let before = frame(&ctx, &mut library, &mut art);
    let chip = chip_rect(&before, "Podcasts");
    let _ = click(&ctx, chip.center(), &mut library, &mut art);

    let after = frame(&ctx, &mut library, &mut art);
    let texts = drawn_texts(&after);
    assert!(texts.contains(&"Show One"), "{texts:?}");
    assert!(
        !texts.contains(&"List One"),
        "the playlists card must be gone: {texts:?}"
    );
}

#[test]
fn the_page_head_note_counts_the_cards_shown() {
    let ctx = context();
    let mut library = library(
        vec![playlist("a", "A"), playlist("b", "B")],
        vec![show("s", "Show One")],
    );
    let mut art = Art::default();

    let output = frame(&ctx, &mut library, &mut art);
    let texts = drawn_texts(&output);
    assert!(
        texts.contains(&"3 SAVED"),
        "the note counts every card: {texts:?}"
    );

    library.root_filter.choose(RootKind::Podcasts);
    let output = frame(&ctx, &mut library, &mut art);
    let texts = drawn_texts(&output);
    assert!(
        texts.contains(&"1 SAVED"),
        "a narrowed chip counts only its own cards: {texts:?}"
    );
}

#[test]
fn only_visible_cards_ask_for_covers() {
    let ctx = context();
    let shows: Vec<FollowedShow> = (0..40)
        .map(|index| FollowedShow {
            uri: format!("spotify:show:{index}"),
            name: format!("Show {index}"),
            named: true,
            publisher: None,
            cover: Some(CoverSet::single(
                CoverUrl::from_wire(&format!("https://i.scdn.co/image/{index:0<40}"))
                    .expect("a cover url"),
            )),
            episodes: None,
        })
        .collect();
    let mut library = library(vec![], shows);
    let mut art = Art::default();
    let now = Instant::now();

    art.begin_frame();
    let _ = frame(&ctx, &mut library, &mut art);
    let requests = art.requests(now);
    assert!(!requests.is_empty(), "the visible cards must be fetched");
    assert!(
        requests.len() < 20,
        "only the cards the viewport shows are asked for: {}",
        requests.len()
    );
}
