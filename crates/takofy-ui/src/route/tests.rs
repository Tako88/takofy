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
