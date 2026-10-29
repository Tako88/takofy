use super::super::*;
use super::test_support::*;
use crate::library::Load;
use crate::message::{DeviceUpdate, Steer, Update};
use crate::player::{NowPlaying, Transport, TransportState};
use crate::queue::Queue;
use crate::sidebar_kind::SidebarKind;
use crate::theme::{extent, space};
use takofy_model::{DeviceKind, PlaylistEntry, RemotePlayback, RemoteState};

/// The seek track the bar painted inside `column`.
pub(super) fn painted_seek_track(output: &egui::FullOutput, column: Rect) -> Rect {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape)
                if (shape.rect.height() - extent::SLIDER_TRACK_H).abs() < 0.01
                    && shape.fill == Tokens::DARK.palette.border_strong
                    && column.contains(shape.rect.center()) =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
        .expect("the bar paints its seek track")
}

/// What the holder is playing, as the cluster reports it.
fn playback(state: RemoteState, steerable: bool) -> Update {
    Update::Devices(DeviceUpdate::Playing(Some(RemotePlayback {
        track: track(9, None),
        state,
        position_ms: 3_000,
        duration_ms: Some(60_000),
        steerable,
    })))
}

#[test]
fn the_devices_button_opens_the_card() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = remote_window();

    let command = window.click(
        &mut transport,
        &mut queue,
        devices_button_at(&window).center(),
    );

    assert!(window.list.is_open(), "the press did not open the card");
    assert_eq!(
        command, None,
        "the open asked for the refresh itself instead of leaving it to `logic`"
    );
}

#[test]
fn a_second_press_of_the_button_closes_the_card() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = remote_window();
    let button = devices_button_at(&window).center();

    window.click(&mut transport, &mut queue, button);
    assert!(window.list.is_open());

    let command = window.click(&mut transport, &mut queue, button);
    assert!(!window.list.is_open(), "a second press left the card open");
    assert_eq!(command, None, "a close asks for nothing");
}

#[test]
fn escape_closes_the_card_and_hands_the_keyboard_back_to_the_button() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = remote_window();
    window.click(
        &mut transport,
        &mut queue,
        devices_button_at(&window).center(),
    );
    assert!(window.list.is_open());

    assert_eq!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Escape),
        None
    );
    assert!(!window.list.is_open(), "Escape left the card open");

    // The button holds the keyboard again, so Enter presses it.
    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
    assert!(
        window.list.is_open(),
        "the keyboard went somewhere other than the devices button"
    );
}

#[test]
fn a_taken_command_slot_leaves_the_refresh_want_for_logic() {
    use crate::view::test_support::click_events;
    let mut transport = Transport {
        state: TransportState::Playing,
        ..Transport::default()
    };
    let mut queue = Queue::default();
    assert!(
        queue
            .start(&[track(0, None)], 0, None, &mut transport)
            .is_some()
    );
    transport.state = TransportState::Playing;
    let mut window = remote_window();
    let at = devices_button_at(&window).center();

    // The release that presses the button shares its frame with a space,
    // which takes the frame's one command first.
    let [moved, down, up] = click_events(at);
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    let _ = window.frame(&mut transport, &mut queue, vec![moved]);
    let _ = window.frame(&mut transport, &mut queue, vec![down]);
    let (command, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![up, key(egui::Key::Space, true)],
    );

    assert!(window.list.is_open(), "the press did not open the card");
    assert!(
        matches!(command, Some(Command::Pause | Command::Resume)),
        "the space did not take the frame's command: {command:?}"
    );
    let (next, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::Space, false)],
    );
    assert_eq!(next, None, "the shell sent the refresh itself");
    assert_eq!(
        window.devices.refresh_request(),
        Some(Command::RefreshDevices),
        "a taken slot dropped the refresh instead of leaving it for `logic`"
    );
}

#[test]
fn a_pick_on_the_card_asks_for_the_sound_to_move() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = holding_window();

    let _ = window.click(
        &mut transport,
        &mut queue,
        devices_button_at(&window).center(),
    );
    assert!(window.list.is_open(), "the press did not open the card");

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = painted_at(&frame, "Laptop").expect("the card names the other device");

    let command = window.click(&mut transport, &mut queue, at);

    assert_eq!(
        command,
        Some(Command::Transfer { to: "b".to_owned() }),
        "the pick did not reach the bridge"
    );
    assert!(!window.list.is_open(), "a pick left the card up");
}

