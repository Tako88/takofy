use super::super::*;
use super::test_support::*;
use crate::message::Update;

#[test]
fn a_right_click_posts_the_popular_menu() {
    let ctx = context();
    let mut library = library_with(detail(
        vec![
            track("spotify:track:t0", "One", "Vela", 200_000),
            track("spotify:track:t1", "Two", "Vela", 210_000),
        ],
        Vec::new(),
    ));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();
    let (_, output) = frame(
        &ctx,
        PAGE,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let at = text_rect(&output, "Two")
        .expect("the second popular row is painted")
        .center();

    for event in crate::view::test_support::secondary_click_events(at) {
        let _ = frame(
            &ctx,
            PAGE,
            vec![event],
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
    }
    let request = crate::view::context_menu::take_request(&ctx)
        .expect("a right-click on a popular row posted a request");
    assert_eq!(request.targets.len(), 1);
    assert_eq!(request.targets[0].uri, "spotify:track:t1");
    assert_eq!(request.page, Some(Page::Artist(ARTIST.to_owned())));
    assert!(
        !request
            .items
            .iter()
            .any(|item| item.verb == crate::menu::Verb::GoToArtist),
        "the artist this page already is is not one of its verbs"
    );
}

/// The likeness row draws under the discography, and a press opens that
/// artist's page.
#[test]
fn the_likeness_row_draws_its_cards_and_a_press_opens_the_artist() {
    let ctx = context();
    let mut detail = detail(
        vec![track("spotify:track:t", "Top", "Vela", 200_000)],
        vec![release(
            "spotify:album:a",
            "Album One",
            &["spotify:track:t"],
        )],
    );
    detail.related = vec!["spotify:artist:other".to_owned()];
    let mut library = library_with(detail);
    // The cards are the page's second read, so the request comes first.
    let _ = library.related_request();
    library.apply(&Update::Related {
        artist: ARTIST.to_owned(),
        cards: vec![FollowedArtist {
            uri: "spotify:artist:other".to_owned(),
            name: "Other Artist".to_owned(),
            named: true,
            portrait: None,
            releases: None,
        }],
    });
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        PAGE,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let texts: Vec<&str> = drawn_texts(&output);
    assert!(texts.contains(&"Fans also like"), "{texts:?}");
    assert!(texts.contains(&"Other Artist"), "{texts:?}");

    let card = text_rect(&output, "Other Artist").expect("the card's name");
    let (command, _) = frame(
        &ctx,
        PAGE,
        crate::view::test_support::click_events(card.center()).to_vec(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert_eq!(
        command,
        Some(Command::LoadArtist {
            artist: "spotify:artist:other".to_owned()
        }),
        "a card opens that artist"
    );
}

/// An artist the wire names nobody for draws no section at all, and one with
/// no releases still draws the row after its empty copy.
#[test]
fn the_likeness_row_is_absent_without_cards_and_survives_an_empty_discography() {
    let ctx = context();
    let mut bare = detail(vec![], vec![]);
    bare.related = vec!["spotify:artist:other".to_owned()];
    let mut library = library_with(bare);
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        PAGE,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert!(!drawn_texts(&output).contains(&"Fans also like"));

    let _ = library.related_request();
    library.apply(&Update::Related {
        artist: ARTIST.to_owned(),
        cards: vec![FollowedArtist {
            uri: "spotify:artist:other".to_owned(),
            name: "Other Artist".to_owned(),
            named: true,
            portrait: None,
            releases: None,
        }],
    });
    let (_, output) = frame(
        &ctx,
        PAGE,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let texts = drawn_texts(&output);
    assert!(texts.contains(&"No releases."), "{texts:?}");
    assert!(
        texts.contains(&"Fans also like"),
        "the row survives an empty discography: {texts:?}"
    );
}
