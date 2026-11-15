use super::super::*;
use super::test_support::*;

use chrono::FixedOffset;
use eframe::App;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use takofy_model::home::{DiscoverySeed, HomeAsk};
use takofy_model::{FollowedArtist, PlaylistEntry, SavedAlbum};
use takofy_ui::home::discovery::hidden::Hidden;
use takofy_ui::message::Update;

use crate::hidden_file::{HiddenWriter, Loaded as HiddenLoaded};

const NOW_MS: u64 = 1_700_000_000_000;

fn playlist(uri: &str) -> PlaylistEntry {
    PlaylistEntry {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        cover: None,
        composed: Vec::new(),
        description: None,
        tracks: None,
        owner: None,
        access: None,
        saved_at: None,
    }
}

fn artist(uri: &str) -> FollowedArtist {
    FollowedArtist {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        portrait: None,
        releases: None,
    }
}

fn album(uri: &str, artist: &str) -> SavedAlbum {
    SavedAlbum {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        artists: artist.to_owned(),
        artist_uris: vec![artist.to_owned()],
        artist_names: vec![artist.to_owned()],
        year: None,
        cover: None,
        tracks: None,
        saved_at: None,
    }
}

/// A window on Home whose three pools' lists have all answered.
fn ready_window() -> (TakofyApp, tokio::sync::mpsc::UnboundedReceiver<Command>) {
    let (mut app, received) = test_window();
    app.state = SignIn::SignedIn;
    let mut library = Library::starting_home(NOW_MS);
    library.apply(&Update::LibraryReady);
    library.apply(&Update::Playlists(
        ["a", "b", "c", "d"]
            .map(|tail| playlist(&format!("spotify:playlist:{tail}")))
            .to_vec(),
    ));
    library.apply(&Update::SavedAlbums(
        ["a", "b", "c", "d"]
            .map(|tail| {
                album(
                    &format!("spotify:album:{tail}"),
                    &format!("spotify:artist:saved-{tail}"),
                )
            })
            .to_vec(),
    ));
    library.apply(&Update::FollowedArtists(
        ["a", "b", "c", "d"]
            .map(|tail| artist(&format!("spotify:artist:{tail}")))
            .to_vec(),
    ));
    // The liked read is the dated shelf's own intake, so it has to be in
    // flight before its answer can settle it.
    assert!(
        library.likes_request().is_some(),
        "arrange: the liked read is armed"
    );
    library.apply(&Update::LikedTracks(Vec::new()));
    app.library = library;
    (app, received)
}

/// The lookups the frames since the last call asked for.
fn lookups(received: &mut tokio::sync::mpsc::UnboundedReceiver<Command>) -> Vec<DiscoverySeed> {
    take_commands(received)
        .into_iter()
        .filter_map(|command| match command {
            Command::LoadHome(HomeAsk::Discovery { seed }) => Some(seed),
            _ => None,
        })
        .collect()
}

/// The window a real launch builds, through the constructor eframe calls.
fn launched(
    loaded: Loaded,
    hidden: HiddenLoaded,
) -> (TakofyApp, tokio::sync::mpsc::UnboundedReceiver<Command>) {
    let (sender, updates) = channel();
    // Nothing will ever arrive; a disconnected channel is what the window
    // already tolerates.
    drop(sender);
    let (commands, received) = tokio::sync::mpsc::unbounded_channel();
    let cc = eframe::CreationContext::_new_kittest(window_context());
    let gate = Arc::new(AtomicBool::new(loaded_discovery(&loaded)));
    let app = TakofyApp::new(
        &cc,
        UiEnd { updates, commands },
        loaded,
        PrefsSync::off(),
        Cached::Absent,
        SnapshotSync::off(),
        gate,
        hidden,
        HiddenSync::off(),
    );
    (app, received)
}

/// The preferences the file held, parsed from `text`.
fn held(text: &str) -> Loaded {
    Loaded::Found(Prefs::parse(text).expect("valid prefs"))
}

#[test]
fn the_day_number_follows_the_local_date_across_midnight() {
    let zone = FixedOffset::east_opt(3_600).expect("a one-hour offset");
    let last = zone
        .with_ymd_and_hms(2024, 3, 10, 23, 59, 59)
        .single()
        .expect("a date in a fixed offset");
    let first = zone
        .with_ymd_and_hms(2024, 3, 11, 0, 0, 1)
        .single()
        .expect("a date in a fixed offset");
    let noon = zone
        .with_ymd_and_hms(2024, 3, 11, 12, 0, 0)
        .single()
        .expect("a date in a fixed offset");

    assert_eq!(
        local_day_at(&first).days(),
        local_day_at(&last).days() + 1,
        "a minute over local midnight is the next day"
    );
    assert_eq!(
        local_day_at(&first),
        local_day_at(&noon),
        "the whole of the date shares its day number"
    );
}

