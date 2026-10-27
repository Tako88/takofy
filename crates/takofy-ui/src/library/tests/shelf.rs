use super::super::*; // the module under test
use super::*; // the slice's shared fixtures, from tests/mod.rs

#[test]
fn the_next_page_write_carries_the_rename_replys_revision() {
    let mut library = sidebar_library(&["a"], 0x07, true, None);
    library.select("a");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t0"), track("t1")],
        cover: None,
        access: access(0x07, true),
    });
    rename_pending(&mut library, "a", crate::naming::Surface::Row);
    library.apply(&Update::PlaylistRenamed {
        playlist: "a".to_owned(),
        name: "New".to_owned(),
        revision: revision(0x09),
    });
    let first = match &library.tracks {
        Load::Ready(tracks) => tracks.tracks[0].clone(),
        Load::Idle | Load::Loading | Load::Failed(_) => panic!("the page is ready"),
    };

    let (command, _) = library
        .move_tracks("a", &[0], 2, &[first])
        .expect("the move is taken");

    assert_eq!(
        command,
        Command::MoveTrack {
            playlist: "a".to_owned(),
            revision: revision(0x09),
            moves: vec![Move {
                from: 0,
                length: 1,
                to: 2,
            }],
        },
        "the next move named the pre-rename base"
    );
}

/// The rows a landed shelf play hands the queue.
fn start(uri: &str, uris: &[&str]) -> Start {
    Start {
        origin: uri.to_owned(),
        tracks: uris.iter().map(|uri| track(uri)).collect(),
    }
}

#[test]
fn a_shelf_play_on_a_playlist_asks_for_its_tracks_once() {
    let mut library = Library::default();
    assert_eq!(library.play_request(), None, "nothing was pressed");
    assert!(!library.has_landed_play());

    assert!(library.want_play("a", SidebarKind::Playlists));
    assert_eq!(
        library.play_request(),
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        })
    );
    assert_eq!(
        library.play_request(),
        None,
        "one press is one ask on the wire"
    );
    assert_eq!(library.page(), None, "a shelf play navigates nowhere");
    assert_eq!(library.tracks, Load::Idle, "the page's rows were marked");
    assert_eq!(library.take_play(), None, "nothing has landed yet");
}

#[test]
fn a_shelf_play_on_an_album_asks_for_the_album() {
    let mut library = Library::default();
    assert!(library.want_play("spotify:album:r", SidebarKind::Albums));

    assert_eq!(
        library.play_request(),
        Some(Command::LoadAlbum {
            album: "spotify:album:r".to_owned()
        })
    );
    assert_eq!(library.play_request(), None);
    assert_eq!(library.page(), None);
    assert_eq!(library.album, Load::Idle, "the album page was marked");
}

#[test]
fn a_shelf_play_on_held_playlist_rows_starts_without_a_load() {
    let mut library = page_with("a", &["t0", "t1"], None);

    assert!(library.want_play("a", SidebarKind::Playlists));
    assert!(library.has_landed_play());
    assert_eq!(
        library.play_request(),
        None,
        "rows the window holds were asked for again"
    );
    assert_eq!(library.take_play(), Some(start("a", &["t0", "t1"])));
    assert_eq!(library.take_play(), None, "a start was taken twice");
    assert_eq!(
        library.tracks,
        Load::Ready(tracks_for("a", vec![track("t0"), track("t1")])),
        "the page's own rows are still the page's"
    );
}

#[test]
fn a_shelf_play_on_a_held_album_starts_without_a_load() {
    let mut library = Library::default();
    library.open(album("spotify:album:r"));
    library.apply(&Update::Album(album_detail("spotify:album:r")));

    assert!(library.want_play("spotify:album:r", SidebarKind::Albums));
    assert!(library.has_landed_play());
    assert_eq!(library.play_request(), None);
    assert_eq!(library.take_play(), Some(start("spotify:album:r", &["t"])));
}

#[test]
fn held_rows_of_another_playlist_do_not_start_a_shelf_play() {
    let mut library = page_with("b", &["p0"], None);

    assert!(library.want_play("a", SidebarKind::Playlists));
    assert!(
        !library.has_landed_play(),
        "another list's rows started this play"
    );
    assert_eq!(
        library.play_request(),
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        })
    );
}