#[test]
fn a_pause_press_while_remote_steers_the_holder_and_never_the_queue() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    assert!(
        queue
            .start(&[track(0, None)], 0, None, &mut transport)
            .is_some()
    );
    transport.state = TransportState::Idle;
    let mut window = remote_window();

    let command = window.click(
        &mut transport,
        &mut queue,
        transport_button_at(&window, crate::availability::Control::PlayPause).center(),
    );

    assert_eq!(
        command,
        Some(Command::Steer {
            device: "a".to_owned(),
            steer: Steer::Pause,
        }),
        "a remote pause was not the holder's"
    );
    assert_eq!(
        queue.current().map(|entry| entry.uri.as_str()),
        Some("t0"),
        "the remote press moved the local queue"
    );
}

#[test]
fn a_remote_skip_leaves_the_local_queue_where_it_was() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let rows = vec![track(0, None), track(1, None)];
    assert!(queue.start(&rows, 1, None, &mut transport).is_some());
    transport.state = TransportState::Idle;
    let mut window = remote_window();

    let command = window.click(
        &mut transport,
        &mut queue,
        transport_button_at(&window, crate::availability::Control::Next).center(),
    );

    assert_eq!(
        command,
        Some(Command::Steer {
            device: "a".to_owned(),
            steer: Steer::SkipNext,
        }),
        "a remote skip did not steer the holder"
    );
    assert_eq!(
        queue.current().map(|entry| entry.uri.as_str()),
        Some("t1"),
        "the remote skip moved the local queue"
    );
}

#[test]
fn the_bar_draws_the_holders_caption_and_title() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = remote_window();

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let (bar, _) = playbar_parts(&window);
    assert!(
        text_in(&frame, "Phone", bar).is_some(),
        "the bar does not name the device holding the sound"
    );
    assert!(
        text_in(&frame, "Nothing playing", bar).is_none(),
        "the bar claims nothing plays while the holder's track is unnamed"
    );

    let mut named = track(9, None);
    named.title = "A Title".to_owned();
    named.named = true;
    window
        .devices
        .apply(&Update::Devices(DeviceUpdate::Labelled(named)), window.now);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        text_in(&frame, "A Title", bar).is_some(),
        "the label that arrived did not reach the bar"
    );
    assert!(
        text_in(&frame, "Phone", bar).is_some(),
        "the caption went missing once the track was named"
    );
}

#[test]
fn a_local_play_flips_the_bar_back_to_this_window() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    assert!(
        queue
            .start(&[track(0, None)], 0, None, &mut transport)
            .is_some()
    );
    let mut window = remote_window();

    transport.state = TransportState::Playing;
    transport.now_playing = Some(NowPlaying {
        uri: "t0".to_owned(),
        title: "Local Track".to_owned(),
        artists: String::new(),
        ..Default::default()
    });
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let (bar, _) = playbar_parts(&window);
    assert!(
        text_in(&frame, "Phone", bar).is_none(),
        "the bar kept the holder after this window started playing"
    );
    assert!(
        text_in(&frame, "Local Track", bar).is_some(),
        "the bar did not draw this window's own track"
    );

    let command = window.click(
        &mut transport,
        &mut queue,
        transport_button_at(&window, crate::availability::Control::PlayPause).center(),
    );
    assert_eq!(
        command,
        Some(Command::Pause),
        "a local play/pause press stopped going through the queue"
    );
}

#[test]
fn space_while_remote_toggles_the_holder_and_leaves_an_armed_queue_alone() {
    let mut transport = Transport::default();
    let mut queue = armed_queue();
    let mut window = remote_window();

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Space);

    assert_eq!(
        command,
        Some(Command::Steer {
            device: "a".to_owned(),
            steer: Steer::Pause,
        }),
        "a space while remote did not toggle the holder"
    );
    assert!(queue.armed(), "the remote space started the local queue");
    assert_eq!(queue.current(), None, "the remote space moved the queue");
}

#[test]
fn the_lyrics_tab_asks_for_the_holders_track_only_once() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = remote_window();
    let mut named = track(9, None);
    named.title = "A Title".to_owned();
    window
        .devices
        .apply(&Update::Devices(DeviceUpdate::Labelled(named)), window.now);
    window.library.lyrics.select(crate::lyrics::Tab::Lyrics);

    let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());
    assert_eq!(
        command,
        Some(Command::LoadLyrics {
            track: "t9".to_owned()
        }),
        "the open tab asked for a track that is not on screen"
    );
    assert!(
        matches!(window.library.lyrics.load(), Load::Loading),
        "the load did not start for the holder's track"
    );

    // The backdrop's track never changes, so nothing is asked for again: a
    // request reading the silent local transport would starve or reset it.
    for _ in 0..3 {
        let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(command, None, "the same lyrics were asked for again");
        assert!(
            matches!(window.library.lyrics.load(), Load::Loading),
            "the load was reset under the reader"
        );
    }
}

