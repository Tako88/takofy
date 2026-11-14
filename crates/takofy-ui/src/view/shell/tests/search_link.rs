use super::super::*;
use super::test_support::*;
use crate::devices::Source;
use crate::player::{Transport, TransportState};
use crate::queue::Queue;
use crate::route::Page;

/// A Spotify id, so the tests read as the address they are about.
const ID: &str = "4uLU6hMCjMI75M1A2tKUQC";

fn track_link() -> String {
    format!("spotify:track:{ID}")
}

#[test]
fn pasting_a_page_link_opens_it_at_once() {
    for kind in ["playlist", "album", "artist", "show"] {
        let uri = format!("spotify:{kind}:{ID}");
        let page = match kind {
            "playlist" => Page::Playlist(uri.clone()),
            "album" => Page::Album(uri.clone()),
            "artist" => Page::Artist(uri.clone()),
            _ => Page::Show(uri.clone()),
        };
        let load = match kind {
            "playlist" => Command::LoadTracks {
                playlist: uri.clone(),
            },
            "album" => Command::LoadAlbum { album: uri.clone() },
            "artist" => Command::LoadArtist {
                artist: uri.clone(),
            },
            _ => Command::LoadShow { show: uri.clone() },
        };
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let web = format!("https://open.spotify.com/{kind}/{ID}");

        assert_eq!(
            paste_into_field(&mut window, &mut transport, &mut queue, &web),
            Some(load),
            "{kind} opens at once"
        );
        assert_eq!(window.library.page(), Some(&page));
        assert_eq!(window.library.search.text, "", "the field empties");
        assert!(!window.library.search.is_open(), "no menu flashes");
        assert!(
            window.ctx.memory(|memory| memory.focused()).is_none(),
            "the field hands the keyboard on"
        );
        assert_eq!(search_read(&mut window), None, "a link is not a read's key");
        assert!(window.library.can_back(), "opening a page is a move");
        let _ = window.library.back();
        assert_eq!(window.library.page(), None, "Back returns to the root");
    }
}

#[test]
fn pasting_the_old_playlist_form_opens_the_canonical_page() {
    let canonical = format!("spotify:playlist:{ID}");
    for text in [
        format!("spotify:user:alice:playlist:{ID}"),
        format!("https://open.spotify.com/user/alice/playlist/{ID}"),
    ] {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        assert_eq!(
            paste_into_field(&mut window, &mut transport, &mut queue, &text),
            Some(Command::LoadTracks {
                playlist: canonical.clone()
            }),
            "{text:?} names one playlist"
        );
        assert_eq!(
            window.library.page(),
            Some(&Page::Playlist(canonical.clone())),
            "the name is not part of what the page addresses"
        );
    }
}

#[test]
fn pasting_a_track_link_with_something_playing_queues_it_behind() {
    let mut transport = Transport::default();
    let mut queue = queue_of(3, &mut transport);
    assert_eq!(transport.state, TransportState::Loading);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));

    assert_eq!(
        paste_into_field(&mut window, &mut transport, &mut queue, &track_link()),
        None,
        "what is playing is not interrupted"
    );
    assert_eq!(
        queue.upcoming().next().map(|(_, entry)| entry.uri.clone()),
        Some(track_link()),
        "the row waits first in the queue"
    );
    assert_eq!(
        queue.current().map(|entry| entry.uri.clone()),
        Some("t0".to_owned()),
        "the current row did not move"
    );
    assert_eq!(window.library.search.text, "");
    assert!(!window.library.search.is_open());
}

#[test]
fn pasting_an_episode_link_is_queued_the_same_way() {
    let episode = format!("spotify:episode:{ID}");
    let mut transport = Transport::default();
    let mut queue = queue_of(3, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));

    assert_eq!(
        paste_into_field(&mut window, &mut transport, &mut queue, &episode),
        None
    );
    assert_eq!(
        queue.upcoming().next().map(|(_, entry)| entry.uri.clone()),
        Some(episode)
    );
}

#[test]
fn pasting_a_track_link_with_nothing_playing_starts_it() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));

    assert_eq!(
        paste_into_field(&mut window, &mut transport, &mut queue, &track_link()),
        Some(Command::Play { uri: track_link() })
    );
    assert_eq!(
        transport.now_playing.as_ref().map(|now| now.uri.clone()),
        Some(track_link()),
        "the playbar names the pasted row"
    );
    assert_eq!(
        queue.current().map(|entry| entry.uri.clone()),
        Some(track_link())
    );
}

/// A row that started with nothing playing is the current row, so it owes a
/// name just as a waiting one does.
#[test]
fn a_started_link_is_asked_for_its_name() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));

    assert!(paste_into_field(&mut window, &mut transport, &mut queue, &track_link()).is_some());
    assert_eq!(
        queue.label_wants(),
        Some(Command::LabelRows {
            uris: vec![track_link()]
        })
    );
}

#[test]
fn pasting_a_track_link_while_another_device_holds_the_sound_only_queues_it() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = remote_window();
    assert_eq!(window.devices.source(&transport), Source::Remote);

    assert_eq!(
        paste_into_field(&mut window, &mut transport, &mut queue, &track_link()),
        None,
        "the phone holding the sound is never interrupted"
    );
    assert_eq!(
        transport.state,
        TransportState::Idle,
        "this window started nothing"
    );
    assert_eq!(
        queue.upcoming().next().map(|(_, entry)| entry.uri.clone()),
        Some(track_link()),
        "the row is still queued"
    );
    assert_eq!(window.library.search.text, "", "the field empties");
}

