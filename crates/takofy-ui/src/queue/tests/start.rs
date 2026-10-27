use super::super::*;
use super::test_support::*;

#[test]
fn replacing_the_queue_mid_play_leaves_no_stale_guard() {
    let first = tracks(3);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.start(&first, 0, None, &mut transport),
        Some(play("t0"))
    );
    clear_guard(&mut queue, &mut transport);

    let second = vec![track("x"), track("y")];
    assert_eq!(
        queue.start(&second, 0, None, &mut transport),
        Some(play("x"))
    );
    assert_eq!(
        end(&mut queue, &mut transport),
        None,
        "the old track's end must not advance the new queue"
    );
}

#[test]
fn the_queue_is_unaffected_by_the_library_changing() {
    use takofy_model::PlaylistEntry;

    let all = tracks(3);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(queue.start(&all, 0, None, &mut transport), Some(play("t0")));
    clear_guard(&mut queue, &mut transport);

    let playlists = Update::Playlists(vec![PlaylistEntry {
        composed: Vec::new(),
        uri: "spotify:playlist:a".to_owned(),
        name: "A".to_owned(),
        named: true,
        cover: None,
        description: None,
        owner: None,
        tracks: None,
        access: None,
    }]);
    assert_eq!(queue.apply(&playlists, &mut transport), None);
    assert_eq!(end(&mut queue, &mut transport), Some(play("t1")));
}

#[test]
fn duplicate_adjacent_uris_behave_like_any_other_pair() {
    let duplicates = vec![track("same"), track("same")];
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(
        queue.start(&duplicates, 0, None, &mut transport),
        Some(play("same"))
    );
    clear_guard(&mut queue, &mut transport);
    assert_eq!(end(&mut queue, &mut transport), Some(play("same")));

    clear_guard(&mut queue, &mut transport);
    assert_eq!(end(&mut queue, &mut transport), None);
}

#[test]
fn start_with_an_out_of_range_index_is_none() {
    let tracks = tracks(2);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert_eq!(queue.start(&tracks, 2, None, &mut transport), None);
    assert_eq!(queue.start(&[], 0, None, &mut transport), None);
    assert!(!queue.can_toggle());
}

#[test]
fn an_empty_queue_has_nothing_to_step_through() {
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    assert!(!queue.can_next());
    assert!(!queue.can_previous());
    assert!(!queue.can_toggle());
    assert_eq!(queue.next(&mut transport), None);
    assert_eq!(queue.previous(1_000, &mut transport), None);
    assert_eq!(queue.toggle(&mut transport), None);
}

#[test]
fn a_populated_queue_can_step_either_way() {
    let tracks = tracks(2);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&tracks, 0, None, &mut transport);
    assert!(queue.can_next());
    assert!(queue.can_previous());
    assert!(queue.can_toggle());
}

#[test]
fn start_over_an_episode_entry_names_it_in_now_playing() {
    let episode = takofy_model::Episode {
        uri: "spotify:episode:abc".to_owned(),
        title: "210: A dry labyrinth".to_owned(),
        show: "Lateral with Tom Scott".to_owned(),
        show_uri: Some("spotify:show:lateral".to_owned()),
        description: None,
        published: None,
        duration_ms: Some(2_773_461),
        cover: None,
        named: true,
    };
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let command = queue.start(&[episode.entry()], 0, None, &mut transport);

    assert_eq!(
        command,
        Some(crate::message::Command::Play {
            uri: "spotify:episode:abc".to_owned()
        })
    );
    let now = transport.now_playing.expect("the episode is playing");
    assert_eq!(now.title, "210: A dry labyrinth");
    assert_eq!(now.artists, "Lateral with Tom Scott");
}

#[test]
fn goto_passes_the_entrys_cover_into_now_playing() {
    let cover =
        takofy_model::CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url");
    let mut entry = track("a");
    entry.cover = Some(takofy_model::CoverSet::single(cover.clone()));
    let mut queue = Queue::default();
    let mut transport = Transport::default();

    let _ = queue.start(&[entry], 0, None, &mut transport);

    assert_eq!(
        transport.now_playing.expect("a named track").cover,
        Some(takofy_model::CoverSet::single(cover))
    );
}

#[test]
fn goto_passes_the_entrys_artists_into_now_playing() {
    let mut queue = Queue::default();
    let mut transport = Transport::default();

    let _ = queue.start(&[track_by("a", "A Band")], 0, None, &mut transport);

    assert_eq!(
        transport.now_playing.expect("a named track").artists,
        "A Band"
    );
}

#[test]
fn goto_passes_the_entrys_album_and_first_artist_uris() {
    let mut entry = track_by("a", "A Band, Another Band");
    entry.album_uri = Some("spotify:album:one".to_owned());
    entry.artist_uris = vec![
        "spotify:artist:first".to_owned(),
        "spotify:artist:second".to_owned(),
    ];
    let mut queue = Queue::default();
    let mut transport = Transport::default();

    let _ = queue.start(&[entry], 0, None, &mut transport);

    let now = transport.now_playing.expect("a named track");
    assert_eq!(now.album_uri.as_deref(), Some("spotify:album:one"));
    assert_eq!(now.artist_uri.as_deref(), Some("spotify:artist:first"));
}
