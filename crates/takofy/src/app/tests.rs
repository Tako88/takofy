/// The desktop entry is another file that names the app id, and a mismatch
/// is invisible until a compositor shows a second icon in the dock.
#[test]
fn the_desktop_entry_names_the_id_the_window_sets() {
    let entry = include_str!("../../../../packaging/io.github.Tako88.Takofy.desktop");
    let field = |name: &str| {
        entry
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .map(str::trim)
    };
    assert_eq!(field("Icon="), Some(APP_ID));
    assert_eq!(field("StartupWMClass="), Some(APP_ID));
    assert_eq!(field("Exec="), Some("takofy"), "the installed binary");
}

/// The icon is compiled in, so a corrupt one would be a silent blank in the
/// task bar rather than a build failure.
#[test]
fn the_window_icon_decodes_at_the_size_the_packages_install() {
    let icon = window_icon().expect("the bundled icon decodes");
    assert_eq!((icon.width, icon.height), (512, 512));
    assert_eq!(icon.rgba.len(), 512 * 512 * 4);
}

use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};

use eframe::App;
use takofy_model::{
    CoverImage, CoverSet, CoverUrl, Device, DeviceKind, Landing, Order, RemotePlayback,
    RemoteState, RequestId, Status, TrackEntry,
};
use takofy_ui::library::Load;
use takofy_ui::message::{Command, DeviceUpdate, Failure, RemoteUpdate, TransportUpdate};
use takofy_ui::prefs::QUIET;
use takofy_ui::repeat::Repeat;
use takofy_ui::route::Page;

use super::*;
use crate::preferences::PrefsWriter;

fn set(url: &CoverUrl) -> CoverSet {
    CoverSet::single(url.clone())
}

