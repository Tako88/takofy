use super::super::*;
use super::test_support::*;
use crate::lyrics::Tab;
use crate::player::NowPlaying;
use takofy_model::TrackLyrics;

#[test]
fn a_lyrics_reply_reaches_the_panel_whatever_the_page() {
    let mut library = Library::default();
    library.open(album("spotify:album:a"));
    library.lyrics.select(Tab::Lyrics);
    let now = NowPlaying {
        uri: "spotify:track:a".to_owned(),
        title: "A track".to_owned(),
        artists: String::new(),
        ..Default::default()
    };
    assert_eq!(
        library.lyrics.request(Some(&now)),
        Some(Command::LoadLyrics {
            track: "spotify:track:a".to_owned(),
        })
    );

    let reply = TrackLyrics::default();
    library.apply(&Update::Lyrics {
        track: "spotify:track:a".to_owned(),
        lyrics: reply.clone(),
    });
    assert_eq!(library.lyrics.load(), &Load::Ready(reply));

    let next = NowPlaying {
        uri: "spotify:track:b".to_owned(),
        title: "Another track".to_owned(),
        artists: String::new(),
        ..Default::default()
    };
    library.lyrics.sync(Some(&next));
    assert_eq!(
        library.lyrics.request(Some(&next)),
        Some(Command::LoadLyrics {
            track: "spotify:track:b".to_owned(),
        })
    );

    library.apply(&Update::LyricsFailed {
        track: "spotify:track:b".to_owned(),
        failure: Failure::Lyrics,
    });
    assert_eq!(library.lyrics.load(), &Load::Failed(Failure::Lyrics));
}

/// The reply path: only the wanted album's answer lands, the first one wins,
/// and a failure is recorded the same way a playlist's is.
#[test]
fn an_album_reply_lands_only_on_its_own_want() {
    let mut library = Library::default();
    library.open(album("spotify:album:a"));
    library.apply(&Update::Album(album_detail("spotify:album:a")));

    assert_eq!(library.album, Load::Ready(album_detail("spotify:album:a")));

    library.apply(&Update::Album(album_detail("spotify:album:b")));
    assert_eq!(
        library.album,
        Load::Ready(album_detail("spotify:album:a")),
        "a reply nobody asked for is discarded"
    );
}

#[test]
fn a_stale_album_reply_is_discarded() {
    let mut library = Library::default();
    library.open(album("spotify:album:a"));
    library.open(album("spotify:album:b"));

    library.apply(&Update::Album(album_detail("spotify:album:a")));

    assert_eq!(library.album, Load::Loading, "A's reply is not B's");
    library.apply(&Update::Album(album_detail("spotify:album:b")));
    assert_eq!(library.album, Load::Ready(album_detail("spotify:album:b")));
}

#[test]
fn a_stale_album_failure_is_discarded() {
    let mut library = Library::default();
    library.open(album("spotify:album:a"));
    library.open(album("spotify:album:b"));

    library.apply(&Update::AlbumFailed {
        album: "spotify:album:a".to_owned(),
        failure: Failure::Load,
    });
    assert_eq!(library.album, Load::Loading, "A's failure is not B's");

    library.apply(&Update::AlbumFailed {
        album: "spotify:album:b".to_owned(),
        failure: Failure::Load,
    });
    assert_eq!(library.album, Load::Failed(Failure::Load));
}

#[test]
fn the_first_album_reply_wins() {
    let mut library = Library::default();
    library.open(album("spotify:album:a"));
    let first = AlbumDetail {
        name: "First".to_owned(),
        ..album_detail("spotify:album:a")
    };
    let second = AlbumDetail {
        name: "Second".to_owned(),
        ..album_detail("spotify:album:a")
    };

    library.apply(&Update::Album(first.clone()));
    library.apply(&Update::Album(second));

    assert_eq!(library.album, Load::Ready(first));
}

#[test]
fn back_onto_a_ready_album_needs_no_command() {
    let mut library = Library::default();
    library.open(album("spotify:album:a"));
    library.apply(&Update::Album(album_detail("spotify:album:a")));
    assert_eq!(
        library.open(artist("spotify:artist:a")),
        Some(artist_command("spotify:artist:a")),
        "the artist page starts its own load"
    );

    assert_eq!(library.back(), None, "a ready album is already settled");
    assert_eq!(library.page(), Some(&album("spotify:album:a")));
}