#[test]
fn a_queued_paste_leaves_an_armed_queue_armed_while_a_device_holds_the_sound() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    assert!(queue.append(vec![track(0, None), track(1, None)]).is_some());
    assert!(queue.armed());
    let mut window = remote_window();
    assert_eq!(window.devices.source(&transport), Source::Remote);

    assert_eq!(
        paste_into_field(&mut window, &mut transport, &mut queue, &track_link()),
        None
    );
    assert!(queue.armed(), "nothing started, so nothing disarmed it");
    assert_eq!(
        queue.upcoming().next().map(|(_, entry)| entry.uri.clone()),
        Some(track_link())
    );
}

/// The queue drew before the insert, so the frame that inserted must ask for
/// the next: a paste raises no event of its own.
///
/// The delay cannot say this — egui's own first-pass work reports zero for
/// every frame here — so the pass's repaint causes do. egui reports them on the
/// pass after the one that asked.
///
/// This pins that the shell asked for a repaint after a queueing paste, not
/// that the paste arm's own line was the only thing that could have asked: any
/// other shell repaint firing in this scenario would make it pass too.
#[test]
fn a_paste_asks_for_a_repaint() {
    let mut transport = Transport::default();
    let mut queue = queue_of(3, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let at = field_box(&mut window, &mut transport, &mut queue).center();
    let _ = window.click(&mut transport, &mut queue, at);

    let (command, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![egui::Event::Paste(track_link())],
    );
    assert_eq!(command, None, "the frame's command was free");
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    let causes = window.ctx.repaint_causes();
    assert!(
        causes.iter().any(|cause| cause.file.ends_with("shell.rs")),
        "the shell asked for no frame of its own: {causes:?}"
    );
}

#[test]
fn typing_a_link_acts_on_nothing_until_enter() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let at = field_box(&mut window, &mut transport, &mut queue).center();
    let _ = window.click(&mut transport, &mut queue, at);

    let (command, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![egui::Event::Text(track_link())],
    );
    assert_eq!(command, None, "typing a link acts on nothing");
    assert_eq!(search_read(&mut window), None, "no menu, no read");
    assert!(!window.library.search.is_open());
    assert_eq!(window.library.search.text, track_link());

    assert_eq!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
        Some(Command::Play { uri: track_link() })
    );
    assert_eq!(window.library.search.text, "", "Enter takes the link");
    assert!(
        window.ctx.memory(|memory| memory.focused()).is_none(),
        "the field hands the keyboard on"
    );
}

#[test]
fn enter_on_a_typed_album_link_opens_it() {
    let album = format!("spotify:album:{ID}");
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let at = field_box(&mut window, &mut transport, &mut queue).center();
    let _ = window.click(&mut transport, &mut queue, at);
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![egui::Event::Text(album.clone())],
    );

    assert_eq!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
        Some(Command::LoadAlbum {
            album: album.clone()
        })
    );
    assert_eq!(window.library.page(), Some(&Page::Album(album)));
}

#[test]
fn a_refused_link_searches_as_before() {
    for text in ["https://example.com/track/x", "spotify:local:a:b:c"] {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        assert_eq!(
            paste_into_field(&mut window, &mut transport, &mut queue, text),
            None,
            "{text:?} names no destination"
        );
        assert_eq!(window.library.page(), None, "nothing opened");
        assert_eq!(window.library.search.text, text, "the text is kept");
        assert!(
            window.library.search.is_open(),
            "it searches like any other query"
        );
        assert_eq!(
            search_read(&mut window),
            Some(Command::Search {
                query: text.to_owned()
            })
        );
    }
}

#[test]
fn a_paste_of_plain_text_is_typing() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));

    assert_eq!(
        paste_into_field(&mut window, &mut transport, &mut queue, "never gonna"),
        None
    );
    assert_eq!(window.library.search.text, "never gonna");
    assert!(window.library.search.is_open());
    assert_eq!(
        search_read(&mut window),
        Some(Command::Search {
            query: "never gonna".to_owned()
        })
    );
}

#[test]
fn a_paste_into_an_unfocused_field_does_nothing() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let _ = window.frame(&mut transport, &mut queue, Vec::new());

    let (command, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![egui::Event::Paste(format!("spotify:album:{ID}"))],
    );
    assert_eq!(command, None);
    assert_eq!(window.library.search.text, "");
    assert!(!window.library.search.is_open());
    assert_eq!(window.library.page(), None);
}

/// The frame's one command went to the navigation, so the paste is not
/// applied; the field keeps the link, and Enter still acts on it.
#[test]
fn a_dropped_paste_leaves_the_link_and_enter_still_acts() {
    let album = format!("spotify:album:{ID}");
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    stale_playlist_behind_an_album(&mut window.library);
    let at = field_box(&mut window, &mut transport, &mut queue).center();
    let _ = window.click(&mut transport, &mut queue, at);

    let (command, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![
            egui::Event::Paste(album.clone()),
            key(egui::Key::BrowserBack, true),
        ],
    );
    assert_eq!(
        command,
        Some(Command::LoadTracks {
            playlist: PLAYLIST.to_owned()
        }),
        "Back took the frame's command"
    );
    assert_eq!(window.library.search.text, album, "the link is not lost");
    assert_eq!(
        window.library.page(),
        Some(&Page::Playlist(PLAYLIST.to_owned())),
        "Back landed on the page the command loads"
    );

    assert_eq!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
        Some(Command::LoadAlbum {
            album: album.clone()
        }),
        "Enter acts on the link the paste left"
    );
    assert_eq!(window.library.page(), Some(&Page::Album(album)));
}