fn track(uri: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: uri.to_owned(),
        artists: String::new(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

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
fn a_new_window_starts_with_the_width_rule_alone() {
    let (app, _) = test_window();
    assert_eq!(app.pane, PanePref::Auto, "a launch hid the pane");
    for width in [layout::WINDOW_DEFAULT[0], 1440.0] {
        assert_eq!(
            layout::regions_with(width, app.pane),
            layout::regions_for(width),
            "{width}"
        );
    }
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
fn logic_requests_the_followed_list_once_playlists_are_ready() {
    // `logic()` only forwards this return value into the command queue, so
    // the contract is `Library::followed_request` itself.
    let mut library = Library::default();
    assert_eq!(library.followed_request(), None, "nothing has loaded yet");

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

    // All draws every kind, so a ready library under it asks for both.
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
fn the_clear_colour_is_the_window_token() {
    let tokens = Tokens::DARK;
    assert_eq!(
        window_clear_color(&tokens),
        tokens.palette.window.to_normalized_gamma_f32()
    );
}

#[test]
fn run_and_return_is_pinned() {
    assert!(native_options().run_and_return);
}

#[test]
fn the_system_font_fallback_stays_off() {
    assert!(
        !native_options().system_font_fallback,
        "a fallback would hide a missing glyph here and show one elsewhere"
    );
}

#[test]
fn the_shipping_chain_keeps_egui_icons() {
    // This binary links eframe, which is what turns on egui's `default_fonts`;
    // a crate depending on egui alone has no builtin fonts at all. So this is
    // the only place the configuration that actually ships can be asserted.
    let defs = takofy_ui::theme::fonts::definitions();
    for (family, chain) in &defs.families {
        assert!(
            chain.iter().any(|name| name == "egui-icons"),
            "{family:?} would leave egui's own icons undrawn"
        );
    }
}

#[test]
fn the_viewport_uses_the_layout_sizes() {
    let options = native_options();
    assert_eq!(
        options.viewport.inner_size,
        Some(egui::vec2(
            layout::WINDOW_DEFAULT[0],
            layout::WINDOW_DEFAULT[1]
        ))
    );
    assert_eq!(
        options.viewport.min_inner_size,
        Some(egui::vec2(layout::WINDOW_MIN[0], layout::WINDOW_MIN[1]))
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
fn a_failed_cover_asks_for_a_wake_at_its_retry() {
    let mut art = Art::default();
    let now = Instant::now();
    let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url");
    art.begin_frame();
    art.look(Some(&set(&url)), 64, now);
    let _ = art.requests(now);
    art.apply(&Update::CoverFailed { url: url.clone() }, now);
    art.begin_frame();
    assert_eq!(
        art.look(Some(&set(&url)), 64, now),
        takofy_ui::cover::Cover::Failed
    );

    let ctx = egui::Context::default();
    // Warm up so egui's own first-frame work (the font atlas and its
    // immediate repaint) is not mistaken for a cover wake.
    for _ in 0..2 {
        let mut warm = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        warm.textures_delta.clear();
    }
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        schedule_wake(ui.ctx(), &art, now);
    });
    output.textures_delta.clear();
    let delay = output
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .expect("the root viewport")
        .repaint_delay;
    assert!(
        delay <= Duration::from_secs(5),
        "a retry in five seconds woke the window in {delay:?}"
    );

    // Nothing pending leaves egui's own default alone. A fresh context is
    // used because the one above is holding the repaint it was asked for.
    let idle = Art::default();
    let ctx = egui::Context::default();
    for _ in 0..2 {
        let mut warm = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        warm.textures_delta.clear();
    }
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        schedule_wake(ui.ctx(), &idle, now);
    });
    output.textures_delta.clear();
    assert_eq!(
        output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .expect("the root viewport")
            .repaint_delay,
        Duration::MAX,
        "an idle window must not be woken"
    );
}

/// A window with no worker behind it, so what `logic` sends can be read
/// back from the command channel.
fn test_window() -> (TakofyApp, tokio::sync::mpsc::UnboundedReceiver<Command>) {
    test_window_with(Loaded::Absent, PrefsSync::off())
}

/// A window restored from `loaded` and saving through `prefs`.
fn test_window_with(
    loaded: Loaded,
    prefs: PrefsSync,
) -> (TakofyApp, tokio::sync::mpsc::UnboundedReceiver<Command>) {
    let (sender, updates) = channel();
    // Nothing will ever arrive; a disconnected channel is what the window
    // already tolerates.
    drop(sender);
    let (commands, received) = tokio::sync::mpsc::unbounded_channel();
    let (transport, queue) = TakofyApp::restored_state(&loaded);
    let app = TakofyApp {
        state: SignIn::default(),
        library: Library::default(),
        transport,
        queue,
        reporter: Reporter::default(),
        devices: Devices::default(),
        list: DeviceList::default(),
        drawer: Drawer::default(),
        pane: PanePref::default(),
        menu: Menu::default(),
        ui_end: UiEnd { updates, commands },
        tokens: Tokens::DARK,
        art: Art::new(),
        since: None,
        prefs,
    };
    (app, received)
}

/// A window whose updates the test can feed, so `logic` drains them.
fn live_window() -> (
    TakofyApp,
    tokio::sync::mpsc::UnboundedReceiver<Command>,
    std::sync::mpsc::Sender<Update>,
) {
    let (mut app, received) = test_window();
    let (sender, updates) = channel();
    app.ui_end.updates = updates;
    (app, received, sender)
}

fn landing(rows: &[&str], index: usize) -> Landing {
    Landing {
        rows: rows.iter().map(|uri| (*uri).to_owned()).collect(),
        index,
        position_ms: 0,
        playing: true,
        context: Some("spotify:playlist:p".to_owned()),
        shuffle: Some(false),
        repeat_context: Some(false),
        repeat_track: Some(false),
        epoch: 1,
    }
}

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

/// Runs one frame of the real `logic` pass, which is where a search read is
/// armed. `logic` is an [`eframe::App`] method, so the frame is the one
/// eframe's own test harness builds.
fn logic_frame(app: &mut TakofyApp) {
    app.logic(
        &egui::Context::default(),
        &mut eframe::Frame::_new_kittest(),
    );
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

#[test]
fn logic_sends_a_report_after_a_landing_drains() {
    let (mut app, mut received, updates) = live_window();
    updates
        .send(Update::Remote(RemoteUpdate::Landed(landing(
            &["spotify:track:a", "spotify:track:b"],
            0,
        ))))
        .expect("landing");

    logic_frame(&mut app);
    let commands = take_commands(&mut received);
    assert!(
        commands.iter().any(|command| matches!(
            command,
            Command::Report(Some(report))
                if report.status == Status::Loading
                    && report
                        .track
                        .as_ref()
                        .map(|track| track.uri.as_str())
                        == Some("spotify:track:a")
        )),
        "a landing is published as it drains: {commands:?}"
    );
}

#[test]
fn logic_sends_a_report_again_on_the_first_position_event() {
    let (mut app, mut received, updates) = live_window();
    updates
        .send(Update::Remote(RemoteUpdate::Landed(landing(
            &["spotify:track:a", "spotify:track:b"],
            0,
        ))))
        .expect("landing");
    logic_frame(&mut app);
    let _ = take_commands(&mut received);

    updates
        .send(Update::Transport(TransportUpdate::Position {
            position_ms: 1_000,
            paused: false,
        }))
        .expect("position");
    logic_frame(&mut app);
    let commands = take_commands(&mut received);
    assert!(
        commands.iter().any(|command| matches!(
            command,
            Command::Report(Some(report)) if report.status == Status::Playing
        )),
        "the first position turns the loading report into playing: {commands:?}"
    );
}

#[test]
fn ui_sends_a_report_when_a_click_changes_the_track() {
    let (mut app, mut received, updates) = live_window();
    updates
        .send(Update::Remote(RemoteUpdate::Landed(landing(
            &["spotify:track:a", "spotify:track:b"],
            0,
        ))))
        .expect("landing");
    logic_frame(&mut app);
    let _ = take_commands(&mut received);

    // The effect a click on the second row has, applied before the draw.
    let _ = app.queue.jump(1, &mut app.transport);
    let ctx = window_context();
    let _ = ui_frame(&mut app, &ctx);

    let commands = take_commands(&mut received);
    assert!(
        commands.iter().any(|command| matches!(
            command,
            Command::Report(Some(report))
                if report
                    .track
                    .as_ref()
                    .map(|track| track.uri.as_str())
                    == Some("spotify:track:b")
        )),
        "the draw's track change is published: {commands:?}"
    );
}

#[test]
fn an_answered_orders_effect_is_published_in_the_same_frame() {
    let (mut app, mut received, updates) = live_window();
    updates
        .send(Update::Remote(RemoteUpdate::Landed(landing(
            &["spotify:track:a", "spotify:track:b"],
            0,
        ))))
        .expect("landing");
    logic_frame(&mut app);
    let _ = take_commands(&mut received);

    let id = RequestId(9);
    updates
        .send(Update::Remote(RemoteUpdate::Order {
            id,
            order: Order::Next(None),
            expires: Instant::now() + Duration::from_secs(5),
        }))
        .expect("order");
    logic_frame(&mut app);

    let commands = take_commands(&mut received);
    assert!(
        commands.contains(&Command::Answer { id, ok: true }),
        "the order is answered: {commands:?}"
    );
    assert!(
        commands.iter().any(|command| matches!(
            command,
            Command::Report(Some(report))
                if report
                    .track
                    .as_ref()
                    .map(|track| track.uri.as_str())
                    == Some("spotify:track:b")
        )),
        "the order's move is published in the frame it was applied: {commands:?}"
    );
}

/// A window's context, set up the way `TakofyApp::new` sets one up.
fn window_context() -> egui::Context {
    let tokens = Tokens::DARK;
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&tokens));
    ctx
}

/// One draw pass, as eframe runs it between two `logic` passes.
fn ui_frame(app: &mut TakofyApp, ctx: &egui::Context) -> egui::FullOutput {
    let mut frame = eframe::Frame::_new_kittest();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(layout::WINDOW_DEFAULT[0], layout::WINDOW_DEFAULT[1]),
        )),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| app.ui(ui, &mut frame));
    output.textures_delta.clear();
    output
}

