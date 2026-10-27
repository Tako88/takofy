use super::super::*;
use super::test_support::*;
use crate::player::Transport;
use crate::queue::Queue;
use takofy_model::{CoverSet, CoverUrl};

/// A cover offering 64, 300 and 640 squares, with their URLs.
pub(super) fn sized(tail: &str) -> (CoverSet, CoverUrl, CoverUrl, CoverUrl) {
    let small =
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}-64")).expect("a cover url");
    let middle =
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}-300")).expect("a cover url");
    let large =
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}-640")).expect("a cover url");
    let set = CoverSet::from_sizes([
        (Some((64, 64)), small.clone()),
        (Some((300, 300)), middle.clone()),
        (Some((640, 640)), large.clone()),
    ])
    .expect("a cover set");
    (set, small, middle, large)
}

#[test]
fn the_panel_asks_for_the_size_its_own_art_box_takes() {
    let (set, _, middle, large) = sized("panel");

    let mut transport = playing(set.clone());
    let mut queue = Queue::default();
    let asked = asked_at(Vec2::new(1400.0, 900.0), 1.0, &mut transport, &mut queue);
    assert!(
        asked.contains(&middle),
        "the 256pt panel box takes the 300 at 1x: {asked:?}"
    );
    assert!(
        !asked.contains(&large),
        "the panel must not fetch the 640 at 1x: {asked:?}"
    );

    let mut transport = playing(set);
    let asked = asked_at(Vec2::new(1400.0, 900.0), 2.0, &mut transport, &mut queue);
    assert!(
        asked.contains(&large),
        "the panel's 512px box takes the 640 at 2x: {asked:?}"
    );
    assert!(
        !asked.contains(&middle),
        "the panel must not keep the 300 at 2x: {asked:?}"
    );
}

#[test]
fn a_hidden_panel_leaves_only_the_playbars_small_tile() {
    let (set, small, _, _) = sized("panel");
    let mut transport = playing(set);
    let mut queue = Queue::default();

    let asked = asked_at(Vec2::new(900.0, 600.0), 2.0, &mut transport, &mut queue);

    assert_eq!(
        asked,
        vec![small],
        "the playbar's 88px box takes the 64, and nothing else is drawn"
    );
}

#[test]
fn the_panel_and_the_playbar_ask_for_different_sizes_of_one_track() {
    let (set, small, middle, large) = sized("panel");
    let mut transport = playing(set);
    let mut queue = Queue::default();

    let mut asked = asked_at(Vec2::new(1400.0, 900.0), 1.0, &mut transport, &mut queue);
    asked.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    let mut expected = vec![small, middle];
    expected.sort_by(|left, right| left.as_str().cmp(right.as_str()));

    assert_eq!(
        asked, expected,
        "the playbar takes the 64 and the panel the 300: {asked:?}"
    );
    assert!(
        !asked.contains(&large),
        "neither box needs the 640 at 1x: {asked:?}"
    );
}

#[test]
fn the_playbar_tile_is_requested_even_with_the_panel_hidden() {
    // Narrower than the panel breakpoint, so the Now Playing panel is not
    // drawn and only the playbar can want a cover — the queue's rows too.
    let mut transport = playing(cover("abc"));
    let tracks = vec![
        track(0, Some(cover("abc"))),
        track(1, Some(cover("row1"))),
        track(2, Some(cover("row2"))),
    ];
    let mut queue = Queue::default();
    queue.start(&tracks, 0, None, &mut transport);

    assert_eq!(
        requested(Vec2::new(900.0, 600.0), &mut transport, &mut queue),
        vec![cover_url("abc")],
        "a hidden panel must not ask for its rows' covers"
    );
}

