use super::super::*;
use super::test_support::*;
use takofy_model::{ArtistPortrait, CoverUrl};

fn search_request(library: &mut Library) -> Option<Command> {
    let page = library.page_search_query().map(str::to_owned);
    library.search.request(page.as_deref())
}

/// Types `text` into the field, as one frame of the real path would.
fn type_search(library: &mut Library, text: &str) {
    library.search.text = text.to_owned();
    let input = crate::search::KeyInput {
        typed: true,
        focused: true,
        ..crate::search::KeyInput::default()
    };
    let _ = library.search.step(input);
}

fn search(query: &str) -> Page {
    Page::Search(query.to_owned())
}

fn search_reply(query: &str, tracks: Vec<TrackEntry>) -> Update {
    Update::Search {
        query: query.to_owned(),
        tracks,
    }
}

#[test]
fn a_search_page_asks_for_its_answer_once_it_is_open() {
    let mut library = Library::default();
    assert_eq!(library.open(search("never gonna")), None);

    assert_eq!(
        search_request(&mut library),
        Some(Command::Search {
            query: "never gonna".to_owned()
        })
    );
    assert_eq!(library.page_search_query(), Some("never gonna"));
}

#[test]
fn back_onto_a_held_search_page_does_not_ask_again() {
    let mut library = Library::default();
    library.open(search("never gonna"));
    library.apply(&search_reply("never gonna", vec![track("t")]));
    assert_eq!(
        library.open(album("spotify:album:a")),
        Some(Command::LoadAlbum {
            album: "spotify:album:a".to_owned()
        })
    );

    assert_eq!(library.back(), None, "the held answer is already settled");
    assert_eq!(library.page_search_query(), Some("never gonna"));
    assert_eq!(
        search_request(&mut library),
        None,
        "the page keeps the answer, so Back asks for nothing"
    );
}

#[test]
fn back_onto_a_dropped_search_page_asks_again() {
    let mut library = Library::default();
    library.open(search("never gonna"));
    library.apply(&search_reply("never gonna", vec![track("t")]));
    library.open(album("spotify:album:a"));
    // A second query replaces the first, so the page no longer holds it.
    library.open(search("give you up"));

    // Back lands on the album; the second Back lands on the dropped search.
    assert_eq!(library.back(), None);
    assert_eq!(library.page(), Some(&album("spotify:album:a")));
    assert_eq!(library.back(), None);
    assert_eq!(library.page_search_query(), Some("never gonna"));
    assert_eq!(
        search_request(&mut library),
        Some(Command::Search {
            query: "never gonna".to_owned()
        }),
        "the dropped answer is asked for again"
    );
}

/// A failed page has nothing to draw, so Back onto it must ask again — the
/// same request the page's Retry makes.
#[test]
fn back_onto_a_failed_search_page_asks_again() {
    let mut library = Library::default();
    library.open(search("never gonna"));
    library.apply(&Update::SearchFailed {
        query: "never gonna".to_owned(),
        failure: Failure::Search,
    });
    library.open(album("spotify:album:a"));

    assert_eq!(library.back(), None, "a search page never settles");
    assert_eq!(library.page_search_query(), Some("never gonna"));
    assert_eq!(
        search_request(&mut library),
        Some(Command::Search {
            query: "never gonna".to_owned()
        }),
        "a failed page is asked for again"
    );
    assert_eq!(
        library.search.page_answer("never gonna"),
        Some(&Load::Loading),
        "and the failure is no longer what the page draws"
    );
}

#[test]
fn back_forward_between_search_pages_keeps_each_answer() {
    let mut library = Library::default();
    library.open(search("a"));
    library.apply(&search_reply("a", vec![track("ta")]));
    library.open(search("b"));
    library.apply(&search_reply("b", vec![track("tb")]));

    assert_eq!(library.back(), None);
    assert_eq!(
        library.search.page_answer("b"),
        None,
        "b's answer must not be drawn under a's heading"
    );
    assert_eq!(
        search_request(&mut library),
        Some(Command::Search {
            query: "a".to_owned()
        })
    );

    library.apply(&search_reply("a", vec![track("ta")]));
    assert_eq!(library.back(), None);
    assert_eq!(
        search_request(&mut library),
        None,
        "the root asks for nothing"
    );
    assert_eq!(library.forward(), None);
    assert_eq!(
        search_request(&mut library),
        None,
        "a's answer is still held"
    );
    assert_eq!(
        library.search.page_answer("a"),
        Some(&Load::Ready(vec![track("ta")]))
    );
}

