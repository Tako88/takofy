use super::super::*; // the module under test
use super::*; // the slice's shared fixtures, from tests/mod.rs

#[test]
fn open_an_artist_starts_its_load_and_keeps_the_album() {
    let mut library = Library::default();
    library.open(album("spotify:album:a"));
    library.apply(&Update::Album(album_detail("spotify:album:a")));

    assert_eq!(
        library.open(artist("spotify:artist:a")),
        Some(artist_command("spotify:artist:a"))
    );
    assert_eq!(library.page(), Some(&artist("spotify:artist:a")));
    assert_eq!(library.artist, Load::Loading);
    assert_eq!(
        library.album,
        Load::Ready(album_detail("spotify:album:a")),
        "the album behind the artist stays ready for Back"
    );
}

#[test]
fn an_artist_reply_lands_only_on_its_own_want_while_loading() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
    assert_eq!(
        library.artist,
        Load::Ready(artist_detail("spotify:artist:a"))
    );

    library.apply(&Update::Artist(artist_detail("spotify:artist:b")));
    assert_eq!(
        library.artist,
        Load::Ready(artist_detail("spotify:artist:a")),
        "a reply nobody asked for is discarded"
    );
}

#[test]
fn a_stale_artist_reply_is_discarded() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    library.open(artist("spotify:artist:b"));

    library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
    assert_eq!(library.artist, Load::Loading, "A's reply is not B's");

    library.apply(&Update::Artist(artist_detail("spotify:artist:b")));
    assert_eq!(
        library.artist,
        Load::Ready(artist_detail("spotify:artist:b"))
    );
}

#[test]
fn a_stale_artist_failure_is_discarded() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    library.open(artist("spotify:artist:b"));

    library.apply(&Update::ArtistFailed {
        artist: "spotify:artist:a".to_owned(),
        failure: Failure::Load,
    });
    assert_eq!(library.artist, Load::Loading, "A's failure is not B's");

    library.apply(&Update::ArtistFailed {
        artist: "spotify:artist:b".to_owned(),
        failure: Failure::Load,
    });
    assert_eq!(library.artist, Load::Failed(Failure::Load));
}

#[test]
fn the_first_artist_reply_wins() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    let first = ArtistDetail {
        name: "First".to_owned(),
        ..artist_detail("spotify:artist:a")
    };
    let second = ArtistDetail {
        name: "Second".to_owned(),
        ..artist_detail("spotify:artist:a")
    };

    library.apply(&Update::Artist(first.clone()));
    library.apply(&Update::Artist(second));

    assert_eq!(library.artist, Load::Ready(first));
}

#[test]
fn back_onto_a_ready_artist_needs_no_command() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
    library.to_playlists();

    assert_eq!(library.back(), Some(artist_command("spotify:artist:a")));
    assert_eq!(library.artist, Load::Loading);
    library.apply(&Update::Artist(artist_detail("spotify:artist:a")));

    library.select("p");
    assert_eq!(library.back(), None, "a ready artist is already settled");
    assert_eq!(library.page(), Some(&artist("spotify:artist:a")));
}

#[test]
fn back_onto_a_failed_artist_reloads_it() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    library.apply(&Update::ArtistFailed {
        artist: "spotify:artist:a".to_owned(),
        failure: Failure::Load,
    });
    library.select("p");

    assert_eq!(
        library.back(),
        Some(artist_command("spotify:artist:a")),
        "a failed artist has nothing to draw, so Back asks again"
    );
    assert_eq!(library.artist, Load::Loading);
}

#[test]
fn reopening_a_failed_artist_retries_it() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    library.apply(&Update::ArtistFailed {
        artist: "spotify:artist:a".to_owned(),
        failure: Failure::Load,
    });

    assert_eq!(
        library.open(artist("spotify:artist:a")),
        Some(artist_command("spotify:artist:a")),
        "the page's Retry is this call, so it must re-issue the load"
    );
    assert_eq!(library.artist, Load::Loading);
}

#[test]
fn artist_page_hands_out_the_filter() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    library.filter.choose(Chip::Kind(ReleaseKind::Single));
    {
        let page = library.artist_page();
        assert_eq!(page.filter.chosen(), Chip::Kind(ReleaseKind::Single));
        page.filter.choose(Chip::All);
    }
    assert_eq!(
        library.filter.chosen(),
        Chip::All,
        "the page borrows the library's own filter, not a copy"
    );
}

