use super::super::*;
use super::test_support::*;

use std::sync::mpsc::channel;
use takofy_model::{LikedTrack, SavedAt};
use takofy_ui::route::Page;

/// A saved date well after the epoch, so the track is worth a card.
fn liked(uri: &str) -> LikedTrack {
    LikedTrack {
        uri: uri.to_owned(),
        saved_at: SavedAt::from_secs(1_700_000_000),
    }
}

/// The window a real launch builds, through the constructor eframe calls.
fn launched() -> TakofyApp {
    let (sender, updates) = channel();
    // Nothing will ever arrive; a disconnected channel is what the window
    // already tolerates.
    drop(sender);
    let (commands, _received) = tokio::sync::mpsc::unbounded_channel();
    let cc = eframe::CreationContext::_new_kittest(window_context());
    TakofyApp::new(
        &cc,
        UiEnd { updates, commands },
        Loaded::Absent,
        PrefsSync::off(),
    )
}

#[test]
fn the_window_opens_on_home() {
    let app = launched();

    assert_eq!(app.library.page(), Some(&Page::Home));
    assert!(
        !app.library.can_back(),
        "the start page has nowhere behind it"
    );
}

#[test]
fn logic_asks_home_for_the_saved_labels_once_per_uri() {
    let (mut app, mut received) = test_window();
    app.state = SignIn::SignedIn;
    app.library = Library::starting_home(1_700_000_000_000);
    app.library.apply(&Update::Playlists(Vec::new()));
    assert_eq!(
        app.library.likes_request(),
        Some(Command::LoadLikedTracks),
        "arrange: the liked read must be in flight"
    );
    app.library
        .apply(&Update::LikedTracks(vec![liked("spotify:track:t")]));
    app.library.apply(&Update::SavedAlbums(Vec::new()));

    logic_frame(&mut app);
    let sent = take_commands(&mut received);
    assert!(
        sent.contains(&Command::LabelSaved {
            tracks: vec!["spotify:track:t".to_owned()],
        }),
        "the shelf's labels are asked for: {sent:?}"
    );

    logic_frame(&mut app);
    let sent = take_commands(&mut received);
    assert!(
        !sent
            .iter()
            .any(|command| matches!(command, Command::LabelSaved { .. })),
        "a URI is asked for once: {sent:?}"
    );
}