#[test]
fn a_playlist_reply_keeps_the_search_pages_in_the_history() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist(
        "spotify:playlist:a",
        "Mix",
    )]));
    library.select("spotify:playlist:a");
    library.open(search("never gonna"));

    // The playlist leaves the library: its page is pruned, the search is not.
    library.apply(&Update::Playlists(Vec::new()));

    assert_eq!(library.page_search_query(), Some("never gonna"));
    assert!(library.can_back(), "the pruned playlist was behind it");
    library.back();
    assert_eq!(library.page(), None, "the pruned playlist is skipped");
}

#[test]
fn a_search_query_that_spells_a_playlist_uri_is_not_that_playlist() {
    let query = "spotify:playlist:x";
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist(query, "Mix")]));
    library.open(search(query));

    assert_eq!(library.page_search_query(), Some(query));
    assert_eq!(
        library.selected(),
        None,
        "the playlist highlight must not follow a search's text"
    );
    assert_eq!(library.wanted, None);
    assert_eq!(library.tracks, Load::Idle);

    // And a playlist reply cannot prune it: the variant decides, not the
    // text.
    library.apply(&Update::Playlists(Vec::new()));
    assert_eq!(library.page_search_query(), Some(query));
}

/// Retry is the page's own control, so it asks for the page's query even
/// while the menu is open and idle on another key: `request` would take the
/// menu's read and leave the page `Idle` until a later frame.
#[test]
fn reload_search_asks_for_the_page_while_the_menu_is_open_on_another_key() {
    let mut library = Library::default();
    library.open(search("never gonna"));
    library.apply(&Update::SearchFailed {
        query: "never gonna".to_owned(),
        failure: Failure::Search,
    });
    type_search(&mut library, "give you up");
    assert!(
        library.search.is_open(),
        "the menu is open on the typed key"
    );
    assert_eq!(
        library.search.menu_view(),
        crate::search::MenuView::Nothing,
        "and has nothing to draw: its read is idle"
    );

    assert_eq!(
        library.reload_search(),
        Some(Command::Search {
            query: "never gonna".to_owned()
        }),
        "the retry asks for the page, not for the menu's key"
    );
    assert_eq!(
        library.search.page_answer("never gonna"),
        Some(&Load::Loading)
    );
}

#[test]
fn reload_search_sets_the_page_idle_and_asks_once() {
    let mut library = Library::default();
    library.open(search("never gonna"));
    library.apply(&Update::SearchFailed {
        query: "never gonna".to_owned(),
        failure: Failure::Search,
    });
    assert_eq!(
        library.search.page_answer("never gonna"),
        Some(&Load::Failed(Failure::Search))
    );

    assert_eq!(
        library.reload_search(),
        Some(Command::Search {
            query: "never gonna".to_owned()
        }),
        "the page's Retry is this call, so it asks in the same frame"
    );
    assert_eq!(
        library.search.page_answer("never gonna"),
        Some(&Load::Loading)
    );
    assert_eq!(library.reload_search(), None, "one read on the wire");

    // Retry is only the search page's: nowhere else has a page to reload.
    let mut library = Library::default();
    assert_eq!(library.reload_search(), None);
}

#[test]
fn a_portrait_reply_does_not_disturb_the_search_reads() {
    let mut library = Library::default();
    library.open(search("never gonna"));
    library.apply(&search_reply("never gonna", vec![track("t")]));
    let menu = format!("{:?}", library.search.menu_view());
    let page = library.search.page_answer("never gonna").cloned();

    library.apply(&Update::Portraits {
        query: "never gonna".to_owned(),
        portraits: vec![ArtistPortrait {
            artist: "spotify:artist:a".to_owned(),
            portrait: Some(CoverSet::single(
                CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url"),
            )),
        }],
    });
    library.apply(&Update::PortraitsFailed {
        query: "never gonna".to_owned(),
    });

    assert_eq!(
        format!("{:?}", library.search.menu_view()),
        menu,
        "a portrait reply is not a page or a menu answer"
    );
    assert_eq!(library.search.page_answer("never gonna"), page.as_ref());
    assert_eq!(library.page_search_query(), Some("never gonna"));
}