#[test]
fn to_playlists_clears_the_artist_want_and_drops_an_unsettled_load() {
    let mut loading = Library::default();
    loading.open(artist("spotify:artist:a"));
    loading.to_playlists();
    assert_eq!(loading.artist, Load::Idle, "a load nobody wants is dropped");
    assert_eq!(
        loading.open(artist("spotify:artist:a")),
        Some(artist_command("spotify:artist:a")),
        "the want was cleared, so the artist is asked for again"
    );

    let mut failed = Library::default();
    failed.open(artist("spotify:artist:a"));
    failed.apply(&Update::ArtistFailed {
        artist: "spotify:artist:a".to_owned(),
        failure: Failure::Load,
    });
    failed.to_playlists();
    assert_eq!(failed.artist, Load::Idle);

    let mut ready = Library::default();
    ready.open(artist("spotify:artist:a"));
    ready.apply(&Update::Artist(artist_detail("spotify:artist:a")));
    ready.to_playlists();
    assert_eq!(
        ready.artist,
        Load::Ready(artist_detail("spotify:artist:a")),
        "a settled artist is kept for the page Back returns to"
    );
    assert_eq!(
        ready.open(artist("spotify:artist:a")),
        Some(artist_command("spotify:artist:a")),
        "the want is cleared even when the artist stays ready"
    );
}

#[test]
fn to_playlists_resets_the_follow_state() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    library.follow.apply(&Update::Followed {
        artist: "spotify:artist:a".to_owned(),
        followed: true,
    });
    assert_eq!(library.follow.label(), "Following");

    library.to_playlists();

    assert_eq!(library.follow.label(), "Follow");
    assert_eq!(library.follow.artist(), None);
}

#[test]
fn current_artist_is_none_while_loading_or_for_another_uri() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    assert_eq!(library.current_artist(), None);

    library.artist = Load::Ready(artist_detail("spotify:artist:b"));
    assert_eq!(
        library.current_artist(),
        None,
        "B's detail must not name A's page"
    );

    library.artist = Load::Ready(artist_detail("spotify:artist:a"));
    assert_eq!(
        library.current_artist(),
        Some(&artist_detail("spotify:artist:a"))
    );
}

#[test]
fn a_playlist_prune_leaves_the_artist_alone() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("p", "P")]));
    library.select("p");
    library.open(artist("spotify:artist:a"));
    library.apply(&Update::Artist(artist_detail("spotify:artist:a")));

    library.apply(&Update::Playlists(vec![playlist("q", "Q")]));

    assert_eq!(library.page(), Some(&artist("spotify:artist:a")));
    assert_eq!(
        library.artist,
        Load::Ready(artist_detail("spotify:artist:a")),
        "a playlist prune never touches the artist"
    );
}

#[test]
fn an_artist_reply_retains_only_its_own_releases() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    library.apply(&Update::Artist(artist_detail("spotify:artist:a")));

    // The first release is asked for, so its entry is in flight rather than
    // idle: that is what makes a dropped entry distinguishable from a
    // retained one below.
    library.releases.look("spotify:album:r1");
    assert_eq!(
        library.releases.requests(),
        vec![Command::LoadReleaseTracks {
            release: "spotify:album:r1".to_owned(),
            tracks: vec!["spotify:album:r1".to_owned()],
        }]
    );

    // Coming back to the artist reloads it; the reply no longer names that
    // release.
    library.open(artist("spotify:artist:b"));
    library.open(artist("spotify:artist:a"));
    let mut other = artist_detail("spotify:artist:a");
    other.releases = vec![release("spotify:album:r2")];
    library.apply(&Update::Artist(other));

    assert_eq!(
        library.releases.look("spotify:album:r1"),
        crate::releases::Labels::Idle,
        "a release the artist no longer names is forgotten"
    );
    library.releases.look("spotify:album:r2");
    assert_eq!(
        library.releases.requests(),
        vec![Command::LoadReleaseTracks {
            release: "spotify:album:r2".to_owned(),
            tracks: vec!["spotify:album:r2".to_owned()],
        }]
    );
}