#[test]
fn back_onto_a_failed_album_reloads_it() {
    let mut library = Library::default();
    library.open(album("spotify:album:a"));
    library.apply(&Update::AlbumFailed {
        album: "spotify:album:a".to_owned(),
        failure: Failure::Load,
    });
    assert_eq!(
        library.open(artist("spotify:artist:a")),
        Some(artist_command("spotify:artist:a"))
    );

    assert_eq!(
        library.back(),
        Some(Command::LoadAlbum {
            album: "spotify:album:a".to_owned()
        }),
        "a failed album has nothing to draw, so Back asks again"
    );
    assert_eq!(library.album, Load::Loading);
}

#[test]
fn reopening_a_failed_album_retries_it() {
    let mut library = Library::default();
    library.open(album("spotify:album:a"));
    library.apply(&Update::AlbumFailed {
        album: "spotify:album:a".to_owned(),
        failure: Failure::Load,
    });

    assert_eq!(
        library.open(album("spotify:album:a")),
        Some(Command::LoadAlbum {
            album: "spotify:album:a".to_owned()
        }),
        "the page's Retry is this call, so it must re-issue the load"
    );
    assert_eq!(library.album, Load::Loading);
}

#[test]
fn to_playlists_clears_the_album_want_and_drops_an_unsettled_load() {
    let mut loading = Library::default();
    loading.open(album("spotify:album:a"));
    loading.to_playlists();
    assert_eq!(loading.album, Load::Idle, "a load nobody wants is dropped");
    assert_eq!(
        loading.open(album("spotify:album:a")),
        Some(Command::LoadAlbum {
            album: "spotify:album:a".to_owned()
        }),
        "the want was cleared, so the album is asked for again"
    );

    let mut failed = Library::default();
    failed.open(album("spotify:album:a"));
    failed.apply(&Update::AlbumFailed {
        album: "spotify:album:a".to_owned(),
        failure: Failure::Load,
    });
    failed.to_playlists();
    assert_eq!(failed.album, Load::Idle);

    let mut ready = Library::default();
    ready.open(album("spotify:album:a"));
    ready.apply(&Update::Album(album_detail("spotify:album:a")));
    ready.to_playlists();
    assert_eq!(
        ready.album,
        Load::Ready(album_detail("spotify:album:a")),
        "a settled album is kept for the page Back returns to"
    );
    assert_eq!(
        ready.open(album("spotify:album:a")),
        Some(Command::LoadAlbum {
            album: "spotify:album:a".to_owned()
        }),
        "the want is cleared even when the album stays ready"
    );
}

#[test]
fn a_playlist_prune_leaves_the_album_alone() {
    let mut library = Library::default();
    library.select("a");
    library.open(album("spotify:album:a"));
    library.apply(&Update::Album(album_detail("spotify:album:a")));

    // The list no longer holds the playlist, so its page is pruned.
    library.apply(&Update::Playlists(vec![playlist("b", "B")]));

    assert_eq!(library.page(), Some(&album("spotify:album:a")));
    assert_eq!(
        library.album,
        Load::Ready(album_detail("spotify:album:a")),
        "a playlist prune never touches the album"
    );
    assert_eq!(
        library.current_album(),
        Some(&album_detail("spotify:album:a"))
    );
}

#[test]
fn current_album_is_none_while_loading() {
    let mut library = Library::default();
    library.open(album("spotify:album:a"));
    assert_eq!(library.current_album(), None);
}

#[test]
fn current_album_is_none_when_a_ready_detail_names_another_page() {
    let mut library = Library::default();
    library.open(album("spotify:album:b"));
    library.album = Load::Ready(album_detail("spotify:album:a"));

    assert_eq!(
        library.current_album(),
        None,
        "A's detail must not name B's page"
    );
}

#[test]
fn current_album_is_some_when_it_names_the_page() {
    let mut library = Library::default();
    library.open(album("spotify:album:a"));
    library.apply(&Update::Album(album_detail("spotify:album:a")));

    assert_eq!(
        library.current_album(),
        Some(&album_detail("spotify:album:a"))
    );
}
