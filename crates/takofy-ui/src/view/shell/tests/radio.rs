//! A row's or a band's radio, from the menu to the command the shell reports.

use super::super::*;
use super::test_support::*;

use crate::menu::{Device, Door, LibraryItem};
use crate::message::Update;
use crate::player::Transport;
use crate::queue::Queue;
use crate::sidebar_kind::SidebarKind;
use takofy_model::TrackEntry;
use takofy_model::home::{SeedKind, StationSeed};

const PLAYLIST_ROW: &str = "spotify:playlist:row";
const ALBUM_ROW: &str = "spotify:album:row";
const ARTIST_ROW: &str = "spotify:artist:row";

/// One row of `kind`, named after the address that kind is listed by.
fn row(kind: SidebarKind, uri: &str) -> LibraryItem {
    LibraryItem {
        kind,
        uri: uri.to_owned(),
        name: "Radio Row".to_owned(),
    }
}

/// The rights a row the library already holds and the user may remove offers.
fn rights() -> crate::verbs::LibraryRights {
    crate::verbs::LibraryRights {
        renamable: false,
        removable: true,
        saved: Some(true),
    }
}

/// A door beside the row, which is where a `⋯` hangs its menu.
fn door() -> Door {
    Door {
        anchor: crate::menu::Anchor::Button(Rect::from_min_size(
            egui::Pos2::new(60.0, 300.0),
            Vec2::new(28.0, 28.0),
        )),
        opener: egui::Id::unique("radio-door"),
        device: Device::Keyboard,
    }
}

/// Opens the menu a request posts, as a keyboard door does.
fn open(window: &mut Window, request: crate::menu::Request) {
    assert!(
        window.menu.open(request, Gate::default()),
        "the menu opened"
    );
}

/// Opens the menu a sidebar row's door posts.
fn open_rows_menu(window: &mut Window, row: LibraryItem) {
    let page = window.library.page().cloned();
    let request =
        crate::verbs::library_request(row, door(), rights(), page, crate::naming::Surface::Row);
    open(window, request);
}

/// Opens the menu a band's door posts, in the shape its own page builds it.
fn open_band_menu(window: &mut Window, row: LibraryItem) {
    let page = window.library.page().cloned();
    let request = if row.kind == SidebarKind::Artists {
        crate::verbs::artist_request(row, door(), false, page)
    } else {
        crate::verbs::library_request(row, door(), rights(), page, crate::naming::Surface::Band)
    };
    open(window, request);
}

/// Moves the menu to its radio row and fires it.
fn choose_radio(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
) -> Option<Command> {
    let at = window
        .menu
        .items()
        .iter()
        .position(|item| item.verb == Verb::StartRadio)
        .expect("the menu offers Start radio");
    for _ in 0..at {
        assert_eq!(
            press(window, transport, queue, egui::Key::ArrowDown),
            None,
            "an arrow key reported a command"
        );
    }
    press(window, transport, queue, egui::Key::Enter)
}

/// One row of a station, whose address is the kind a radio continues from.
fn radio_row(index: usize) -> TrackEntry {
    TrackEntry {
        uri: format!("spotify:track:t{index}"),
        ..track(index, None)
    }
}

/// A radio's rows, `count` of them, as a station answers them.
fn radio_rows(count: usize) -> Vec<TrackEntry> {
    (0..count).map(radio_row).collect()
}

#[test]
fn a_sidebar_rows_radio_starts_its_stations() {
    let cases = [
        (SidebarKind::Playlists, SeedKind::Playlist, PLAYLIST_ROW),
        (SidebarKind::Albums, SeedKind::Album, ALBUM_ROW),
        (SidebarKind::Artists, SeedKind::Artist, ARTIST_ROW),
    ];
    for (kind, seed_kind, uri) in cases {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        open_rows_menu(&mut window, row(kind, uri));
        settle_menu(&mut window, &mut transport, &mut queue);

        assert_eq!(
            choose_radio(&mut window, &mut transport, &mut queue),
            Some(Command::StartRadio {
                seed: StationSeed {
                    kind: seed_kind,
                    uri: uri.to_owned(),
                },
            }),
            "{kind:?}"
        );
    }
}

#[test]
fn a_bands_radio_starts_its_stations() {
    let cases = [
        (SidebarKind::Playlists, SeedKind::Playlist, PLAYLIST_ROW),
        (SidebarKind::Albums, SeedKind::Album, ALBUM_ROW),
        (SidebarKind::Artists, SeedKind::Artist, ARTIST_ROW),
    ];
    for (kind, seed_kind, uri) in cases {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        open_band_menu(&mut window, row(kind, uri));
        settle_menu(&mut window, &mut transport, &mut queue);

        assert_eq!(
            choose_radio(&mut window, &mut transport, &mut queue),
            Some(Command::StartRadio {
                seed: StationSeed {
                    kind: seed_kind,
                    uri: uri.to_owned(),
                },
            }),
            "{kind:?}"
        );
    }
}

#[test]
fn a_landed_radio_starts_a_queue_that_asks_for_more() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let seed = StationSeed {
        kind: SeedKind::Song,
        uri: "spotify:track:a".to_owned(),
    };
    let _ = window.library.start_radio(seed.clone());
    window.library.apply(&Update::Radio {
        seed: seed.uri.clone(),
        rows: radio_rows(5),
    });

    let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());

    assert!(
        matches!(command, Some(Command::Play { .. })),
        "the frame never started the rows: {command:?}"
    );
    assert_eq!(
        queue.current().map(|entry| entry.uri.as_str()),
        Some("spotify:track:t0"),
        "the shell never started the radio"
    );
    assert!(
        matches!(queue.drift_wants(), Some(Command::ContinueRadio { .. })),
        "a landed radio left nothing asking for more"
    );
}

#[test]
fn a_shelf_play_of_the_same_rows_asks_for_more() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    assert!(window.library.want_play_rows(None, radio_rows(5)));

    let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());

    assert!(
        matches!(command, Some(Command::Play { .. })),
        "the frame never started the rows: {command:?}"
    );
    assert_eq!(
        queue.current().map(|entry| entry.uri.as_str()),
        Some("spotify:track:t0"),
        "the shell never started the play"
    );
    assert!(
        matches!(queue.drift_wants(), Some(Command::ContinueRadio { .. })),
        "a shelf play near its end asked for nothing"
    );
}

#[test]
fn a_podcast_row_has_no_radio_row() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let row = row(SidebarKind::Podcasts, "spotify:show:s");

    for band in [false, true] {
        if band {
            open_band_menu(&mut window, row.clone());
        } else {
            open_rows_menu(&mut window, row.clone());
        }
        settle_menu(&mut window, &mut transport, &mut queue);

        let labels: Vec<&str> = window
            .menu
            .items()
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(labels, ["Copy link", "Remove from library"], "band {band}");
        assert!(
            window
                .menu
                .items()
                .iter()
                .all(|item| item.verb != Verb::StartRadio),
            "a show offered a radio, band {band}"
        );

        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);
        assert!(!window.menu.is_open(), "the menu stayed open");
    }
}