#[test]
fn a_local_play_mid_drag_takes_the_holders_preview_with_its_bar() {
    use crate::view::test_support::drag_events;
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = remote_window();
    playlist_page(&mut window, &["p0", "p1"]);
    let _ = click_page_row(&mut window, &mut transport, &mut queue, "p0");

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let (bar, form) = playbar_parts(&window);
    let centre = layout::playbar_for(bar, form).columns(bar)[1];
    let track = painted_seek_track(&frame, centre);
    let from = egui::Pos2::new(track.left() + track.width() * 0.2, track.center().y);
    let to = egui::Pos2::new(track.left() + track.width() * 0.8, track.center().y);

    let mut held = drag_events(from, to);
    held.pop();
    for event in held {
        let _ = window.frame(&mut transport, &mut queue, vec![event]);
    }
    assert!(
        window.devices.display().dragging,
        "the drag left no preview on the holder's bar"
    );

    // Enter on the focused page row starts a local track, so this frame's
    // bar is this window's own and the release reaches the holder's slider
    // never.
    let release = window.release(to);
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![release, key(egui::Key::Enter, true)],
    );
    assert_eq!(
        window.devices.display().scrub,
        None,
        "the holder's bar is gone, so its preview must not outlive it"
    );
    assert!(!window.devices.display().dragging);
}

#[test]
fn now_playings_more_is_inert_while_the_holder_plays_a_track_this_queue_lacks() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    // This window's own row has verbs, and the holder's push is another
    // track: the button must not reach the row the pane is not drawing.
    queue.start(&[menu_track(0)], 0, None, &mut transport);
    transport.state = TransportState::Idle;
    let mut window = remote_window();

    let at = now_playing_more(&window);
    let _ = window.click(&mut transport, &mut queue, at);
    settle_menu(&mut window, &mut transport, &mut queue);

    assert!(
        !window.menu.is_open(),
        "the more button acted on the local row while the holder played"
    );
}

#[test]
fn now_playings_more_acts_on_the_holders_track_while_the_queue_holds_it() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut held = track(9, None);
    held.album_uri = Some(ALBUM.to_owned());
    held.artist_uris = vec!["spotify:artist:b".to_owned()];
    held.artist_names = vec!["A Band".to_owned()];
    queue.start(&[track(0, None), held], 0, None, &mut transport);
    transport.state = TransportState::Idle;
    let mut window = remote_window();

    let at = now_playing_more(&window);
    let _ = window.click(&mut transport, &mut queue, at);
    settle_menu(&mut window, &mut transport, &mut queue);

    assert!(window.menu.is_open(), "the more button opened no menu");
    assert_eq!(
        window
            .menu
            .targets()
            .iter()
            .map(|entry| entry.uri.as_str())
            .collect::<Vec<_>>(),
        ["t9"],
        "the menu is about a track the pane is not drawing"
    );
    assert!(window.menu.cut().is_none(), "the entry was removable");
}

#[test]
fn now_playings_more_acts_on_this_windows_row_while_it_plays() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    queue.start(&[menu_track(0)], 0, None, &mut transport);
    // A holder is still there behind the local play, so the source rule is
    // what keeps this frame on the local row.
    let mut window = remote_window();

    let at = now_playing_more(&window);
    let _ = window.click(&mut transport, &mut queue, at);
    settle_menu(&mut window, &mut transport, &mut queue);

    assert!(window.menu.is_open(), "the more button opened no menu");
    assert_eq!(
        window
            .menu
            .targets()
            .iter()
            .map(|entry| entry.uri.as_str())
            .collect::<Vec<_>>(),
        ["spotify:track:0"],
        "the menu is about the holder's track while this window plays"
    );
}