#[test]
fn the_day_number_is_the_zone_and_not_the_instant() {
    // The same instant in two zones: the day it falls on is the zone's
    // business, so the two readings differ.
    let east = FixedOffset::east_opt(3_600).expect("a one-hour offset");
    let west = FixedOffset::west_opt(3_600).expect("a one-hour offset");
    let evening = east
        .with_ymd_and_hms(2024, 3, 11, 0, 30, 0)
        .single()
        .expect("a date in a fixed offset");
    let same_moment = evening.with_timezone(&west);

    assert_eq!(
        local_day_at(&evening).days(),
        local_day_at(&same_moment).days() + 1,
        "two hours west of the same reading it is still the previous evening"
    );
}

#[test]
fn a_frame_names_the_day_before_it_asks() {
    let (mut app, mut received) = ready_window();

    logic_frame(&mut app);

    assert!(
        !lookups(&mut received).is_empty(),
        "a frame that names the day and then asks owes a lookup"
    );
    assert!(
        app.library.hidden().positions_for(local_day()).is_some(),
        "the day's rows were recorded, which only an ask after the naming does"
    );
    assert!(
        app.gate.load(Ordering::Relaxed),
        "the gate follows the setting"
    );
}

#[test]
fn a_window_with_the_setting_off_asks_for_no_lookup() {
    let (mut app, mut received) = ready_window();
    app.library.set_discovery_enabled(false);

    for _ in 0..3 {
        logic_frame(&mut app);
    }

    assert!(
        lookups(&mut received).is_empty(),
        "an off setting sends no lookup"
    );
    assert!(
        app.library.hidden().positions_for(local_day()).is_none(),
        "no row ran, so no day was recorded"
    );
    assert!(
        !app.gate.load(Ordering::Relaxed),
        "the gate follows the setting, so a lookup already on its way stops"
    );
}

#[test]
fn the_window_starts_on_the_setting_the_file_held() {
    let (app, _received) = launched(held("volume=40\ndiscovery=false\n"), HiddenLoaded::Absent);

    assert!(
        !app.library.discovery_enabled(),
        "the file's off setting is what the window starts on"
    );
    assert!(!app.gate.load(Ordering::Relaxed), "the gate starts closed");
}

#[test]
fn a_window_with_no_file_starts_on_the_default() {
    let (app, _received) = launched(Loaded::Absent, HiddenLoaded::Absent);

    assert!(
        app.library.discovery_enabled(),
        "a missing file leaves the setting on"
    );
    assert!(app.gate.load(Ordering::Relaxed), "the gate starts open");
}

#[test]
fn the_hidden_file_is_what_the_window_starts_with() {
    let saved = Hidden::parse("hide=spotify:artist:n1\n").expect("a hidden set");
    let (app, _received) = launched(Loaded::Absent, HiddenLoaded::Found(saved));

    assert!(
        app.library.hidden().contains("spotify:artist:n1"),
        "the artists the file hid are hidden from the first frame"
    );
}

#[test]
fn a_hide_reaches_the_file_when_the_window_closes() {
    let log: Arc<Mutex<Vec<Hidden>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = {
        let log = Arc::clone(&log);
        move |hidden: &Hidden| {
            log.lock().expect("the sink log").push(hidden.clone());
            Ok(())
        }
    };
    let (mut app, _received) = launched(Loaded::Absent, HiddenLoaded::Absent);
    app.hidden = HiddenSync::for_test(0, HiddenWriter::with_sink(sink, Duration::from_secs(2)));

    app.library.hide_artist("spotify:artist:n1");
    App::on_exit(&mut app, None);
    drop(app);

    let saved = log.lock().expect("the sink log");
    assert!(
        saved
            .iter()
            .any(|hidden| hidden.contains("spotify:artist:n1")),
        "closing writes the hide no frame had time to settle: {saved:?}"
    );
}

#[test]
fn an_unhide_reaches_the_file_when_the_window_closes() {
    let log: Arc<Mutex<Vec<Hidden>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = {
        let log = Arc::clone(&log);
        move |hidden: &Hidden| {
            log.lock().expect("the sink log").push(hidden.clone());
            Ok(())
        }
    };
    let saved = Hidden::parse("hide=spotify:artist:n1\n").expect("a hidden set");
    let (mut app, _received) = launched(Loaded::Absent, HiddenLoaded::Found(saved));
    app.hidden = HiddenSync::for_test(0, HiddenWriter::with_sink(sink, Duration::from_secs(2)));

    app.library.unhide_artist("spotify:artist:n1");
    App::on_exit(&mut app, None);
    drop(app);

    let saved = log.lock().expect("the sink log");
    assert!(
        saved
            .last()
            .is_some_and(|hidden| !hidden.contains("spotify:artist:n1")),
        "closing writes the unhide no frame had time to settle: {saved:?}"
    );
}