type PrefsLog = Arc<Mutex<Vec<Prefs>>>;

/// A sink that keeps every value it is handed.
fn recording() -> (
    PrefsLog,
    impl Fn(&Prefs) -> std::io::Result<()> + Send + 'static,
) {
    let log: PrefsLog = Arc::new(Mutex::new(Vec::new()));
    let sink = {
        let log = Arc::clone(&log);
        move |prefs: &Prefs| {
            log.lock().expect("the sink log").push(prefs.clone());
            Ok(())
        }
    };
    (log, sink)
}

/// A sync whose file already holds `written`, saving through `sink`.
fn synced(
    written: Prefs,
    sink: impl Fn(&Prefs) -> std::io::Result<()> + Send + 'static,
) -> PrefsSync {
    PrefsSync::for_test(
        written,
        PrefsWriter::with_sink(sink, Duration::from_secs(2)),
    )
}

fn found(text: &str) -> Loaded {
    Loaded::Found(Prefs::parse(text).expect("valid prefs"))
}

/// The volume commands the frames since the last call sent.
fn volume_frames(received: &mut tokio::sync::mpsc::UnboundedReceiver<Command>) -> Vec<u16> {
    let mut levels = Vec::new();
    while let Ok(command) = received.try_recv() {
        if let Command::Volume { percent } = command {
            levels.push(percent);
        }
    }
    levels
}

