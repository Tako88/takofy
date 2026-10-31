use super::super::*;
use super::test_support::*;

use std::sync::mpsc::channel;
use takofy_model::home::{SeedKind, StationSeed};
use takofy_model::{CoverImage, CoverUrl, TrackEntry};
use takofy_ui::library::Load;
use takofy_ui::message::{Failure, TransportUpdate};

#[test]
fn wall_clock_ms_is_within_a_minute_of_the_system_clock() {
    let system = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after the epoch")
        .as_millis();
    let reading = u128::from(wall_clock_ms());
    assert!(
        system.abs_diff(reading) < 60_000,
        "read {reading} against the system's {system}"
    );
    assert!(reading > 1_704_067_200_000, "a 2024 floor, read {reading}");
}

#[test]
fn drain_applies_every_update_in_order() {
    let (tx, rx) = channel();
    let mut state = SignIn::Starting;
    let mut library = Library::default();
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut devices = Devices::default();
    tx.send(Update::NeedsAuthorization).expect("send");
    tx.send(Update::SignInFailed(Failure::PortBusy))
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
    assert_eq!(state, SignIn::NeedsAuthorization);
}

#[test]
fn drain_feeds_the_library() {
    let (tx, rx) = channel();
    let mut state = SignIn::Starting;
    let mut library = Library::default();
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut devices = Devices::default();
    tx.send(Update::SignedIn).expect("send");
    tx.send(Update::Playlists(Vec::new())).expect("send");

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
    assert_eq!(library.playlists, Load::Ready(Vec::new()));
}

#[test]
fn drain_feeds_the_transport() {
    let (tx, rx) = channel();
    let mut state = SignIn::SignedIn;
    let mut library = Library::default();
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut devices = Devices::default();
    tx.send(Update::Transport(TransportUpdate::Position {
        position_ms: 9,
        paused: false,
    }))
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
    assert_eq!(transport.state, TransportState::Playing);
    assert_eq!(transport.position_ms, 9);
}

#[test]
fn drain_returns_nothing_on_an_empty_channel_without_blocking() {
    let (_tx, rx) = channel();
    let mut state = SignIn::Starting;
    let mut library = Library::default();
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut devices = Devices::default();

    assert!(
        drain(
            &rx,
            &mut state,
            &mut library,
            &mut transport,
            &mut queue,
            &mut devices,
            &mut Art::default(),
            &mut Playclock::at(Instant::now())
        )
        .is_empty(),
        "an empty channel yields no commands"
    );
    assert_eq!(state, SignIn::Starting);
}

#[test]
fn drain_survives_a_disconnected_sender() {
    let (tx, rx) = channel();
    let mut state = SignIn::Starting;
    let mut library = Library::default();
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut devices = Devices::default();
    tx.send(Update::SignedIn).expect("send");
    drop(tx);

    assert!(
        drain(
            &rx,
            &mut state,
            &mut library,
            &mut transport,
            &mut queue,
            &mut devices,
            &mut Art::default(),
            &mut Playclock::at(Instant::now())
        )
        .is_empty()
    );
    assert_eq!(state, SignIn::SignedIn);
    assert!(
        drain(
            &rx,
            &mut state,
            &mut library,
            &mut transport,
            &mut queue,
            &mut devices,
            &mut Art::default(),
            &mut Playclock::at(Instant::now())
        )
        .is_empty()
    );
}

#[test]
fn drain_returns_the_next_play_when_a_track_ends() {
    let (tx, rx) = channel();
    let mut state = SignIn::SignedIn;
    let mut library = Library::default();
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut devices = Devices::default();
    let all = vec![track("a"), track("b")];
    assert_eq!(
        queue.start(&all, 0, None, &mut transport),
        Some(Command::Play {
            uri: "a".to_owned()
        })
    );
    tx.send(Update::Transport(TransportUpdate::Loading))
        .expect("send");
    tx.send(Update::Transport(TransportUpdate::Ended))
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

    assert_eq!(
        commands,
        vec![Command::Play {
            uri: "b".to_owned()
        }]
    );
}

#[test]
fn drain_skips_an_unavailable_track() {
    let (tx, rx) = channel();
    let mut state = SignIn::SignedIn;
    let mut library = Library::default();
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut devices = Devices::default();
    let all = vec![track("a"), track("b")];
    assert_eq!(
        queue.start(&all, 0, None, &mut transport),
        Some(Command::Play {
            uri: "a".to_owned()
        })
    );
    tx.send(Update::Transport(TransportUpdate::Loading))
        .expect("send");
    tx.send(Update::Transport(TransportUpdate::Unavailable))
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

    assert_eq!(
        commands,
        vec![Command::Play {
            uri: "b".to_owned()
        }]
    );
}

