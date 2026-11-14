use super::*;

fn playlist(uri: &str) -> Option<Page> {
    Some(Page::Playlist(uri.to_owned()))
}

fn album(uri: &str) -> Option<Page> {
    Some(Page::Album(uri.to_owned()))
}

fn artist(uri: &str) -> Option<Page> {
    Some(Page::Artist(uri.to_owned()))
}

fn show(uri: &str) -> Option<Page> {
    Some(Page::Show(uri.to_owned()))
}

fn search(query: &str) -> Option<Page> {
    Some(Page::Search(query.to_owned()))
}

fn home() -> Option<Page> {
    Some(Page::Home)
}

#[test]
fn a_show_uri_credits_a_show_page() {
    assert_eq!(
        Page::credit("spotify:show:abc"),
        Page::Show("spotify:show:abc".to_owned())
    );
}

#[test]
fn an_artist_uri_credits_an_artist_page() {
    assert_eq!(
        Page::credit("spotify:artist:abc"),
        Page::Artist("spotify:artist:abc".to_owned())
    );
}

#[test]
fn anything_else_credits_an_artist_page() {
    assert_eq!(
        Page::credit("spotify:album:abc"),
        Page::Artist("spotify:album:abc".to_owned())
    );
}

#[test]
fn go_records_the_previous_page_and_clears_forward() {
    let mut history = History::default();
    assert!(history.go(playlist("a")));
    assert!(history.go(playlist("b")));
    assert!(history.back());

    assert_eq!(history.current(), playlist("a").as_ref());
    assert!(history.can_forward());

    assert!(history.go(playlist("c")));
    assert_eq!(history.current(), playlist("c").as_ref());
    assert!(!history.can_forward(), "a new page clears forward");
}

#[test]
fn go_to_the_current_page_is_a_no_op() {
    let mut history = History::default();
    history.go(playlist("a"));

    assert!(!history.go(playlist("a")));

    // The single move to `a` left only the root behind it.
    assert!(history.back());
    assert_eq!(history.current(), None);
    assert!(!history.can_back(), "the no-op must not grow back");
}

#[test]
fn back_then_forward_round_trips() {
    let mut history = History::default();
    history.go(playlist("a"));
    history.go(album("b"));

    assert!(history.back());
    assert_eq!(history.current(), playlist("a").as_ref());
    assert!(history.forward());
    assert_eq!(history.current(), album("b").as_ref());
}

#[test]
fn back_and_forward_on_an_empty_history_change_nothing() {
    let mut history = History::default();

    assert!(!history.back());
    assert_eq!(history.current(), None);
    assert!(!history.can_back());

    assert!(!history.forward());
    assert_eq!(history.current(), None);
    assert!(!history.can_forward());
}

#[test]
fn a_new_page_after_back_clears_forward() {
    let mut history = History::default();
    history.go(playlist("a"));
    history.go(playlist("b"));
    history.back();

    history.go(playlist("c"));

    assert!(!history.can_forward());
    assert!(!history.forward());
}

#[test]
fn the_cap_drops_the_oldest_entry() {
    let mut history = History::default();
    for index in 0..=MAX {
        history.go(playlist(&format!("p{index}")));
    }

    let mut steps = 0;
    while history.back() {
        steps += 1;
    }

    assert_eq!(steps, MAX, "the history is capped at {MAX}");
    // The oldest entry left is the first page pushed after the dropped
    // root, so the walk ends there rather than at the root.
    assert_eq!(history.current(), playlist("p0").as_ref());
}

#[test]
fn retain_drops_a_rejected_page_from_back() {
    let mut history = History::default();
    history.go(playlist("a"));
    history.go(album("x"));
    history.go(playlist("b"));

    history.retain(|page| *page != Page::Album("x".to_owned()));

    assert_eq!(history.current(), playlist("b").as_ref());
    assert!(history.back());
    assert_eq!(history.current(), playlist("a").as_ref(), "x is skipped");
    assert!(history.back());
    assert_eq!(history.current(), None);
}

#[test]
fn retain_drops_a_rejected_page_from_forward() {
    let mut history = History::default();
    history.go(playlist("a"));
    history.go(album("x"));
    history.back();

    history.retain(|page| *page != Page::Album("x".to_owned()));

    assert_eq!(history.current(), playlist("a").as_ref());
    assert!(!history.can_forward());
}