#[test]
fn restored_state_builds_the_transport_and_queue() {
    let (transport, queue) =
        TakofyApp::restored_state(&found("volume=40\nmute=false\nshuffle=true\nrepeat=all\n"));
    assert_eq!(
        transport.volume.level(),
        40,
        "the saved level seeds the bar"
    );
    assert!(!transport.volume.muted(), "the file was not muted");
    assert!(queue.shuffle(), "the saved shuffle seeds the queue");
    assert_eq!(queue.repeat(), Repeat::All, "the saved repeat seeds it too");
}

#[test]
fn a_restored_volume_is_sent_once_on_the_first_frame() {
    let (mut app, mut received) = test_window_with(found("volume=40\n"), PrefsSync::off());
    let ctx = window_context();
    let _ = ui_frame(&mut app, &ctx);
    assert_eq!(
        volume_frames(&mut received),
        vec![40],
        "a remembered level reaches the player on the first frame"
    );
    let _ = ui_frame(&mut app, &ctx);
    assert!(
        volume_frames(&mut received).is_empty(),
        "one change is one command"
    );
}

#[test]
fn a_restored_mute_sends_zero() {
    let (mut app, mut received) =
        test_window_with(found("volume=70\nmute=true\n"), PrefsSync::off());
    let ctx = window_context();
    let _ = ui_frame(&mut app, &ctx);
    assert_eq!(
        volume_frames(&mut received),
        vec![0],
        "a remembered mute silences the player"
    );
    assert_eq!(
        app.transport.volume.level(),
        70,
        "the bar keeps the level behind the lid"
    );
}

#[test]
fn default_prefs_send_no_volume_command() {
    for loaded in [
        Loaded::Absent,
        Loaded::Refused("line 3: bad value for `repeat`".to_owned()),
        Loaded::Unreadable,
    ] {
        let (mut app, mut received) = test_window_with(loaded, PrefsSync::off());
        let ctx = window_context();
        let _ = ui_frame(&mut app, &ctx);
        assert!(
            volume_frames(&mut received).is_empty(),
            "nothing is owed when the file named no level"
        );
    }
}

/// The delay the window asked for, which is egui's own first passes' work
/// until the app stops asking for anything.
fn root_delay(output: &egui::FullOutput) -> Duration {
    output
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .expect("the root viewport")
        .repaint_delay
}

/// A signed-in window with nothing loading, so the only thing that can ask
/// for a frame is a value on its way to the file.
fn idle_window() -> (TakofyApp, tokio::sync::mpsc::UnboundedReceiver<Command>) {
    let (mut app, received) = test_window();
    app.state = SignIn::SignedIn;
    app.library.playlists = Load::Ready(Vec::new());
    (app, received)
}

#[test]
fn a_changed_volume_asks_for_a_wake_within_quiet() {
    let (mut app, _received) = idle_window();
    let ctx = window_context();
    let mut idle = Duration::ZERO;
    for _ in 0..10 {
        idle = root_delay(&ui_frame(&mut app, &ctx));
        if idle == Duration::MAX {
            break;
        }
    }
    assert_eq!(idle, Duration::MAX, "an idle window must not be woken");

    assert!(app.transport.volume.set_fraction(0.4));
    let delay = root_delay(&ui_frame(&mut app, &ctx));
    assert!(
        delay < Duration::MAX,
        "a pending value must wake the window, at {delay:?}"
    );
    assert!(
        delay <= QUIET,
        "the write is due within the quiet window: {delay:?}"
    );
}

#[test]
fn on_exit_flushes_an_unsettled_change() {
    let (log, sink) = recording();
    let written = Prefs::parse("volume=40\n").expect("valid prefs");
    let (mut app, _received) =
        test_window_with(Loaded::Found(written.clone()), synced(written, sink));
    assert!(app.transport.volume.set_fraction(0.6));
    let changed = Prefs::of(&app.transport.volume, &app.queue);

    App::on_exit(&mut app, None);
    drop(app);

    assert_eq!(
        *log.lock().expect("the sink log"),
        vec![changed],
        "close writes the value no frame had time to settle"
    );
}

