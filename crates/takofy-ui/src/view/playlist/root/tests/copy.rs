use super::super::*;
use super::test_support::*;

use takofy_model::{Account, PlaylistEntry};

use crate::library::Snapshot;
use crate::message::Update;

fn account() -> Account {
    Account::from_wire("dana").expect("an account")
}

/// A root drawn from last session's copy, whose playlists refresh failed.
fn stale(playlists: Vec<PlaylistEntry>) -> Library {
    let mut library = Library::default();
    let mut snapshot = Snapshot::new(account());
    snapshot.playlists = Some(playlists);
    library.restore(snapshot);
    library.apply(&Update::Account(account()));
    library.apply(&Update::LibraryReady);
    library.apply(&Update::PlaylistFailed(Failure::Load));
    library
}

#[test]
fn a_failed_refresh_draws_the_copy_note_over_its_kept_rows() {
    let ctx = context();
    let mut library = stale(vec![playlist("spotify:playlist:a", "A")]);
    let mut art = Art::default();

    let output = frame(&ctx, &mut library, &mut art);
    let texts = drawn_texts(&output);

    assert!(
        matches!(idle_view(&library), IdleView::Grid { .. }),
        "the kept rows still make a grid: {:?}",
        idle_view(&library)
    );
    assert!(texts.contains(&copy_note()), "{texts:?}");
    assert!(texts.contains(&"A"), "the kept card is drawn: {texts:?}");
    assert!(
        !texts.contains(&crate::signin::failure_message(Failure::Load)),
        "the failure is not what a kept row draws: {texts:?}"
    );
}

#[test]
fn the_copy_notes_retry_asks_for_the_library_again() {
    let ctx = context();
    let mut library = stale(vec![playlist("spotify:playlist:a", "A")]);
    let mut art = Art::default();

    let before = frame(&ctx, &mut library, &mut art);
    let retry = text_rect(&before, "Retry").expect("the note's retry");
    let command = click(&ctx, retry.center(), &mut library, &mut art);

    assert_eq!(command, Some(Command::ReloadLibrary));
    assert!(
        matches!(library.playlists, Load::Ready(_)),
        "the retry keeps the rows it is refreshing"
    );
    assert!(!library.copy_failed(), "and the sentence goes with it");
}

#[test]
fn a_live_root_draws_no_copy_note() {
    let ctx = context();
    let mut library = library(vec![playlist("spotify:playlist:a", "A")], vec![]);
    let mut art = Art::default();

    let output = frame(&ctx, &mut library, &mut art);

    assert!(!drawn_texts(&output).contains(&copy_note()));
}