#[test]
fn retain_falls_back_to_the_root_when_the_current_page_fails() {
    let mut history = History::default();
    history.go(playlist("a"));

    history.retain(|_| false);

    assert_eq!(history.current(), None);
    assert!(!history.can_back());
}

#[test]
fn retain_collapses_consecutive_duplicates() {
    let mut history = History::default();
    history.go(album("a"));
    history.go(None);
    history.go(album("b"));

    history.retain(|page| *page != Page::Album("a".to_owned()));

    assert_eq!(history.current(), album("b").as_ref());
    assert!(history.back());
    assert_eq!(history.current(), None);
    assert!(
        !history.can_back(),
        "the two root entries must collapse into one"
    );
}

#[test]
fn retain_collapses_a_back_entry_equal_to_the_current_page() {
    let mut history = History::default();
    history.go(album("a"));

    history.retain(|page| *page != Page::Album("a".to_owned()));

    assert_eq!(history.current(), None);
    assert!(!history.can_back(), "the root must not repeat the root");
}

#[test]
fn retain_keeps_non_playlist_pages() {
    let mut history = History::default();
    history.go(album("album"));
    history.go(playlist("p"));
    history.go(artist("artist"));
    history.go(show("show"));

    history.retain(|page| !matches!(page, Page::Playlist(_)));

    assert_eq!(history.current(), show("show").as_ref());
    assert!(history.back());
    assert_eq!(history.current(), artist("artist").as_ref());
    assert!(history.back());
    assert_eq!(history.current(), album("album").as_ref());
    assert!(history.back());
    assert_eq!(history.current(), None);
}

#[test]
fn retain_keeps_non_playlist_pages_in_forward() {
    let mut history = History::default();
    history.go(album("album"));
    history.go(artist("artist"));
    history.go(show("show"));
    history.back();
    history.back();
    assert_eq!(history.current(), album("album").as_ref());

    history.retain(|page| !matches!(page, Page::Playlist(_)));

    assert!(history.forward());
    assert_eq!(history.current(), artist("artist").as_ref());
    assert!(history.forward());
    assert_eq!(history.current(), show("show").as_ref());
}

#[test]
fn a_page_names_its_uri_and_kind() {
    let cases = [
        (Page::Home, "", "Home"),
        (Page::Playlist("p".to_owned()), "p", "Playlist"),
        (Page::Album("a".to_owned()), "a", "Album"),
        (Page::Artist("r".to_owned()), "r", "Artist"),
        (Page::Show("s".to_owned()), "s", "Show"),
        (Page::Search("q".to_owned()), "q", "Search"),
    ];
    for (page, uri, kind) in cases {
        assert_eq!(page.uri(), uri);
        assert_eq!(page.kind(), kind);
    }
}

/// A query can be anything the field accepts, including something that
/// spells a URI. Such a page is a search and must never be mistaken for the
/// page that URI names: the payload is read, the kind is decided by the
/// variant.
#[test]
fn a_search_query_that_spells_a_uri_is_still_a_search() {
    let page = Page::Search("spotify:playlist:x".to_owned());

    assert_eq!(page.kind(), "Search");
    assert_eq!(page.uri(), "spotify:playlist:x");
    assert_ne!(page, Page::Playlist("spotify:playlist:x".to_owned()));
}

/// The history is pruned of playlists that left the library, and a search
/// whose query spells a playlist URI must survive that pruning: it is not a
/// page the playlist list can name.
#[test]
fn retain_keeps_a_search_query_that_spells_a_playlist_uri() {
    let mut history = History::default();
    history.go(search("spotify:playlist:x"));

    history.retain(|page| !matches!(page, Page::Playlist(_)));

    assert_eq!(history.current(), search("spotify:playlist:x").as_ref());
}

/// A history that starts on a page has nowhere behind it: the page is where
/// the session opened, not a step taken from the root.
#[test]
fn starting_at_a_page_has_no_back_entry() {
    let history = History::starting_at(Page::Home);

    assert_eq!(history.current(), home().as_ref());
    assert!(!history.can_back());
    assert!(!history.can_forward());
}

#[test]
fn back_from_a_page_opened_on_home_returns_to_home() {
    let mut history = History::starting_at(Page::Home);
    assert!(history.go(playlist("a")));

    assert!(history.back());
    assert_eq!(history.current(), home().as_ref());

    assert!(history.forward());
    assert_eq!(history.current(), playlist("a").as_ref());
}

