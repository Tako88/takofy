use egui::{pos2, vec2};
use takofy_model::{ArtistDetail, Release as _Release, ReleaseKind};

use super::super::*;
use super::test_support::*;
use crate::cover::Cover;
use crate::message::{Failure, Update};
use crate::releases::Labels;
use crate::theme::stroke;
use crate::view::release_block::Block;
use crate::view::test_support::click_events;

#[test]
fn a_release_rows_double_click_plays_that_track() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let size = Vec2::new(900.0, 800.0);
    let release = _Release {
        uri: "spotify:album:r1".to_owned(),
        name: "One".to_owned(),
        kind: ReleaseKind::Album,
        year: Some(2024),
        cover: None,
        tracks: vec!["spotify:track:1".to_owned(), "spotify:track:2".to_owned()],
    };
    let labels = vec![
        track("spotify:track:1", "One", "", 222_000),
        track("spotify:track:2", "Two", "", 198_000),
    ];
    let mut transport = Transport::default();
    let mut clicked = None;
    let likes = crate::likes::Likes::default();

    // The second row's rect inside a block drawn at the top of the pane.
    let mut row_rect = None;
    run(&ctx, input(size, Vec::new()), |ui| {
        let top = ui.cursor().min;
        let list_top = top.y + stroke::HAIRLINE + space::S5 + extent::RELEASE_ART + space::S4;
        let list = Rect::from_min_size(
            pos2(top.x, list_top),
            vec2(ui.available_width(), 2.0 * extent::ROW_H),
        );
        row_rect = Some(table_layout::row_rect(list, 1, extent::ROW_H));
    });
    let row_rect = row_rect.expect("the block's body lays out");

    for _ in 0..2 {
        for event in click_events(row_rect.center()) {
            run(&ctx, input(size, vec![event]), |ui| {
                let block = Block {
                    release: &release,
                    labels: Labels::Ready(&labels),
                    cover: Cover::Missing,
                    on_screen: true,
                    playable: true,
                    here: None,
                    writing: false,
                    likes: &likes,
                    saved: None,
                };
                let out =
                    super::super::release_block::show(ui, Table::Full, &block, &transport, &tokens);
                clicked = Some(out);
            });
        }
    }

    assert_eq!(
        clicked.and_then(|clicked| clicked.row),
        Some(1),
        "the row under the double click is what plays"
    );
    let _ = &mut transport;
}

/// A debug build paints egui's `Unaligned` marker inside any widget whose
/// rect is off the rounding grid, which makes the chip row unreadable. The
/// page must draw none.
#[test]
fn the_page_paints_no_unaligned_marker() {
    let ctx = context();
    let mut library = library_with(detail(
        vec![track(
            "spotify:track:1",
            "Slow Tide",
            "Vela Moreno",
            222_000,
        )],
        vec![
            release("spotify:album:r1", "Slow Tide", &["spotify:track:1"]),
            release("spotify:album:r2", "Kestrel", &["spotify:track:1"]),
        ],
    ));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let art = &mut Art::default();

    let (_, output) = frame(
        &ctx,
        Vec2::new(900.0, 800.0),
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        art,
    );

    assert!(
        !drawn_texts(&output).contains(&"Unaligned"),
        "the page paints an unaligned widget: {:?}",
        drawn_texts(&output)
    );
}

/// The sections sit one gutter inside the pane, so a release's art does not
/// rub the panel's edge and the count does not touch the far one. The hero
/// stays full-bleed above them.
#[test]
fn the_sections_sit_one_gutter_inside_the_pane() {
    let ctx = context();
    let mut library = library_with(detail(
        vec![track(
            "spotify:track:1",
            "Slow Tide",
            "Vela Moreno",
            222_000,
        )],
        vec![release(
            "spotify:album:r1",
            "Slow Tide",
            &["spotify:track:1"],
        )],
    ));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let art = &mut Art::default();

    let probe = Vec2::new(900.0, 800.0);
    let (_, output) = frame(
        &ctx,
        probe,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        art,
    );

    let popular = text_rect(&output, "Popular").expect("the section head is painted");
    assert!(
        (popular.left() - space::GUTTER).abs() < 0.01,
        "the section title sits at {}, the pane's gutter is {}",
        popular.left(),
        space::GUTTER
    );

    let aside = drawn_texts(&output)
        .into_iter()
        .find(|text| text.contains("RELEASE"))
        .expect("the discography counts its releases");
    let aside = text_rect(&output, aside).expect("the aside is painted");
    assert!(
        (aside.right() - (probe.x - space::GUTTER)).abs() < 0.01,
        "the aside ends at {}, the pane's far gutter starts at {}",
        aside.right(),
        probe.x - space::GUTTER
    );

    // The hero's words start beside the sleeve, and the sleeve on the edge.
    let title = text_rect(&output, "Vela Moreno").expect("the hero title is painted");
    assert!(
        (title.left() - (crate::layout::hero::plate_side(probe.x) + space::S5)).abs() < 0.01,
        "the hero title sits at {}",
        title.left()
    );
}

#[test]
fn an_empty_artist_draws_both_empty_copies() {
    let ctx = context();
    let mut library = library_with(detail(Vec::new(), Vec::new()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        Vec2::new(900.0, 800.0),
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let texts = drawn_texts(&output);

    assert!(
        texts.contains(&"No popular tracks."),
        "the Popular empty copy: {texts:?}"
    );
    assert!(
        texts.contains(&"No releases."),
        "the Discography empty copy: {texts:?}"
    );
    assert!(texts.contains(&"Popular"), "the section head: {texts:?}");
    assert!(texts.contains(&"Discography"), "{texts:?}");
}

#[test]
fn a_failure_draws_the_message_and_a_retry_that_asks_again() {
    let ctx = context();
    let mut library = Library::default();
    library.open(Page::Artist(ARTIST.to_owned()));
    library.apply(&Update::ArtistFailed {
        artist: ARTIST.to_owned(),
        failure: Failure::Load,
    });
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        Vec2::new(900.0, 800.0),
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let texts = drawn_texts(&output);
    assert!(
        texts.contains(&crate::signin::failure_message(Failure::Load)),
        "the failure's own copy: {texts:?}"
    );

    let retry = text_rect(&output, "Retry").expect("a retry control");
    let command = click(
        &ctx,
        Vec2::new(900.0, 800.0),
        retry.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert_eq!(
        command,
        Some(Command::LoadArtist {
            artist: ARTIST.to_owned()
        }),
        "Retry re-issues the same load"
    );
    assert_eq!(library.artist, Load::Loading);
}

#[test]
fn a_ready_artist_for_another_page_draws_the_loading_state() {
    let ctx = context();
    let mut library = Library::default();
    library.open(Page::Artist(ARTIST.to_owned()));
    // The state the library guard exists for: a ready detail left from an
    // earlier page.
    library.artist = Load::Ready(ArtistDetail {
        uri: OTHER_ARTIST.to_owned(),
        name: "Another Artist".to_owned(),
        ..detail(Vec::new(), Vec::new())
    });
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        Vec2::new(900.0, 800.0),
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let texts = drawn_texts(&output);

    assert!(
        texts.is_empty(),
        "the page paints no words for another artist: {texts:?}"
    );
    assert!(
        output.shapes.iter().any(|clipped| matches!(
            &clipped.shape,
            egui::epaint::Shape::Rect(rect) if rect.fill == Tokens::DARK.palette.skeleton_rule
        )),
        "the page holds its rows open instead of naming another artist"
    );
}
