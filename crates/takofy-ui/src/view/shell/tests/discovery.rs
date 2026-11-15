use super::super::*;
use super::test_support::*;
use crate::menu::{Anchor, Device, Door, Gate, Verb};
use crate::player::Transport;
use crate::queue::Queue;
use crate::sidebar_kind::SidebarKind;
use takofy_model::home::{DiscoveryArtist, DiscoveryCard};

const ARTIST: &str = "spotify:artist:vane";

fn door() -> Door {
    Door {
        anchor: Anchor::Pointer(egui::Pos2::ZERO),
        opener: egui::Id::unique("discovery-more"),
        device: Device::Keyboard,
    }
}

fn row() -> crate::menu::LibraryItem {
    crate::menu::LibraryItem {
        kind: SidebarKind::Artists,
        uri: ARTIST.to_owned(),
        name: "Ilse Vane".to_owned(),
    }
}

/// A discovery card's menu, which is where a hide is offered.
fn hide_request() -> crate::menu::Request {
    let card = DiscoveryCard {
        artist: DiscoveryArtist {
            uri: ARTIST.to_owned(),
            name: "Ilse Vane".to_owned(),
        },
        portrait: None,
        tracks: Vec::new(),
        latest_year: None,
    };
    crate::verbs::discovery_request(&card, door())
}

fn open(window: &mut Window, request: crate::menu::Request) {
    assert!(
        window.menu.open(request, Gate::default()),
        "the menu opened"
    );
}

/// Moves the keyboard to `verb`'s row and fires it, returning the command.
fn choose(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
    verb: Verb,
) -> Option<Command> {
    let at = window
        .menu
        .items()
        .iter()
        .position(|item| item.verb == verb)
        .unwrap_or_else(|| panic!("the menu offers {verb:?}"));
    for _ in 0..at {
        assert_eq!(
            press(window, transport, queue, egui::Key::ArrowDown),
            None,
            "an arrow key reported a command"
        );
    }
    press(window, transport, queue, egui::Key::Enter)
}

#[test]
fn a_hide_row_hides_the_artist_and_reports_no_command() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    window.library.open(Page::Home);
    open(&mut window, hide_request());
    settle_menu(&mut window, &mut transport, &mut queue);

    assert_eq!(
        choose(
            &mut window,
            &mut transport,
            &mut queue,
            Verb::HideFromDiscovery
        ),
        None,
        "a hide touches no wire"
    );
    assert!(
        window.library.hidden().contains(ARTIST),
        "the artist is hidden from every row"
    );
}

#[test]
fn a_show_row_offers_a_hidden_artist_again() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    window.library.hide_artist(ARTIST);
    open(
        &mut window,
        crate::verbs::artist_request(row(), door(), false, true, None),
    );
    settle_menu(&mut window, &mut transport, &mut queue);

    assert_eq!(
        choose(
            &mut window,
            &mut transport,
            &mut queue,
            Verb::ShowInDiscovery
        ),
        None
    );
    assert!(
        !window.library.hidden().contains(ARTIST),
        "the show row put the artist back"
    );
}

#[test]
fn a_bands_follow_asks_once_and_a_second_press_while_it_is_out_asks_nothing() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));

    open(
        &mut window,
        crate::verbs::artist_request(row(), door(), true, false, None),
    );
    settle_menu(&mut window, &mut transport, &mut queue);
    assert_eq!(
        choose(&mut window, &mut transport, &mut queue, Verb::Follow),
        Some(Command::SetFollow {
            artist: ARTIST.to_owned(),
            followed: true,
        })
    );
    assert!(window.library.follow_pending(ARTIST));

    open(
        &mut window,
        crate::verbs::artist_request(row(), door(), true, false, None),
    );
    settle_menu(&mut window, &mut transport, &mut queue);
    assert_eq!(
        choose(&mut window, &mut transport, &mut queue, Verb::Follow),
        None,
        "a second write for the same artist is dropped"
    );
}