#[test]
fn retain_keeps_the_home_page() {
    let mut history = History::starting_at(Page::Home);
    history.retain(|page| matches!(page, Page::Home));
    assert_eq!(
        history.current(),
        home().as_ref(),
        "the page a session opened on is not pruned away"
    );

    history.go(playlist("a"));
    history.retain(|page| !matches!(page, Page::Playlist(_)));

    assert!(history.back());
    assert_eq!(history.current(), home().as_ref());
}

#[test]
fn the_same_query_twice_is_one_page() {
    let mut history = History::default();
    history.go(search("never gonna"));

    assert!(!history.go(search("never gonna")));
    assert!(history.go(search("never gonna give")));
}

fn dest_page(page: Page) -> Option<Link> {
    Some(Link::Page(page))
}

fn dest_play(uri: &str) -> Option<Link> {
    Some(Link::Play(uri.to_owned()))
}

/// Every shape a link arrives in: the bare uri, the account-named playlist,
/// and the web address, including the older user path and share suffixes.
#[test]
fn link_accepts_every_listed_form() {
    let id = "4uLU6hMCjMI75M1A2tKUQC";
    let playlist_uri = format!("spotify:playlist:{id}");
    let album_uri = format!("spotify:album:{id}");
    let artist_uri = format!("spotify:artist:{id}");
    let show_uri = format!("spotify:show:{id}");
    let track_uri = format!("spotify:track:{id}");
    let episode_uri = format!("spotify:episode:{id}");

    let cases: Vec<(String, Option<Link>)> = vec![
        // The six kinds as a bare uri.
        (
            playlist_uri.clone(),
            dest_page(Page::Playlist(playlist_uri.clone())),
        ),
        (album_uri.clone(), dest_page(Page::Album(album_uri.clone()))),
        (
            artist_uri.clone(),
            dest_page(Page::Artist(artist_uri.clone())),
        ),
        (show_uri.clone(), dest_page(Page::Show(show_uri.clone()))),
        (track_uri.clone(), dest_play(&track_uri)),
        (episode_uri.clone(), dest_play(&episode_uri)),
        // The older account-named playlist, canonicalised to the plain uri.
        (
            format!("spotify:user:alice:playlist:{id}"),
            dest_page(Page::Playlist(playlist_uri.clone())),
        ),
        // Each kind as a web address.
        (
            format!("https://open.spotify.com/playlist/{id}"),
            dest_page(Page::Playlist(playlist_uri.clone())),
        ),
        (
            format!("https://open.spotify.com/album/{id}"),
            dest_page(Page::Album(album_uri.clone())),
        ),
        (
            format!("https://open.spotify.com/artist/{id}"),
            dest_page(Page::Artist(artist_uri.clone())),
        ),
        (
            format!("https://open.spotify.com/show/{id}"),
            dest_page(Page::Show(show_uri.clone())),
        ),
        (
            format!("https://open.spotify.com/track/{id}"),
            dest_play(&track_uri),
        ),
        (
            format!("https://open.spotify.com/episode/{id}"),
            dest_play(&episode_uri),
        ),
        // The older web playlist path and the shapes it comes in.
        (
            format!("https://open.spotify.com/user/alice/playlist/{id}"),
            dest_page(Page::Playlist(playlist_uri.clone())),
        ),
        (
            format!("https://open.spotify.com/user/alice/playlist/{id}/"),
            dest_page(Page::Playlist(playlist_uri.clone())),
        ),
        (
            format!("https://open.spotify.com/intl-pt/user/alice/playlist/{id}"),
            dest_page(Page::Playlist(playlist_uri.clone())),
        ),
        (
            format!("https://open.spotify.com/user/alice/playlist/{id}?si=x"),
            dest_page(Page::Playlist(playlist_uri.clone())),
        ),
        // Scheme, host and locale variants.
        (
            format!("http://open.spotify.com/track/{id}"),
            dest_play(&track_uri),
        ),
        (
            format!("https://www.open.spotify.com/album/{id}"),
            dest_page(Page::Album(album_uri.clone())),
        ),
        (
            format!("https://open.spotify.com/artist/{id}/"),
            dest_page(Page::Artist(artist_uri.clone())),
        ),
        (
            format!("https://open.spotify.com/intl-pt/track/{id}"),
            dest_play(&track_uri),
        ),
        // Share suffixes are dropped.
        (
            format!("https://open.spotify.com/track/{id}?si=abc"),
            dest_play(&track_uri),
        ),
        (
            format!("https://open.spotify.com/track/{id}#frag"),
            dest_play(&track_uri),
        ),
        (
            format!("https://open.spotify.com/track/{id}?si=a#b"),
            dest_play(&track_uri),
        ),
        // Surrounding whitespace is trimmed; the casing of scheme and host is
        // not the id's affair.
        (
            format!("  https://open.spotify.com/track/{id}\n"),
            dest_play(&track_uri),
        ),
        (
            format!("HTTPS://Open.Spotify.com/track/{id}"),
            dest_play(&track_uri),
        ),
    ];

    for (text, expected) in cases {
        assert_eq!(link(&text), expected, "{text:?}");
    }
}

