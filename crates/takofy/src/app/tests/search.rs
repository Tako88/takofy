use super::super::*;
use super::test_support::*;

use eframe::App;
use std::sync::mpsc::channel;
use takofy_ui::route::Page;

/// Types `text` into the field, as one frame of the real path would.
fn type_search(library: &mut Library, text: &str) {
    library.search.text = text.to_owned();
    let input = takofy_ui::search::KeyInput {
        typed: true,
        focused: true,
        ..takofy_ui::search::KeyInput::default()
    };
    let _ = library.search.step(input);
}

#[test]
fn drain_feeds_the_search_state() {
    let (tx, rx) = channel();
    let mut state = SignIn::SignedIn;
    let mut library = Library::default();
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut devices = Devices::default();
    type_search(&mut library, "never gonna");
    assert_eq!(
        library.search.request(None),
        Some(Command::Search {
            query: "never gonna".to_owned()
        })
    );
    tx.send(Update::Search {
        query: "never gonna".to_owned(),
        tracks: vec![track("spotify:track:a")],
    })
    .expect("send");

    let commands = drain(
        &rx,
        &mut state,
        &mut library,
        &mut transport,
        &mut queue,
        &mut devices,
        &mut Art::default(),
        &mut Playclock::at(Instant::now()),
    );

    assert!(commands.is_empty());
    let listing = library
        .search
        .listing()
        .expect("a drained reply reaches the search state");
    assert_eq!(listing.answer, &[track("spotify:track:a")]);
    assert_eq!(listing.shape.len(), 1);
}

#[test]
fn logic_asks_for_the_typed_query_once() {
    let (mut app, mut received) = test_window();
    type_search(&mut app.library, "never gonna");

    logic_frame(&mut app);
    assert_eq!(
        received.try_recv().expect("the search read"),
        Command::Search {
            query: "never gonna".to_owned()
        }
    );
    logic_frame(&mut app);
    assert!(
        received.try_recv().is_err(),
        "one read on the wire at a time, so a burst cannot queue twenty"
    );
}

#[test]
fn logic_asks_for_the_portraits_after_a_search_reply_with_artists() {
    let (mut app, mut received) = test_window();
    app.state = SignIn::SignedIn;
    type_search(&mut app.library, "a");
    logic_frame(&mut app);
    assert_eq!(
        received.try_recv().expect("the search read"),
        Command::Search {
            query: "a".to_owned()
        }
    );

    let mut first = track("t0");
    first.artists = "One".to_owned();
    first.artist_uris = vec!["spotify:artist:1".to_owned()];
    first.artist_names = vec!["One".to_owned()];
    let mut second = track("t1");
    second.artists = "Two".to_owned();
    second.artist_uris = vec!["spotify:artist:2".to_owned()];
    second.artist_names = vec!["Two".to_owned()];
    app.library.apply(&Update::Search {
        query: "a".to_owned(),
        tracks: vec![first, second],
    });

    logic_frame(&mut app);
    assert_eq!(
        received.try_recv().expect("the portrait read"),
        Command::LoadPortraits {
            query: "a".to_owned(),
            artists: vec!["spotify:artist:1".to_owned(), "spotify:artist:2".to_owned()],
        }
    );
    logic_frame(&mut app);
    assert!(received.try_recv().is_err(), "one portrait read at a time");
}

#[test]
fn logic_asks_for_the_pages_portraits_once_its_answer_lands() {
    let (mut app, mut received) = test_window();
    app.state = SignIn::SignedIn;
    app.library
        .open(takofy_ui::route::Page::Search("a".to_owned()));

    logic_frame(&mut app);
    assert_eq!(
        received.try_recv().expect("the page's search read"),
        Command::Search {
            query: "a".to_owned()
        }
    );

    let mut tracks = Vec::new();
    for n in 0..3 {
        let mut entry = track(&format!("t{n}"));
        entry.artist_uris = vec![format!("spotify:artist:{n}")];
        entry.artist_names = vec![format!("{n}")];
        tracks.push(entry);
    }
    app.library.apply(&Update::Search {
        query: "a".to_owned(),
        tracks,
    });

    logic_frame(&mut app);
    assert_eq!(
        received.try_recv().expect("the page's portrait read"),
        Command::LoadPortraits {
            query: "a".to_owned(),
            artists: vec![
                "spotify:artist:0".to_owned(),
                "spotify:artist:1".to_owned(),
                "spotify:artist:2".to_owned(),
            ],
        },
        "the page asks for the artists its cards draw"
    );
}

#[test]
fn focus_alone_asks_for_no_search() {
    let (mut app, mut received) = test_window();
    let focused = takofy_ui::search::KeyInput {
        focused: true,
        ..takofy_ui::search::KeyInput::default()
    };
    let _ = app.library.search.step(focused);

    logic_frame(&mut app);
    assert!(
        received.try_recv().is_err(),
        "an empty query fetches nothing: the first character starts a read"
    );
}

/// An open results page is a read the `logic` pass asks for, keyed by the
/// page's own query. It is the seam the route and the page meet at: the
/// field is not involved at all.
#[test]
fn logic_asks_for_the_open_results_page_once() {
    let (mut app, mut received) = test_window();
    let _ = app
        .library
        .open(takofy_ui::route::Page::Search("never gonna".to_owned()));

    logic_frame(&mut app);
    assert_eq!(
        received.try_recv().expect("the page's read"),
        Command::Search {
            query: "never gonna".to_owned()
        }
    );
    logic_frame(&mut app);
    assert!(
        received.try_recv().is_err(),
        "one read on the wire at a time"
    );
}

/// A dropped results page is asked for after the draw, but `logic` has
/// already run this frame: the draw must ask for the next one, or the page
/// sits on its skeleton until the user moves the mouse.
#[test]
fn back_onto_a_dropped_search_page_asks_for_the_frame_that_sends_it() {
    let (mut app, mut received) = test_window();
    app.state = SignIn::SignedIn;
    let _ = app.library.open(Page::Search("never gonna".to_owned()));
    app.library.apply(&Update::Search {
        query: "never gonna".to_owned(),
        tracks: vec![track("spotify:track:a")],
    });
    let _ = app.library.open(Page::Album("spotify:album:a".to_owned()));
    // A second query drops the first page's answer.
    let _ = app.library.open(Page::Search("give you up".to_owned()));
    // Back onto the album, then onto the dropped page, as the topbar does.
    let _ = app.library.back();
    assert_eq!(app.library.back(), None);
    assert_eq!(app.library.page_search_query(), Some("never gonna"));

    let ctx = window_context();
    // Warm up, so egui's own first-pass work is not read as this draw's.
    for _ in 0..2 {
        let mut warm = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        warm.textures_delta.clear();
    }
    let _ = ui_frame(&mut app, &ctx);
    // egui reports a pass's repaint causes on the pass after it, which is
    // the frame this test is about: the one that sends the read.
    let mut wake = ctx.run_ui(egui::RawInput::default(), |_ui| {});
    wake.textures_delta.clear();
    let causes = ctx.repaint_causes();
    assert!(
        causes.iter().any(|cause| cause.file.ends_with("app.rs")),
        "the draw armed a read, so it must ask for the frame that sends it: {causes:?}"
    );

    app.logic(&ctx, &mut eframe::Frame::_new_kittest());
    assert_eq!(
        received.try_recv().expect("the read the draw armed"),
        Command::Search {
            query: "never gonna".to_owned()
        }
    );
}