#[test]
fn a_dropped_steer_clears_the_preview_the_drag_left() {
    use crate::view::test_support::drag_events;
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = remote_window();
    // A Back settles a stale playlist in this frame, taking its one command
    // before the playbar draws, so the seek the release asks for is dropped.
    stale_playlist_behind_an_album(&mut window.library);

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let (bar, form) = playbar_parts(&window);
    let centre = layout::playbar_for(bar, form).columns(bar)[1];
    let track = painted_seek_track(&frame, centre);
    let from = egui::Pos2::new(track.left() + track.width() * 0.2, track.center().y);
    let to = egui::Pos2::new(track.left() + track.width() * 0.8, track.center().y);

    // The drag stops with the button still down, so the bar holds a preview.
    let mut held = drag_events(from, to);
    held.pop();
    for event in held {
        let _ = window.frame(&mut transport, &mut queue, vec![event]);
    }
    assert!(
        window.devices.display().scrub.is_some(),
        "the drag left no preview to lose"
    );

    let release = window.release(to);
    let (command, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![release, key(egui::Key::BrowserBack, true)],
    );
    assert!(
        matches!(command, Some(Command::LoadTracks { .. })),
        "the nav did not take the frame's command: {command:?}"
    );
    assert_eq!(
        window.devices.display().scrub,
        None,
        "a dropped seek left its preview standing until a push never comes"
    );
}

#[test]
fn space_emits_nothing_while_the_holder_cannot_be_steered() {
    let mut transport = Transport::default();
    let mut queue = armed_queue();
    let mut window = remote_window();
    let now = window.now;
    window
        .devices
        .apply(&playback(RemoteState::Playing, false), now);

    assert_eq!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Space),
        None,
        "a space steered a holder that forbids it"
    );
    assert!(queue.armed(), "a refused space started the local queue");
}

/// A library root holding one named playlist, with its rows already loaded.
fn shelf_with_rows(window: &mut Window, rows: usize) {
    window.library.apply(&Update::Playlists(vec![PlaylistEntry {
        saved_at: None,
        composed: Vec::new(),
        uri: PLAYLIST.to_owned(),
        name: "Mix".to_owned(),
        named: true,
        cover: None,
        description: None,
        owner: None,
        tracks: None,
        access: None,
    }]));
    window.library.select(PLAYLIST);
    window.library.apply(&Update::Tracks {
        playlist: PLAYLIST.to_owned(),
        tracks: (0..rows).map(|index| track(index, None)).collect(),
        cover: None,
        access: None,
    });
    window.library.to_playlists();
}

#[test]
fn landed_shelf_rows_start_the_queue_at_the_first_track_with_the_object_as_origin() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    shelf_with_rows(&mut window, 2);
    assert!(window.library.want_play(PLAYLIST, SidebarKind::Playlists));
    assert!(window.library.has_landed_play());

    let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());

    assert_eq!(
        command,
        Some(Command::Play {
            uri: "t0".to_owned()
        })
    );
    assert_eq!(queue.origin(), Some(PLAYLIST), "the queue named no object");
    assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("t0"));
    assert_eq!(
        transport.now_playing.as_ref().map(|now| now.uri.as_str()),
        Some("t0"),
        "the start never reached the transport"
    );
    assert!(
        !window.library.has_landed_play(),
        "the landed rows were kept after a start"
    );
}

#[test]
fn a_frame_that_took_a_command_keeps_the_landed_rows_for_the_next() {
    let mut transport = playing(cover("abc"));
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    window.library.lyrics.select(crate::lyrics::Tab::Lyrics);
    window.library.apply(&Update::Playlists(Vec::new()));
    assert!(window.library.want_play(PLAYLIST, SidebarKind::Playlists));
    window.library.apply(&Update::Tracks {
        playlist: PLAYLIST.to_owned(),
        tracks: vec![track(0, None), track(1, None)],
        cover: None,
        access: None,
    });

    let (command, output) = window.frame(&mut transport, &mut queue, Vec::new());
    assert_eq!(
        command,
        Some(Command::LoadLyrics {
            track: "spotify:track:x".to_owned()
        }),
        "the frame was expected to take the lyrics read"
    );
    assert!(
        window.library.has_landed_play(),
        "a taken frame dropped the landed rows"
    );
    let delay = output
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .map(|viewport| viewport.repaint_delay);
    assert_eq!(
        delay,
        Some(std::time::Duration::ZERO),
        "a held start asked for no frame of its own"
    );
    assert!(
        queue.current().is_none(),
        "a taken frame started the play anyway"
    );

    let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());
    assert_eq!(
        command,
        Some(Command::Play {
            uri: "t0".to_owned()
        }),
        "the held play never got its frame"
    );
    assert_eq!(queue.origin(), Some(PLAYLIST));
}