#[test]
fn a_visible_panel_requests_the_visible_rows_covers() {
    let mut transport = playing(cover("abc"));
    let tracks = vec![
        track(0, Some(cover("abc"))),
        track(1, Some(cover("row1"))),
        track(2, Some(cover("row2"))),
    ];
    let mut queue = Queue::default();
    queue.start(&tracks, 0, None, &mut transport);

    let mut asked = requested(Vec2::new(1400.0, 900.0), &mut transport, &mut queue);
    asked.sort_by(|left, right| left.as_str().cmp(right.as_str()));

    let mut expected = vec![cover_url("abc"), cover_url("row1"), cover_url("row2")];
    expected.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    assert_eq!(
        asked, expected,
        "the panel must ask for the rows it draws, and the playbar for its tile"
    );
}

/// The Lyrics tab asks for the playing track's lyrics once, and the shell is
/// what sends it.
#[test]
fn opening_the_lyrics_tab_asks_for_the_lyrics_once() {
    let mut transport = playing(cover("abc"));
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = painted_at(&frame, "LYRICS").expect("the lyrics tab is painted");
    let command = window.click(&mut transport, &mut queue, at + Vec2::new(2.0, 2.0));

    assert_eq!(
        command,
        Some(Command::LoadLyrics {
            track: "spotify:track:x".to_owned()
        }),
        "opening the tab asks for the track the transport names"
    );

    // The next frame does not ask again: the load is in flight.
    let (_, _) = window.frame(&mut transport, &mut queue, Vec::new());
    assert_eq!(
        window.frame(&mut transport, &mut queue, Vec::new()).0,
        None,
        "one request per track"
    );
}

#[test]
fn a_single_click_on_a_queue_row_does_not_play() {
    // A single click is a press; the shell applies presses with the drag
    // wiring, so nothing may move the transport here.
    let mut transport = playing(cover("abc"));
    let tracks = vec![
        track(0, Some(cover("abc"))),
        track(1, Some(cover("row1"))),
        track(2, Some(cover("row2"))),
    ];
    let mut queue = Queue::default();
    queue.start(&tracks, 0, None, &mut transport);

    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = painted_at(&frame, "t2").expect("row t2 is painted");

    assert_eq!(
        window.click(&mut transport, &mut queue, at + Vec2::new(6.0, 6.0)),
        None
    );
    assert_eq!(
        transport.now_playing.as_ref().map(|now| now.uri.as_str()),
        Some("t0"),
        "a single click must not move the transport"
    );
}

#[test]
fn a_double_click_on_a_queue_row_plays_that_entry() {
    // The panel reports the row and the shell is what moves the queue and
    // the transport, so nothing can name a track the transport never got.
    let mut transport = playing(cover("abc"));
    let tracks = vec![
        track(0, Some(cover("abc"))),
        track(1, Some(cover("row1"))),
        track(2, Some(cover("row2"))),
    ];
    let mut queue = Queue::default();
    queue.start(&tracks, 0, None, &mut transport);

    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = painted_at(&frame, "t2").expect("row t2 is painted");

    assert_eq!(
        window.double_click(&mut transport, &mut queue, at + Vec2::new(6.0, 6.0)),
        Some(Command::Play {
            uri: "t2".to_owned()
        })
    );
    assert_eq!(
        transport.now_playing.as_ref().map(|now| now.uri.as_str()),
        Some("t2"),
        "the shell's jump must reach the transport, not only the queue"
    );
}

/// Draws one frame of the whole shell at `screen` and returns the covers it
/// asked for.
fn requested(screen: Vec2, transport: &mut Transport, queue: &mut Queue) -> Vec<CoverUrl> {
    let mut window = Window::new(screen);
    let _ = window.frame(transport, queue, Vec::new());
    window.requests()
}

/// The covers a fresh window of `size` at `ppp` asks for, once the zoom has
/// settled.
fn asked_at(size: Vec2, ppp: f32, transport: &mut Transport, queue: &mut Queue) -> Vec<CoverUrl> {
    let mut window = Window::new(size);
    window.ctx.set_pixels_per_point(ppp);
    // A zoom change lands on the pass after it, so the window is measured
    // once it has settled.
    let _ = window.frame(transport, queue, Vec::new());
    let _ = window.frame(transport, queue, Vec::new());
    window.requests()
}
