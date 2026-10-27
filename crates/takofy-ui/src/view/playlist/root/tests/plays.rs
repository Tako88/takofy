use super::super::*;
use super::test_support::*;

use crate::message::Update;

#[test]
fn a_press_on_a_cards_play_wants_the_play_and_selects_nothing() {
    let ctx = context();
    let mut art = Art::default();
    let mut library = library(vec![playlist("spotify:playlist:a", "Mix")], Vec::new());
    let card = card_rect_for(&ctx, 0, 1);

    let command = click(
        &ctx,
        media::tile_play_rect(card).center(),
        &mut library,
        &mut art,
    );

    assert_eq!(command, None, "a play press took the frame's command");
    assert_eq!(library.page(), None, "a play press opened the page");
    assert_eq!(library.selected(), None, "a play press selected the page");
    assert_eq!(
        library.play_request(),
        Some(Command::LoadTracks {
            playlist: "spotify:playlist:a".to_owned()
        }),
        "the press never became a want"
    );
}

#[test]
fn a_press_on_a_rows_play_wants_the_play_and_opens_nothing() {
    let ctx = context();
    let mut art = Art::default();
    let mut library = library(vec![playlist("spotify:playlist:a", "Mix")], Vec::new());
    library.layout = MediaLayout::List;
    let row = row_rect_for(&ctx, 0);

    let command = click(
        &ctx,
        media::row_play_rect(row).center(),
        &mut library,
        &mut art,
    );

    assert_eq!(command, None, "a play press took the frame's command");
    assert_eq!(library.page(), None, "a play press opened the page");
    assert_eq!(library.selected(), None, "a play press selected the page");
    assert_eq!(
        library.play_request(),
        Some(Command::LoadTracks {
            playlist: "spotify:playlist:a".to_owned()
        }),
        "the press never became a want"
    );
}

#[test]
fn a_failed_shelf_play_draws_the_load_sentence_under_the_chips() {
    let ctx = context();
    let mut art = Art::default();
    let mut library = library(vec![playlist("spotify:playlist:a", "Mix")], Vec::new());
    assert!(library.want_play("spotify:playlist:a", SidebarKind::Playlists));
    assert!(library.play_request().is_some());
    library.apply(&Update::TracksFailed {
        playlist: "spotify:playlist:a".to_owned(),
        failure: Failure::Load,
    });

    let output = frame(&ctx, &mut library, &mut art);
    let sentence = crate::signin::failure_message(Failure::Load);
    let line = text_rect(&output, sentence).expect("the failure sentence is drawn");
    let chips = text_rect(&output, "ALL").expect("the chips are drawn");
    let card = text_rect(&output, "Mix").expect("the card is drawn");

    assert!(
        line.top() >= chips.bottom(),
        "the sentence sits over the chips: {line:?} in {chips:?}"
    );
    assert!(
        line.bottom() <= card.top(),
        "the sentence sits under the cards: {line:?} in {card:?}"
    );
}