/// A queue playing row `index` of `uris`, with a length to jump in.
fn playing(uris: &[&str], index: usize) -> (Queue, Transport) {
    let all: Vec<TrackEntry> = uris.iter().map(|uri| track(uri)).collect();
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let _ = queue.start(&all, index, None, &mut transport);
    transport.state = TransportState::Playing;
    transport.duration_ms = Some(200_000);
    (queue, transport)
}

/// An order the phone sent, expiring when the entry's budget would answer it.
fn order_at(order: Order, expires: Instant) -> Update {
    Update::Remote(RemoteUpdate::Order {
        id: RequestId(9),
        order,
        expires,
    })
}

/// The time an order still has when the window receives it, longer than any
/// test takes to reach `drain`.
const ORDER_LEFT: Duration = Duration::from_secs(5);

fn order(order: Order) -> Update {
    order_at(order, Instant::now() + ORDER_LEFT)
}

/// `now` moved back by `elapsed`, clamped so a young clock cannot underflow.
fn ago(now: Instant, elapsed: Duration) -> Instant {
    now.checked_sub(elapsed).unwrap_or(now)
}

/// A clock whose anchor moved `elapsed` ago.
fn counted(now: Instant, elapsed: Duration) -> Playclock {
    Playclock {
        now,
        since: Some(ago(now, elapsed)),
    }
}

#[test]
fn drain_orders_the_effect_before_its_answer() {
    let (tx, rx) = channel();
    let mut state = SignIn::SignedIn;
    let mut library = Library::default();
    let mut devices = Devices::default();
    let (mut queue, mut transport) = playing(&["a", "b"], 0);
    tx.send(order(Order::Pause)).expect("send");

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

    assert_eq!(
        commands,
        vec![
            Command::Pause,
            Command::Answer {
                id: RequestId(9),
                ok: true
            }
        ],
        "the phone hears about the effect only after it is asked for"
    );
}

#[test]
fn drain_answers_false_when_react_fails() {
    let (tx, rx) = channel();
    let mut state = SignIn::SignedIn;
    let mut library = Library::default();
    let mut devices = Devices::default();
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    tx.send(order(Order::Pause)).expect("send");

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

    assert_eq!(
        commands,
        vec![Command::Answer {
            id: RequestId(9),
            ok: false
        }],
        "nothing loaded is nothing to pause, and the phone is told so"
    );
    assert_eq!(transport.state, TransportState::Idle, "nothing moved");
}

#[test]
fn an_expired_order_is_answered_false_without_any_effect() {
    let (tx, rx) = channel();
    let mut state = SignIn::SignedIn;
    let mut library = Library::default();
    let mut devices = Devices::default();
    let (mut queue, mut transport) = playing(&["a", "b"], 0);
    let now = Instant::now();
    let expired = ago(now, Duration::from_secs(1));
    tx.send(order_at(Order::Pause, expired)).expect("send");

    let commands = drain(
        &rx,
        &mut state,
        &mut library,
        &mut transport,
        &mut queue,
        &mut devices,
        &mut Art::default(),
        &mut Playclock::at(now),
    );

    assert_eq!(
        commands,
        vec![Command::Answer {
            id: RequestId(9),
            ok: false
        }],
        "an order the budget already failed is not carried out"
    );
    assert_eq!(transport.state, TransportState::Playing);
    assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("a"));
}

#[test]
fn drain_hands_react_the_extrapolated_position() {
    for (elapsed, expected) in [
        (
            Duration::ZERO,
            Command::Play {
                uri: "a".to_owned(),
            },
        ),
        (Duration::from_secs(2), Command::Seek { position_ms: 0 }),
    ] {
        let (tx, rx) = channel();
        let mut state = SignIn::SignedIn;
        let mut library = Library::default();
        let mut devices = Devices::default();
        let (mut queue, mut transport) = playing(&["a", "b"], 1);
        transport.position_ms = 3_000;
        tx.send(order(Order::Prev)).expect("send");

        let now = Instant::now();
        let commands = drain(
            &rx,
            &mut state,
            &mut library,
            &mut transport,
            &mut queue,
            &mut devices,
            &mut Art::default(),
            &mut counted(now, elapsed),
        );

        assert_eq!(
            commands.first(),
            Some(&expected),
            "after {elapsed:?} of play, the position is what decides"
        );
    }
}