#[test]
fn on_exit_after_a_settled_write_sends_nothing_more() {
    let (log, sink) = recording();
    let written = Prefs::parse("volume=40\n").expect("valid prefs");
    let (mut app, _received) =
        test_window_with(Loaded::Found(written.clone()), synced(written, sink));
    assert!(app.transport.volume.set_fraction(0.6));
    let changed = Prefs::of(&app.transport.volume, &app.queue);
    let now = Instant::now();
    assert_eq!(
        app.prefs.observe(now, &changed),
        Some(QUIET),
        "the change is armed, not written"
    );
    assert_eq!(
        app.prefs.observe(now + QUIET, &changed),
        None,
        "quiet has passed, so the value is written"
    );

    App::on_exit(&mut app, None);
    drop(app);

    assert_eq!(
        *log.lock().expect("the sink log"),
        vec![changed],
        "a settled value is not written again"
    );
}

#[test]
fn an_untouched_session_never_writes() {
    let (log, sink) = recording();
    let written = Prefs::parse("volume=40\nshuffle=true\nrepeat=all\n").expect("valid prefs");
    let (mut app, mut received) =
        test_window_with(Loaded::Found(written.clone()), synced(written, sink));
    let ctx = window_context();
    for _ in 0..3 {
        let _ = ui_frame(&mut app, &ctx);
    }
    assert_eq!(
        volume_frames(&mut received),
        vec![40],
        "the restored level went out once, so the frames did run"
    );

    App::on_exit(&mut app, None);
    drop(app);

    assert!(
        log.lock().expect("the sink log").is_empty(),
        "nothing changed, so nothing is written"
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

/// Every command the frames since the last call sent.
fn take_commands(received: &mut tokio::sync::mpsc::UnboundedReceiver<Command>) -> Vec<Command> {
    let mut commands = Vec::new();
    while let Ok(command) = received.try_recv() {
        commands.push(command);
    }
    commands
}

fn watched(watches: &[Command]) -> usize {
    watches
        .iter()
        .filter(|command| matches!(command, Command::WatchDevices))
        .count()
}

#[test]
fn logic_sends_watch_devices_once_after_the_playlists_land() {
    let (mut app, mut received) = test_window();
    logic_frame(&mut app);
    assert_eq!(
        watched(&take_commands(&mut received)),
        0,
        "a watch was asked for before the account could be named"
    );

    app.library.playlists = Load::Ready(Vec::new());
    logic_frame(&mut app);
    assert_eq!(
        watched(&take_commands(&mut received)),
        1,
        "the playlists landing is what arms the watch"
    );

    logic_frame(&mut app);
    assert_eq!(
        watched(&take_commands(&mut received)),
        0,
        "a second frame asked the one-shot dealer to start again"
    );
}

#[test]
fn a_roster_drains_into_the_devices() {
    let (tx, rx) = channel();
    let mut state = SignIn::SignedIn;
    let mut library = Library::default();
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut devices = Devices::default();
    let now = Instant::now();
    tx.send(Update::Devices(DeviceUpdate::Roster {
        devices: vec![Device {
            id: "a".to_owned(),
            name: "Phone".to_owned(),
            kind: DeviceKind::Phone,
        }],
        holder: Some("a".to_owned()),
        held_here: false,
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
        &mut Playclock::at(now),
    );

    assert!(commands.is_empty());
    assert_eq!(devices.caption(now), "Phone");
    assert_eq!(
        devices.rows(&Transport::default(), false).len(),
        1,
        "the roster did not reach the card"
    );
}

/// A window already watching an account whose phone plays in this window's
/// own place: the remote transport is the one on screen.
fn remote_window(now: Instant) -> (TakofyApp, tokio::sync::mpsc::UnboundedReceiver<Command>) {
    let (mut app, received) = test_window();
    app.state = SignIn::SignedIn;
    app.library.playlists = Load::Ready(Vec::new());
    assert_eq!(app.devices.watch_request(true), Some(Command::WatchDevices));
    app.devices.apply(
        &Update::Devices(DeviceUpdate::Roster {
            devices: vec![Device {
                id: "a".to_owned(),
                name: "Phone".to_owned(),
                kind: DeviceKind::Phone,
            }],
            holder: Some("a".to_owned()),
            held_here: false,
        }),
        now,
    );
    app.devices.apply(
        &Update::Devices(DeviceUpdate::Playing(Some(RemotePlayback {
            track: track("spotify:track:a"),
            state: RemoteState::Playing,
            position_ms: 1_000,
            duration_ms: Some(60_000),
            steerable: true,
        }))),
        now,
    );
    (app, received)
}

/// The delay one `logic` pass asked for. The pass is the one a delayed
/// repaint is timed from, so the ask is read from that same output.
fn logic_delay(app: &mut TakofyApp) -> Duration {
    let ctx = window_context();
    // egui's own first passes ask for a frame; the app's ask is what is left.
    for _ in 0..2 {
        let mut warm = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        warm.textures_delta.clear();
    }
    let mut frame = eframe::Frame::_new_kittest();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        app.logic(ui.ctx(), &mut frame);
    });
    output.textures_delta.clear();
    root_delay(&output)
}

#[test]
fn an_idle_window_is_woken_at_the_watch_patience_and_then_rests() {
    let (mut app, _received) = idle_window();

    let delay = logic_delay(&mut app);
    assert!(
        delay > REDRAW,
        "an idle window was redrawn at the transport cadence: {delay:?}"
    );
    assert!(
        delay <= takofy_ui::devices::WATCH_PATIENCE,
        "the watch's own patience is a wake this window must get: {delay:?}"
    );

    // A watch that has answered has no clock left, so the next pass asks
    // for nothing without a remote holding the sound.
    app.devices.apply(
        &Update::Devices(DeviceUpdate::Failed(Failure::Devices)),
        Instant::now(),
    );
    assert_eq!(
        logic_delay(&mut app),
        Duration::MAX,
        "a settled idle window still woke for the watch"
    );
}

#[test]
fn a_flash_wakes_the_window_and_its_end_lets_it_rest() {
    let (mut app, _received) = idle_window();
    let now = Instant::now();
    // The watch has already settled, so the notice alone is asking.
    app.devices.apply(
        &Update::Devices(DeviceUpdate::Failed(Failure::Devices)),
        now,
    );
    app.devices
        .apply(&Update::Devices(DeviceUpdate::SteerFailed), now);

    let delay = logic_delay(&mut app);
    assert!(
        delay <= takofy_ui::devices::FLASH_FOR,
        "the notice's own deadline is not the wake: {delay:?}"
    );

    assert!(
        app.devices.expire(now + takofy_ui::devices::FLASH_FOR),
        "the notice did not expire"
    );
    assert_eq!(
        logic_delay(&mut app),
        Duration::MAX,
        "the expired notice still woke the window"
    );
}

#[test]
fn logic_sends_the_refresh_a_card_open_asked_for() {
    let now = Instant::now();
    let (mut app, mut received) = remote_window(now);
    app.devices.list_opened();

    logic_frame(&mut app);
    let sent = take_commands(&mut received);
    assert_eq!(
        sent.iter()
            .filter(|command| matches!(command, Command::RefreshDevices))
            .count(),
        1,
        "the open's refresh was not the logic pass's to send: {sent:?}"
    );

    logic_frame(&mut app);
    let sent = take_commands(&mut received);
    assert_eq!(
        sent.iter()
            .filter(|command| matches!(command, Command::RefreshDevices))
            .count(),
        0,
        "the open's refresh was sent twice: {sent:?}"
    );
}

#[test]
fn logic_sends_the_pick_a_taken_frame_left_behind() {
    let now = Instant::now();
    let (mut app, mut received) = remote_window(now);
    // A press whose own frame already held a command keeps its pick.
    assert_eq!(app.devices.pick("a", false), None);

    logic_frame(&mut app);
    let sent = take_commands(&mut received);
    assert_eq!(
        sent.iter()
            .filter(|command| matches!(command, Command::Transfer { .. }))
            .count(),
        1,
        "the delayed pick was not the logic pass's to send: {sent:?}"
    );

    logic_frame(&mut app);
    let sent = take_commands(&mut received);
    assert!(
        !sent
            .iter()
            .any(|command| matches!(command, Command::Transfer { .. })),
        "the delayed pick was sent twice: {sent:?}"
    );
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

#[test]
fn while_the_remote_plays_the_window_keeps_the_redraw_cadence() {
    let now = Instant::now();
    let (mut app, _received) = remote_window(now);

    let delay = logic_delay(&mut app);

    assert!(
        delay <= REDRAW,
        "the remote position would stand still: {delay:?}"
    );
}
