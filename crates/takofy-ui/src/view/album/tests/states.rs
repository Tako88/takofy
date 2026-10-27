use takofy_model::AlbumDetail;

use super::super::*;
use super::test_support::*;
use crate::message::{Failure, Update};

#[test]
fn a_failure_draws_the_message_and_a_retry_that_asks_again() {
    let ctx = context();
    let mut library = Library::default();
    library.open(Page::Album(ALBUM.to_owned()));
    library.apply(&Update::AlbumFailed {
        album: ALBUM.to_owned(),
        failure: Failure::Load,
    });
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
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
        retry.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert_eq!(
        command,
        Some(Command::LoadAlbum {
            album: ALBUM.to_owned()
        }),
        "Retry re-issues the same load"
    );
    assert_eq!(library.album, Load::Loading);
}

/// A ready detail that does not name this page belongs to another album: the
/// page keeps its loading state rather than painting that album's band and
/// rows.
#[test]
fn a_ready_album_for_another_page_draws_the_loading_state() {
    let ctx = context();
    let mut library = Library::default();
    library.open(Page::Album(ALBUM.to_owned()));
    // The state the library guard exists for: a ready detail left from an
    // earlier page. `open` marks this page loading again, so it is staged
    // directly, as the library's own guard test does.
    library.album = Load::Ready(AlbumDetail {
        uri: OTHER_ALBUM.to_owned(),
        name: "Another Record".to_owned(),
        tracks: vec![track("spotify:track:z", "Elsewhere", "Other Band", 111_000)],
        ..detail(Vec::new())
    });
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let texts = drawn_texts(&output);

    assert!(
        band_top(&output, PROBE.x).is_none(),
        "the page draws no band for an album it does not name: {texts:?}"
    );
    assert!(
        texts.is_empty(),
        "the page paints no words for another album: {texts:?}"
    );
    assert!(
        output.shapes.iter().any(|clipped| matches!(
            &clipped.shape,
            egui::epaint::Shape::Rect(rect) if rect.fill == Tokens::DARK.palette.skeleton_rule
        )),
        "the page holds its rows open instead of naming another album"
    );
}

#[test]
fn the_loading_skeleton_and_the_loaded_table_put_the_title_at_the_same_left() {
    let ctx = context();
    let mut library = Library::default();
    library.open(Page::Album(ALBUM.to_owned()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let loading = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(rect)
                if rect.fill == Tokens::DARK.palette.skeleton_rule
                    && (rect.rect.height() - extent::SKELETON_LINE).abs() < 0.01 =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
        .next()
        .expect("the loading list holds a title bar open");

    let ctx = context();
    let mut library = library_with(detail(vec![track(
        "spotify:track:a",
        "One",
        "Band",
        222_000,
    )]));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();
    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let loaded = text_rect(&output, "One").expect("the row's name");

    assert_eq!(
        loading.left(),
        loaded.left(),
        "the title does not shift when the album lands"
    );
}
