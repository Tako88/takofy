use super::super::*;
use super::test_support::*;

use crate::snapshot::{Cached, QUIET, SnapshotSync, SnapshotWriter};
use eframe::App;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use takofy_model::{
    Account, FollowedArtist, FollowedShow, LikedTrack, PlaylistAccess, PlaylistEntry, Revision,
    SavedAlbum,
};
use takofy_ui::library::{Load, Snapshot};
use takofy_ui::message::Failure;

type SnapshotLog = Arc<Mutex<Vec<Snapshot>>>;

fn account(name: &str) -> Account {
    Account::from_wire(name).expect("an account")
}

fn playlist(uri: &str, name: &str) -> PlaylistEntry {
    PlaylistEntry {
        uri: uri.to_owned(),
        name: name.to_owned(),
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

fn show(uri: &str) -> FollowedShow {
    FollowedShow {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        publisher: None,
        episodes: None,
        cover: None,
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

fn album(uri: &str) -> SavedAlbum {
    SavedAlbum {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        artists: String::new(),
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        year: None,
        cover: None,
        tracks: None,
        saved_at: None,
    }
}

fn liked(uri: &str) -> LikedTrack {
    LikedTrack {
        uri: uri.to_owned(),
        saved_at: None,
    }
}

/// A copy holding one row in every list.
fn copy_of(account: Account) -> Snapshot {
    let mut snapshot = Snapshot::new(account);
    snapshot.playlists = Some(vec![playlist("spotify:playlist:a", "A")]);
    snapshot.shows = Some(vec![show("spotify:show:s")]);
    snapshot.artists = Some(vec![artist("spotify:artist:r")]);
    snapshot.albums = Some(vec![album("spotify:album:r")]);
    snapshot.liked = Some(vec![liked("spotify:track:t")]);
    snapshot
}

/// The window a real launch builds, through the constructor eframe calls.
fn launched(
    cached: Cached,
    snapshots: SnapshotSync,
) -> (TakofyApp, tokio::sync::mpsc::UnboundedReceiver<Command>) {
    let (sender, updates) = channel();
    // Nothing will ever arrive; a disconnected channel is what the window
    // already tolerates.
    drop(sender);
    let (commands, received) = tokio::sync::mpsc::unbounded_channel();
    let cc = eframe::CreationContext::_new_kittest(window_context());
    let app = TakofyApp::new(
        &cc,
        UiEnd { updates, commands },
        Loaded::Absent,
        PrefsSync::off(),
        cached,
        snapshots,
    );
    (app, received)
}

/// A sink that keeps every copy it is handed.
fn recording() -> (
    SnapshotLog,
    impl Fn(&Snapshot) -> std::io::Result<()> + Send + 'static,
) {
    let log: SnapshotLog = Arc::new(Mutex::new(Vec::new()));
    let sink = {
        let log = Arc::clone(&log);
        move |snapshot: &Snapshot| {
            log.lock().expect("the sink log").push(snapshot.clone());
            Ok(())
        }
    };
    (log, sink)
}

/// A sync whose file holds `written`, saving through `sink`.
fn synced(
    written: Option<Snapshot>,
    sink: impl Fn(&Snapshot) -> std::io::Result<()> + Send + 'static,
) -> SnapshotSync {
    SnapshotSync::for_test(
        written,
        SnapshotWriter::with_sink(sink, Duration::from_secs(2)),
    )
}

/// The shortest wake `logic` asks for on `ctx`.
///
/// The request is made outside a pass, and a pass's output starts its delay
/// again, so egui's own callback is what observes it.
fn logic_wake(app: &mut TakofyApp, ctx: &egui::Context) -> Duration {
    let seen = Arc::new(Mutex::new(Duration::MAX));
    {
        let seen = Arc::clone(&seen);
        ctx.set_request_repaint_callback(move |info| {
            let mut seen = seen.lock().expect("the wake log");
            if info.delay < *seen {
                *seen = info.delay;
            }
        });
    }
    app.logic(ctx, &mut eframe::Frame::_new_kittest());
    *seen.lock().expect("the wake log")
}

#[test]
fn the_copy_is_drawn_before_the_first_frame() {
    let copy = copy_of(account("dana"));
    let (app, _received) = launched(Cached::Found(copy.clone()), SnapshotSync::off());

    assert_eq!(
        app.library.playlists,
        Load::Ready(copy.playlists.clone().expect("playlists"))
    );
    assert_eq!(
        app.library.followed,
        Load::Ready(copy.shows.clone().expect("shows"))
    );
    assert_eq!(
        app.library.followed_artists,
        Load::Ready(copy.artists.clone().expect("artists"))
    );
    assert_eq!(
        app.library.saved_albums,
        Load::Ready(copy.albums.clone().expect("albums"))
    );
    assert_eq!(app.library.likes.kept(), copy.liked);
    assert!(
        !app.library
            .can_remove(takofy_ui::sidebar_kind::SidebarKind::Playlists),
        "the copy carries no write authority"
    );
}

#[test]
fn a_miss_is_todays_library() {
    for cached in [
        Cached::Absent,
        Cached::Refused("line 1: not a takofy library".to_owned()),
        Cached::Unreadable,
    ] {
        let (app, _received) = launched(cached, SnapshotSync::off());
        assert_eq!(app.library.playlists, Load::Idle);
        assert_eq!(app.library.followed, Load::Idle);
        assert_eq!(app.library.followed_artists, Load::Idle);
        assert_eq!(app.library.saved_albums, Load::Idle);
        assert_eq!(app.library.likes.kept(), None);
        assert_eq!(app.library.page(), Some(&takofy_ui::route::Page::Home));
    }
}

#[test]
fn the_sign_in_card_discards_the_copy() {
    for state in [
        SignIn::NeedsAuthorization,
        SignIn::Authorizing,
        SignIn::Failed(Failure::Other),
    ] {
        let (mut app, _received) =
            launched(Cached::Found(copy_of(account("dana"))), SnapshotSync::off());
        app.state = state;
        logic_frame(&mut app);

        assert_eq!(app.library.playlists, Load::Idle, "at {state:?}");
        assert_eq!(app.library.followed, Load::Idle, "at {state:?}");
        assert_eq!(app.library.saved_albums, Load::Idle, "at {state:?}");
        assert_eq!(app.library.likes.kept(), None, "at {state:?}");
    }
}

#[test]
fn a_resumed_sign_in_keeps_the_copy_until_the_account_answers() {
    let copy = copy_of(account("dana"));
    let (mut app, _received) = launched(Cached::Found(copy.clone()), SnapshotSync::off());
    app.state = SignIn::SignedIn;

    logic_frame(&mut app);
    assert_eq!(
        app.library.playlists,
        Load::Ready(copy.playlists.clone().expect("playlists"))
    );

    app.library.apply(&Update::Account(account("dana")));
    assert_eq!(
        app.library.playlists,
        Load::Ready(copy.playlists.clone().expect("playlists")),
        "the account the copy belongs to keeps it"
    );
    assert_eq!(
        app.library.snapshot().map(|snapshot| snapshot.account),
        Some(account("dana")),
        "and a matching account is what makes the copy saveable"
    );
}

#[test]
fn a_mismatched_account_discards_the_copy() {
    let (mut app, _received) =
        launched(Cached::Found(copy_of(account("dana"))), SnapshotSync::off());
    app.state = SignIn::SignedIn;
    logic_frame(&mut app);

    app.library.apply(&Update::Account(account("sam")));

    assert_eq!(app.library.playlists, Load::Idle);
    assert_eq!(app.library.followed, Load::Idle);
    assert_eq!(app.library.followed_artists, Load::Idle);
    assert_eq!(app.library.saved_albums, Load::Idle);
    assert_eq!(app.library.likes.kept(), None);
    assert!(
        !app.library
            .snapshot()
            .expect("the new account is known")
            .has_rows(),
        "nothing of the old account's is left to save"
    );
}

#[test]
fn logic_saves_after_quiet_and_asks_for_the_wake() {
    let copy = copy_of(account("dana"));
    let (log, sink) = recording();
    let (mut app, _received) = test_window_restored(
        Cached::Found(copy.clone()),
        synced(None, sink),
        Loaded::Absent,
        PrefsSync::off(),
    );
    app.state = SignIn::SignedIn;
    app.library.apply(&Update::Account(account("dana")));
    // The copy is drawn; a read's answer is what arms the save window.
    app.library.apply(&Update::SavedAlbums(Vec::new()));

    let ctx = window_context();
    let delay = logic_wake(&mut app, &ctx);
    assert!(
        delay <= QUIET,
        "a pending copy must wake the window within its quiet time: {delay:?}"
    );
    assert!(
        log.lock().expect("the sink log").is_empty(),
        "the copy is armed, not written"
    );

    // The call the wake's frame makes, two seconds on: logic reads the wall
    // clock, so the test hands the pass the instant it cannot wait for.
    let generation = app.library.generation();
    let expected = app_copy(&copy, &[]);
    let again = app
        .snapshots
        .observe(Instant::now() + QUIET, generation, || {
            app.library.snapshot()
        });
    assert_eq!(again, None, "quiet has passed, so the copy is written");
    drop(app);

    assert_eq!(
        *log.lock().expect("the sink log"),
        vec![expected],
        "quiet writes exactly the copy the library holds"
    );
}

/// The copy the library builds from `restored` once `albums` replaced its own.
fn app_copy(restored: &Snapshot, albums: &[SavedAlbum]) -> Snapshot {
    let mut snapshot = restored.clone();
    snapshot.albums = Some(albums.to_vec());
    snapshot
}

#[test]
fn on_exit_flushes_the_copy() {
    let copy = copy_of(account("dana"));
    let (log, sink) = recording();
    let (mut app, _received) = test_window_restored(
        Cached::Found(copy.clone()),
        synced(None, sink),
        Loaded::Absent,
        PrefsSync::off(),
    );
    app.library.apply(&Update::Account(account("dana")));

    App::on_exit(&mut app, None);
    drop(app);

    assert_eq!(
        *log.lock().expect("the sink log"),
        vec![copy],
        "close writes the copy no frame had time to settle"
    );
}

#[test]
fn an_in_flight_write_at_exit_saves_nothing() {
    let (log, sink) = recording();
    let (mut app, _received) = test_window_restored(
        Cached::Found(copy_of(account("dana"))),
        synced(None, sink),
        Loaded::Absent,
        PrefsSync::off(),
    );
    app.library.apply(&Update::Account(account("dana")));
    assert!(
        app.library.likes.request(true),
        "arrange: the liked read must be in flight"
    );
    app.library.apply(&Update::LikedTracks(Vec::new()));
    assert!(
        app.library
            .likes
            .set(&["spotify:track:t".to_owned()], true)
            .is_some(),
        "arrange: a like write must be in flight"
    );

    App::on_exit(&mut app, None);
    drop(app);

    assert!(
        log.lock().expect("the sink log").is_empty(),
        "the rows on screen are what a write is replacing"
    );
}

/// A live playlists reply that may be written through, so a row edit can drop
/// its cover and make the cover read due.
fn writable(uri: &str, revision: &Revision) -> PlaylistEntry {
    PlaylistEntry {
        access: Some(PlaylistAccess {
            revision: revision.clone(),
            editable: true,
        }),
        tracks: Some(1),
        ..playlist(uri, uri)
    }
}

#[test]
fn a_restored_window_asks_nothing_before_the_session_answers() {
    let revision = Revision::from_wire(b"r").expect("a revision");
    let mut restored = copy_of(account("dana"));
    restored.playlists = Some(vec![writable("spotify:playlist:a", &revision)]);
    let (mut app, mut received) = test_window_restored(
        Cached::Found(restored),
        SnapshotSync::off(),
        Loaded::Absent,
        PrefsSync::off(),
    );
    app.state = SignIn::Starting;
    // The arming route a live window uses: a row edit drops the row's cover, so
    // the read that replaces it is due while the playlists are still the copy's.
    assert!(
        app.library
            .add_tracks(0, &["spotify:track:x".to_owned()])
            .is_some(),
        "arrange: a row edit must be in flight"
    );
    app.library.apply(&Update::TrackAdded {
        playlist: "spotify:playlist:a".to_owned(),
        revision: revision.clone(),
    });

    logic_frame(&mut app);
    assert!(
        take_commands(&mut received).is_empty(),
        "no read may name a restored row, nor may the watch follow one"
    );

    app.library.apply(&Update::Playlists(vec![writable(
        "spotify:playlist:a",
        &revision,
    )]));
    logic_frame(&mut app);
    assert_eq!(
        take_commands(&mut received),
        vec![
            Command::LoadTracks {
                playlist: "spotify:playlist:a".to_owned(),
            },
            Command::WatchDevices,
        ],
        "the session's own list releases exactly the due cover read and the watch"
    );
}

#[test]
fn home_asks_nothing_over_the_restored_copy() {
    let (mut app, mut received) =
        launched(Cached::Found(copy_of(account("dana"))), SnapshotSync::off());
    assert_eq!(
        app.library.page(),
        Some(&takofy_ui::route::Page::Home),
        "arrange: the window opens on Home, whose shelves ask"
    );
    assert_eq!(app.state, SignIn::Starting);

    logic_frame(&mut app);

    let sent = take_commands(&mut received);
    assert!(
        sent.is_empty(),
        "Home's shelves wait for live rows rather than reading over the copy: {sent:?}"
    );
}

#[test]
fn a_live_reply_opens_the_due_cover_reads_and_the_watch() {
    let (mut app, mut received) = test_window_restored(
        Cached::Found(copy_of(account("dana"))),
        SnapshotSync::off(),
        Loaded::Absent,
        PrefsSync::off(),
    );
    app.state = SignIn::Starting;
    let revision = Revision::from_wire(b"r").expect("a revision");
    app.library.apply(&Update::Playlists(vec![writable(
        "spotify:playlist:a",
        &revision,
    )]));
    assert_eq!(
        app.library.add_tracks(0, &["spotify:track:x".to_owned()]),
        Some(Command::AddTracks {
            playlist: "spotify:playlist:a".to_owned(),
            revision: revision.clone(),
            from_index: 1,
            tracks: vec!["spotify:track:x".to_owned()],
        }),
        "arrange: a row edit must be in flight"
    );
    app.library.apply(&Update::TrackAdded {
        playlist: "spotify:playlist:a".to_owned(),
        revision,
    });

    logic_frame(&mut app);
    let sent = take_commands(&mut received);
    assert!(
        sent.contains(&Command::LoadTracks {
            playlist: "spotify:playlist:a".to_owned(),
        }),
        "the dropped cover is asked for once the list is live: {sent:?}"
    );
    assert!(
        sent.contains(&Command::WatchDevices),
        "the watch follows the live list too: {sent:?}"
    );
}

#[test]
fn a_restored_window_sends_no_followed_read_before_library_ready() {
    let (mut app, mut received) = test_window_restored(
        Cached::Found(copy_of(account("dana"))),
        SnapshotSync::off(),
        Loaded::Absent,
        PrefsSync::off(),
    );
    app.state = SignIn::SignedIn;
    app.library.apply(&Update::Account(account("dana")));

    logic_frame(&mut app);
    let sent = take_commands(&mut received);
    assert!(
        !sent.contains(&Command::LoadFollowedShows),
        "the followed list waits for the walk: {sent:?}"
    );
    assert!(
        !sent.contains(&Command::LoadFollowedArtists),
        "the followed artists wait for the walk: {sent:?}"
    );
    assert!(
        !sent.contains(&Command::LoadSavedAlbums),
        "the saved albums wait for the walk: {sent:?}"
    );
    assert!(
        !sent.contains(&Command::LoadLikedTracks),
        "the liked set waits for the walk: {sent:?}"
    );
}
