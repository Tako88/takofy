use super::super::*;
use super::test_support::*;
use crate::player::Transport;

/// A track that names the album and credit its row links to.
fn linked_track(index: usize) -> TrackEntry {
    TrackEntry {
        album_uri: Some("spotify:album:a".to_owned()),
        artist_uris: vec!["spotify:artist:b".to_owned()],
        ..track(index)
    }
}

#[test]
fn a_link_click_is_reported_as_open_not_play() {
    let ctx = context();
    let mut art = Art::default();
    let now = Instant::now();
    let tracks: Vec<TrackEntry> = (0..4).map(linked_track).collect();
    let mut queue = Queue::default();
    queue.start(&tracks, 0, None, &mut Transport::default());

    let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
    let at = painted_at(&output, "t2").expect("row t2 is painted");
    let reply = click(&ctx, PANE, &queue, at + Vec2::new(4.0, 4.0));

    assert_eq!(
        reply.open,
        Some(Page::Album("spotify:album:a".to_owned())),
        "a click on the name must ask for the album"
    );
    assert_eq!(
        (reply.play, reply.press),
        (None, None),
        "a link click is neither a play nor a press"
    );
}

#[test]
fn an_artist_link_is_reported_as_open_not_play() {
    let ctx = context();
    let mut art = Art::default();
    let now = Instant::now();
    let tracks: Vec<TrackEntry> = (0..4).map(linked_track).collect();
    let mut queue = Queue::default();
    queue.start(&tracks, 0, None, &mut Transport::default());

    let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
    let at = painted_at(&output, "a2").expect("the artist of row t2 is painted");
    let reply = click(&ctx, PANE, &queue, at + Vec2::new(4.0, 4.0));

    assert_eq!(
        reply.open,
        Some(Page::Artist("spotify:artist:b".to_owned())),
        "a click on the artist must ask for the artist"
    );
    assert_eq!(
        (reply.play, reply.press),
        (None, None),
        "a link click is neither a play nor a press"
    );
}