#[test]
fn rows_that_land_for_a_shelf_play_are_taken_once() {
    let mut library = Library::default();
    assert!(library.want_play("a", SidebarKind::Playlists));
    assert_eq!(
        library.play_request(),
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        })
    );
    assert!(!library.has_landed_play());

    let reply = Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t0"), track("t1")],
        cover: None,
        access: None,
    };
    library.apply(&reply);
    assert!(library.has_landed_play());
    assert_eq!(library.take_play(), Some(start("a", &["t0", "t1"])));
    assert_eq!(library.take_play(), None, "a landed play started twice");
    assert!(!library.has_landed_play());
}

#[test]
fn a_shelf_play_does_not_replace_the_page_rows() {
    let mut library = page_with("b", &["p0"], None);
    assert!(library.want_play("a", SidebarKind::Playlists));
    assert_eq!(
        library.play_request(),
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        })
    );

    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t0")],
        cover: None,
        access: None,
    });

    assert!(library.has_landed_play());
    assert_eq!(
        library.tracks,
        Load::Ready(tracks_for("b", vec![track("p0")])),
        "the shelf play wrote the page's rows"
    );
    assert_eq!(library.take_play(), Some(start("a", &["t0"])));
}

#[test]
fn a_second_shelf_play_replaces_the_first_and_the_first_reply_is_ignored() {
    let mut library = Library::default();
    assert!(library.want_play("a", SidebarKind::Playlists));
    assert_eq!(
        library.play_request(),
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        })
    );
    assert!(library.want_play("b", SidebarKind::Playlists));
    assert_eq!(
        library.play_request(),
        Some(Command::LoadTracks {
            playlist: "b".to_owned()
        })
    );

    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("ta")],
        cover: None,
        access: None,
    });
    assert!(
        !library.has_landed_play(),
        "the replaced play took its late reply"
    );

    library.apply(&Update::Tracks {
        playlist: "b".to_owned(),
        tracks: vec![track("tb")],
        cover: None,
        access: None,
    });
    assert_eq!(library.take_play(), Some(start("b", &["tb"])));
}

#[test]
fn pressing_the_same_shelf_play_twice_asks_once() {
    let mut library = Library::default();

    assert!(library.want_play("a", SidebarKind::Playlists));
    assert!(library.want_play("a", SidebarKind::Playlists));
    assert!(library.want_play("a", SidebarKind::Playlists));
    assert_eq!(
        library.play_request(),
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        }),
        "a repeat press asked twice"
    );

    assert!(library.want_play("a", SidebarKind::Playlists));
    assert_eq!(
        library.play_request(),
        None,
        "a repeat press while the rows are in flight asked again"
    );
}

#[test]
fn the_same_rows_landing_twice_start_once() {
    let mut library = Library::default();
    assert!(library.want_play("a", SidebarKind::Playlists));
    assert!(library.play_request().is_some());

    for _ in 0..2 {
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t0")],
            cover: None,
            access: None,
        });
    }

    assert_eq!(library.take_play(), Some(start("a", &["t0"])));
    assert_eq!(library.take_play(), None, "a second landing started again");
}

#[test]
fn a_failed_shelf_load_starts_nothing_and_says_so() {
    let mut library = Library::default();
    assert!(library.want_play("a", SidebarKind::Playlists));
    assert!(library.play_request().is_some());

    library.apply(&Update::TracksFailed {
        playlist: "a".to_owned(),
        failure: Failure::Load,
    });

    assert_eq!(library.play_notice, Some(Failure::Load));
    assert_eq!(library.play_request(), None, "a failed play asked again");
    assert_eq!(library.take_play(), None, "a failed play started rows");

    // Pressing play again is the retry.
    assert!(library.want_play("a", SidebarKind::Playlists));
    assert_eq!(library.play_notice, None, "the retry kept the sentence");
    assert_eq!(
        library.play_request(),
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        })
    );
}

#[test]
fn a_failed_shelf_album_load_starts_nothing_and_says_so() {
    let mut library = Library::default();
    assert!(library.want_play("spotify:album:r", SidebarKind::Albums));
    assert!(library.play_request().is_some());

    library.apply(&Update::AlbumFailed {
        album: "spotify:album:r".to_owned(),
        failure: Failure::Other,
    });

    assert_eq!(library.play_notice, Some(Failure::Other));
    assert_eq!(library.take_play(), None);
}

#[test]
fn a_failure_for_another_uri_leaves_the_shelf_play_waiting() {
    let mut library = Library::default();
    assert!(library.want_play("a", SidebarKind::Playlists));

    library.apply(&Update::TracksFailed {
        playlist: "b".to_owned(),
        failure: Failure::Load,
    });

    assert_eq!(library.play_notice, None);
    assert_eq!(
        library.play_request(),
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        }),
        "another list's failure dropped this play"
    );
}