#[test]
fn a_navigation_in_the_frame_the_rows_land_starts_nothing() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    shelf_with_rows(&mut window, 1);
    let _ = window.library.select(PLAYLIST);
    window.library.apply(&Update::Tracks {
        playlist: PLAYLIST.to_owned(),
        tracks: vec![track(0, None)],
        cover: None,
        access: None,
    });

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let crumb = text_in(&frame, "PLAYLISTS", topbar_rect(&window))
        .expect("the trail's Playlists crumb is painted");
    let [arrive, press, release] = crate::view::test_support::click_events(crumb.center());
    let _ = window.frame(&mut transport, &mut queue, vec![arrive, press]);

    assert!(window.library.want_play(PLAYLIST, SidebarKind::Playlists));
    window.library.apply(&Update::Tracks {
        playlist: PLAYLIST.to_owned(),
        tracks: vec![track(1, None)],
        cover: None,
        access: None,
    });
    assert!(window.library.has_landed_play());

    let _ = window.frame(&mut transport, &mut queue, vec![release]);
    assert!(
        queue.current().is_none(),
        "a navigation frame started the play"
    );
    assert_eq!(
        window.library.page(),
        None,
        "the press was expected to go home"
    );

    let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());
    assert_eq!(command, None);
    assert!(
        queue.current().is_none(),
        "a play outlived the page it was pressed on"
    );
}

#[test]
fn a_carry_holds_a_landed_shelf_play_until_it_ends() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    shelf_with_rows(&mut window, 1);
    assert!(window.library.want_play(PLAYLIST, SidebarKind::Playlists));

    egui::DragAndDrop::set_payload(&window.ctx, ());
    let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());
    egui::DragAndDrop::clear_payload(&window.ctx);

    assert_eq!(command, None, "a carried block started the play");
    assert!(
        window.library.has_landed_play(),
        "a carried frame dropped the landed rows"
    );

    let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());
    assert_eq!(
        command,
        Some(Command::Play {
            uri: "t0".to_owned()
        }),
        "the play was not started once the carry ended"
    );
}

/// A window whose watch answered with the phone playing.
fn remote_window() -> Window {
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let now = window.now;
    assert_eq!(
        window.devices.watch_request(true),
        Some(Command::WatchDevices)
    );
    window.devices.apply(&roster(), now);
    window
        .devices
        .apply(&playback(RemoteState::Playing, true), now);
    window
}

/// A window that holds the sound itself, so the card's rows can move it.
fn holding_window() -> Window {
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let now = window.now;
    assert_eq!(
        window.devices.watch_request(true),
        Some(Command::WatchDevices)
    );
    window.devices.apply(
        &Update::Devices(DeviceUpdate::Roster {
            devices: vec![
                device("a", "Phone", DeviceKind::Phone),
                device("b", "Laptop", DeviceKind::Computer),
            ],
            holder: None,
            held_here: true,
        }),
        now,
    );
    window
}

fn devices_button_at(window: &Window) -> Rect {
    let (bar, form) = playbar_parts(window);
    playbar::devices_button(bar, form).expect("the full extras draw the devices button")
}

/// One transport button, solved the way the bar solves it: play is centred
/// and the runs walk out from it.
fn transport_button_at(window: &Window, control: crate::availability::Control) -> Rect {
    use crate::availability::Control;
    let (bar, _) = playbar_parts(window);
    let stack = extent::BUTTON_LG + space::S1 + extent::SLIDER_H;
    let top = bar.center().y - stack / 2.0;
    let down_w = extent::BUTTON + space::S1 + extent::BUTTON;
    let row = layout::playbar::transport_row(bar.center().x, down_w);
    let square =
        |x: f32, y: f32| Rect::from_min_size(egui::pos2(x, y), Vec2::splat(extent::BUTTON));
    match control {
        Control::PlayPause => {
            Rect::from_min_size(egui::pos2(row.play_x, top), Vec2::splat(extent::BUTTON_LG))
        }
        Control::Previous => square(
            row.down_x + extent::BUTTON + space::S1,
            top + extent::BUTTON / 2.0,
        ),
        Control::Next => square(row.up_x, top + extent::BUTTON / 2.0),
        other => panic!("{other:?} is not a transport button"),
    }
}

/// An armed local queue: rows with nothing current, which a space would
/// start, and the index a remote press must not move.
fn armed_queue() -> Queue {
    let mut queue = Queue::default();
    let rows = vec![track(0, None), track(1, None)];
    assert!(queue.append(rows).is_some());
    queue
}

/// The account's roster, with the phone holding the sound.
fn roster() -> Update {
    Update::Devices(DeviceUpdate::Roster {
        devices: vec![
            device("a", "Phone", DeviceKind::Phone),
            device("b", "Laptop", DeviceKind::Computer),
        ],
        holder: Some("a".to_owned()),
        held_here: false,
    })
}