#[test]
fn a_position_event_earlier_in_the_drain_restamps_the_play_clock() {
    let (tx, rx) = channel();
    let mut state = SignIn::SignedIn;
    let mut library = Library::default();
    let mut devices = Devices::default();
    let (mut queue, mut transport) = playing(&["a", "b"], 1);
    // Three seconds in, with an anchor two seconds old: the drain's own
    // start would read five and restart, where the event's instant steps
    // back.
    tx.send(Update::Transport(TransportUpdate::Position {
        position_ms: 3_000,
        paused: false,
    }))
    .expect("send");
    tx.send(order(Order::Prev)).expect("send");

    let now = Instant::now();
    let commands = drain(
        &rx,
        &mut state,
        &mut library,
        &mut transport,
        &mut queue,
        &mut devices,
        &mut Art::default(),
        &mut counted(now, Duration::from_secs(2)),
    );

    assert_eq!(
        commands.first(),
        Some(&Command::Play {
            uri: "a".to_owned()
        }),
        "the position that moved restamps the clock the order reads"
    );
}

#[test]
fn logic_requests_the_followed_list_once_the_session_is_ready() {
    // `logic()` only forwards this return value into the command queue, so
    // the contract is `Library::followed_request` itself.
    let mut library = Library::default();
    assert_eq!(library.followed_request(), None, "nothing has loaded yet");

    library.apply(&Update::LibraryReady);
    library.apply(&Update::Playlists(Vec::new()));
    assert_eq!(library.followed_request(), Some(Command::LoadFollowedShows));
    assert_eq!(library.followed, Load::Loading);
    assert_eq!(
        library.followed_request(),
        None,
        "the load is in flight, so the next frame sends nothing"
    );
}

/// The artists and albums lists are want-driven like the shows: one send
/// per chip press, and none before the chip is the chosen one.
#[test]
fn logic_requests_the_artists_and_albums_for_the_chip_that_names_them() {
    let mut library = Library::default();
    assert_eq!(library.followed_artists_request(), None);
    assert_eq!(library.saved_albums_request(), None);

    // All draws every kind, so a ready session under it asks for both.
    library.apply(&Update::LibraryReady);
    library.apply(&Update::Playlists(Vec::new()));
    assert_eq!(
        library.followed_artists_request(),
        Some(Command::LoadFollowedArtists)
    );
    assert_eq!(
        library.saved_albums_request(),
        Some(Command::LoadSavedAlbums)
    );

    // A chip that draws only playlists names neither list, so a library
    // that has not read them yet stays idle under it.
    let mut narrowed = Library::default();
    narrowed.apply(&Update::LibraryReady);
    narrowed.apply(&Update::Playlists(Vec::new()));
    assert!(
        narrowed
            .root_filter
            .choose(takofy_ui::library_filter::RootKind::Playlists)
    );
    assert_eq!(narrowed.followed_artists_request(), None);
    assert_eq!(narrowed.saved_albums_request(), None);

    // A chip of its own asks for its own list, once.
    let mut chip = Library::default();
    chip.apply(&Update::LibraryReady);
    chip.apply(&Update::Playlists(Vec::new()));
    assert!(
        chip.root_filter
            .choose(takofy_ui::library_filter::RootKind::Artists)
    );
    assert_eq!(
        chip.followed_artists_request(),
        Some(Command::LoadFollowedArtists)
    );
    assert_eq!(chip.followed_artists, Load::Loading);
    assert_eq!(
        chip.followed_artists_request(),
        None,
        "the load is in flight, so the next frame sends nothing"
    );

    assert!(
        chip.root_filter
            .choose(takofy_ui::library_filter::RootKind::Albums)
    );
    assert_eq!(chip.saved_albums_request(), Some(Command::LoadSavedAlbums));
    assert_eq!(chip.saved_albums_request(), None);

    // The sidebar's chips are the other surface that names the same lists.
    let mut sidebar = Library::default();
    sidebar.apply(&Update::LibraryReady);
    sidebar.apply(&Update::Playlists(Vec::new()));
    assert!(
        sidebar
            .sidebar_kind
            .choose(takofy_ui::sidebar_kind::SidebarKind::Artists)
    );
    assert_eq!(
        sidebar.followed_artists_request(),
        Some(Command::LoadFollowedArtists)
    );
    assert!(
        sidebar
            .sidebar_kind
            .choose(takofy_ui::sidebar_kind::SidebarKind::Albums)
    );
    assert_eq!(
        sidebar.saved_albums_request(),
        Some(Command::LoadSavedAlbums)
    );
}