#[test]
fn a_failure_for_the_same_uri_drops_the_play_and_says_so() {
    // A page's own ask and a shelf play name the same object, and the reply
    // cannot say which asked, so the failure is taken for both.
    let mut library = Library::default();
    library.select("a");
    assert!(library.want_play("a", SidebarKind::Playlists));

    library.apply(&Update::TracksFailed {
        playlist: "a".to_owned(),
        failure: Failure::Load,
    });

    assert_eq!(library.play_notice, Some(Failure::Load));
    assert_eq!(library.take_play(), None);
    assert_eq!(library.tracks, Load::Failed(Failure::Load));
}

#[test]
fn a_shelf_play_does_not_outlive_the_page_it_was_pressed_on() {
    let landed = |library: &mut Library| {
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t0")],
            cover: None,
            access: None,
        });
        library.take_play()
    };

    let mut select = Library::default();
    assert!(select.want_play("a", SidebarKind::Playlists));
    select.select("b");
    assert_eq!(landed(&mut select), None, "a select kept the play");

    let mut open = Library::default();
    assert!(open.want_play("a", SidebarKind::Playlists));
    open.open(album("album"));
    assert_eq!(landed(&mut open), None, "an open kept the play");

    let mut back = Library::default();
    back.select("a");
    back.open(album("album"));
    assert!(back.want_play("a", SidebarKind::Playlists));
    back.back();
    assert_eq!(landed(&mut back), None, "a back kept the play");

    let mut forward = Library::default();
    forward.select("a");
    forward.open(album("album"));
    forward.back();
    assert!(forward.want_play("a", SidebarKind::Playlists));
    forward.forward();
    assert_eq!(landed(&mut forward), None, "a forward kept the play");

    let mut root = Library::default();
    assert!(root.want_play("a", SidebarKind::Playlists));
    root.select("a");
    root.to_playlists();
    assert_eq!(landed(&mut root), None, "a round trip kept the play");
}

#[test]
fn a_playlist_removed_while_its_play_is_pending_drops_the_play() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![
        playlist("a", "A"),
        playlist("b", "B"),
    ]));
    assert!(library.want_play("a", SidebarKind::Playlists));

    library.apply(&Update::Playlists(vec![playlist("b", "B")]));

    assert_eq!(library.play_request(), None, "a removed playlist was asked");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t0")],
        cover: None,
        access: None,
    });
    assert_eq!(library.take_play(), None, "a removed playlist started");
}

#[test]
fn a_shelf_play_on_an_artist_or_a_show_is_refused() {
    assert!(Library::playable(SidebarKind::Playlists));
    assert!(Library::playable(SidebarKind::Albums));
    assert!(!Library::playable(SidebarKind::Artists));
    assert!(!Library::playable(SidebarKind::Podcasts));

    let mut library = Library::default();
    assert!(!library.want_play("spotify:artist:a", SidebarKind::Artists));
    assert!(!library.want_play("spotify:show:s", SidebarKind::Podcasts));
    assert_eq!(library.play_request(), None);
    assert!(!library.has_landed_play());
    assert_eq!(library.play_notice, None);
}

#[test]
fn an_empty_playlist_clears_the_shelf_play_and_starts_nothing() {
    let mut library = Library::default();
    assert!(library.want_play("a", SidebarKind::Playlists));
    assert!(library.play_request().is_some());

    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: Vec::new(),
        cover: None,
        access: None,
    });

    assert!(library.has_landed_play(), "the empty rows did not land");
    assert_eq!(library.take_play(), None, "an empty list started");
    assert_eq!(library.take_play(), None, "the empty play was kept");
    assert_eq!(library.play_notice, None, "an empty list is not a failure");
}

#[test]
fn a_new_press_and_a_page_change_clear_the_shelf_notice() {
    let mut library = Library::default();
    library.select("held");
    assert!(library.want_play("a", SidebarKind::Playlists));
    assert!(library.play_request().is_some());
    library.apply(&Update::TracksFailed {
        playlist: "a".to_owned(),
        failure: Failure::Load,
    });
    assert_eq!(library.play_notice, Some(Failure::Load));

    assert!(library.want_play("b", SidebarKind::Playlists));
    assert_eq!(library.play_notice, None, "a new press kept the sentence");

    assert!(library.play_request().is_some());
    library.apply(&Update::TracksFailed {
        playlist: "b".to_owned(),
        failure: Failure::Load,
    });
    assert_eq!(library.play_notice, Some(Failure::Load));

    library.to_playlists();
    assert_eq!(library.play_notice, None, "a page change kept the sentence");
}