/// Nothing that is not a link may be treated as one, or a query would become a
/// navigation.
#[test]
fn link_refuses_everything_else() {
    let cases = [
        "",
        "   ",
        "never gonna",
        "track:abc",
        "spotify:track:a b",
        "spotify:local:a:b:c",
        "https://open.spotify.com/local/x",
        "spotify:user:alice",
        "spotify:user:alice:collection",
        "https://open.spotify.com/user/alice",
        "spotify:genre:x",
        "https://open.spotify.com/genre/x",
        "spotify:track:",
        "https://open.spotify.com/track/",
        "https://open.spotify.com/track",
        "spotify:track:a:b",
        "spotify:playlist:abc:extra",
        "spotify:user:alice:playlist:a:b",
        "spotify:track:a?si=x",
        "spotify:track:a/b",
        "https://example.com/track/x",
        "https://open.spotify.com.evil.com/track/x",
        "https://evil.com/open.spotify.com/track/x",
        "https://open.spotify.com:443/track/x",
        "https://open.spotify.com/embed/track/x",
        "track/x/y",
        "intl-pt/track/x/y",
        "intl-pt",
        "https://open.spotify.com/",
        "https://open.spotify.com/user/alice/playlist/",
        "https://open.spotify.com/user/alice/playlist/a/b",
        "ftp://open.spotify.com/track/x",
        "open.spotify.com/track/x",
        // Credentials, empty segments and a doubled locale marker.
        "https://open.spotify.com@evil.com/track/x",
        "https://evil.com@open.spotify.com/track/x",
        "https://open.spotify.com//track/x",
        "https://open.spotify.com/track//x",
        "https://open.spotify.com/intl-pt/intl-de/track/x",
        // An id is base62, so punctuation, non-ASCII and a control character
        // are not an id.
        "spotify:track:ab%cd",
        "spotify:track:ab&cd",
        "spotify:track:ab@cd",
        "spotify:track:naïve",
        "spotify:track:ab\u{0}cd",
    ];
    for text in cases {
        assert_eq!(link(text), None, "{text:?}");
    }

    let id = "4uLU6hMCjMI75M1A2tKUQC";
    let legacy = [
        format!("https://open.spotify.com/user//playlist/{id}"),
        format!("https://open.spotify.com/user/alice/album/{id}"),
        format!("https://open.spotify.com/user/al:ice/playlist/{id}"),
        // A colon in a path segment would splice the path into the
        // colon-delimited uri shape.
        format!("https://open.spotify.com/user:alice:playlist/{id}"),
        format!("https://open.spotify.com/track:x/{id}"),
    ];
    for text in legacy {
        assert_eq!(link(&text), None, "{text:?}");
    }
}

/// A web address this app writes is one it reads back, and a local file is
/// neither.
#[test]
fn every_link_the_app_writes_reads_back() {
    for kind in ["track", "episode", "playlist", "album", "artist", "show"] {
        let uri = format!("spotify:{kind}:4uLU6hMCjMI75M1A2tKUQC");
        let web = crate::verbs::spotify_link(&uri).expect("a link");
        let expected = link(&uri);

        assert!(expected.is_some(), "{uri} names a destination");
        assert_eq!(link(&web), expected, "{web}");
    }

    assert_eq!(crate::verbs::spotify_link("spotify:local:a:b:c"), None);
    assert_eq!(link("spotify:local:a:b:c"), None);
}