#[test]
fn drain_routes_a_failed_cover_into_the_store() {
    let (tx, rx) = channel();
    let mut state = SignIn::SignedIn;
    let mut library = Library::default();
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut devices = Devices::default();
    let mut art = Art::default();
    let now = Instant::now();
    let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url");
    // The URL must be known before its failure arrives, as it is in a frame:
    // the draw looks it, sends the fetch, and the failure answers it.
    assert_eq!(
        art.look(Some(&set(&url)), 64, now),
        takofy_ui::cover::Cover::Loading
    );
    let _ = art.requests(now);
    tx.send(Update::CoverFailed { url: url.clone() })
        .expect("send");

    let commands = drain(
        &rx,
        &mut state,
        &mut library,
        &mut transport,
        &mut queue,
        &mut devices,
        &mut art,
        &mut Playclock::at(now),
    );

    assert!(commands.is_empty());
    assert_eq!(
        art.look(Some(&set(&url)), 64, now),
        takofy_ui::cover::Cover::Failed
    );
}

#[test]
fn drain_routes_a_cover_into_the_store() {
    let (tx, rx) = channel();
    let mut state = SignIn::SignedIn;
    let mut library = Library::default();
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut devices = Devices::default();
    let mut art = Art::default();
    let now = Instant::now();
    let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url");
    art.look(Some(&set(&url)), 64, now);
    let _ = art.requests(now);
    tx.send(Update::Cover {
        url: url.clone(),
        image: CoverImage::new(1, 1, vec![0; 4]).expect("a 1x1 image"),
    })
    .expect("send");

    let _ = drain(
        &rx,
        &mut state,
        &mut library,
        &mut transport,
        &mut queue,
        &mut devices,
        &mut art,
        &mut Playclock::at(now),
    );
    let ctx = egui::Context::default();
    art.upload(&ctx, Tokens::DARK.palette.tint.alpha_floor);

    assert!(matches!(
        art.look(Some(&set(&url)), 64, now),
        takofy_ui::cover::Cover::Ready(_)
    ));
}

#[test]
fn logic_asks_for_the_shelf_play_once() {
    let (mut app, mut received) = idle_window();
    assert!(app.library.want_play(
        "spotify:playlist:a",
        takofy_ui::sidebar_kind::SidebarKind::Playlists
    ));

    logic_frame(&mut app);
    let sent = take_commands(&mut received);
    assert_eq!(
        sent.iter()
            .filter(|command| matches!(
                command,
                Command::LoadTracks { playlist } if playlist == "spotify:playlist:a"
            ))
            .count(),
        1,
        "the shelf play was not the logic pass's to send: {sent:?}"
    );

    logic_frame(&mut app);
    let sent = take_commands(&mut received);
    assert!(
        !sent.iter().any(|command| matches!(
            command,
            Command::LoadTracks { playlist } if playlist == "spotify:playlist:a"
        )),
        "the shelf play was asked for twice: {sent:?}"
    );
}

/// Home's related shelf reads the followed artists, so the page asks for them
/// whatever chip the root was left on: the chip that would ask is not on screen.
#[test]
fn home_asks_for_the_artists_whatever_chip_is_chosen() {
    let mut library = Library::starting_home(1_700_000_000_000);
    assert_eq!(
        library.followed_artists_request(),
        None,
        "nothing has named a signed-in window yet"
    );

    library.apply(&Update::LibraryReady);
    library.apply(&Update::Playlists(Vec::new()));
    assert!(
        library
            .root_filter
            .choose(takofy_ui::library_filter::RootKind::Playlists)
    );
    assert_eq!(
        library.followed_artists_request(),
        Some(Command::LoadFollowedArtists),
        "Home reads the artists under a chip that names none"
    );
    assert_eq!(
        library.followed_artists_request(),
        None,
        "the load is in flight, so the next frame sends nothing"
    );
}

/// A page that is not Home keeps the chip's own gate: the list is only read when
/// the user asked for a chip that draws it.
#[test]
fn a_page_that_is_not_home_keeps_the_artists_gate() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(Vec::new()));
    assert!(
        library
            .root_filter
            .choose(takofy_ui::library_filter::RootKind::Playlists)
    );

    assert_eq!(library.followed_artists_request(), None);
    assert_eq!(library.followed_artists, Load::Idle);
}

#[test]
fn drain_lands_a_radio_and_leaves_the_start_to_the_frame() {
    let (tx, rx) = channel();
    let mut state = SignIn::SignedIn;
    let mut library = Library::default();
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut devices = Devices::default();
    let seed = StationSeed {
        kind: SeedKind::Song,
        uri: "spotify:track:a".to_owned(),
    };
    let _ = library.start_radio(seed);
    tx.send(Update::Radio {
        seed: "spotify:track:a".to_owned(),
        rows: vec![track("spotify:track:a")],
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

    assert!(
        commands.is_empty(),
        "drain started the queue itself instead of the frame"
    );
    assert!(queue.current().is_none(), "drain moved the queue");
    assert!(
        library.has_landed_play(),
        "the rows never reached the library"
    );
}
