use super::super::*;
use super::test_support::*;

fn portrait_command(query: &str, artists: &[&str]) -> Command {
    Command::LoadPortraits {
        query: query.to_owned(),
        artists: artists.iter().map(|artist| (*artist).to_owned()).collect(),
    }
}

fn portrait(artist: &str, cover: Option<&str>) -> Update {
    Update::Portraits {
        query: "abc".to_owned(),
        portraits: vec![takofy_model::ArtistPortrait {
            artist: artist.to_owned(),
            portrait: cover.map(|tail| {
                takofy_model::CoverSet::single(
                    takofy_model::CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}"))
                        .expect("a cover url"),
                )
            }),
        }],
    }
}

fn artist(n: usize) -> String {
    format!("spotify:artist:r{n}")
}

fn uris(range: std::ops::Range<usize>) -> Vec<String> {
    range.map(artist).collect()
}

#[test]
fn no_portraits_are_asked_for_while_the_menu_has_nothing_to_draw() {
    // Loading, not open, blank, and answering another key.
    let mut loading = typed("abc");
    assert_eq!(loading.request(None), Some(search("abc")));
    assert_eq!(loading.request_portraits(None), None);

    let mut closed = banded();
    closed.close();
    assert_eq!(closed.request_portraits(None), None);

    let mut blank = banded();
    blank.text.clear();
    blank.text_changed();
    assert_eq!(blank.request_portraits(None), None);

    let mut moved = banded();
    type_text(&mut moved, "abcd");
    assert_eq!(moved.request_portraits(None), None);
}

#[test]
fn the_menu_asks_for_its_two_artists_in_band_order() {
    let mut state = banded();
    assert_eq!(
        state.request_portraits(None),
        Some(portrait_command(
            "abc",
            &["spotify:artist:r0", "spotify:artist:r1"]
        ))
    );
    assert_eq!(state.request_portraits(None), None, "one read at a time");
}

#[test]
fn the_closed_menu_leaves_the_page_asking_for_its_ten() {
    let mut state = paged(25);
    assert!(!state.is_open());
    let expected: Vec<String> = uris(0..10);
    let expected: Vec<&str> = expected.iter().map(String::as_str).collect();
    assert_eq!(
        state.request_portraits(Some("abc")),
        Some(portrait_command("abc", &expected))
    );
}

#[test]
fn an_open_menu_with_nothing_due_never_falls_through_to_the_page() {
    let mut state = banded();
    assert!(state.request_portraits(None).is_some());
    state.apply(&portrait("spotify:artist:r0", Some("0")));
    state.apply(&portrait("spotify:artist:r1", Some("1")));
    assert_eq!(
        state.request_portraits(Some("abc")),
        None,
        "the menu's two are answered, so no read is due"
    );
}

#[test]
fn a_loading_failed_or_empty_page_asks_for_no_face() {
    let mut loading = typed("abc");
    loading.open_results("page");
    assert_eq!(loading.page_view("page"), Some(PageView::Waiting));
    assert_eq!(loading.request_portraits(Some("page")), None);

    let mut failed = typed("abc");
    failed.open_results("page");
    failed.apply(&Update::SearchFailed {
        query: "page".to_owned(),
        failure: Failure::Search,
    });
    assert_eq!(
        failed.page_view("page"),
        Some(PageView::Failed(Failure::Search))
    );
    assert_eq!(failed.request_portraits(Some("page")), None);

    let mut empty = typed("abc");
    empty.open_results("page");
    empty.apply(&reply("page", &[]));
    assert_eq!(empty.page_view("page"), Some(PageView::Empty));
    assert_eq!(empty.request_portraits(Some("page")), None);
}

#[test]
fn a_newer_query_never_asks_for_the_previous_answers_artists() {
    let mut state = banded();
    assert!(state.request_portraits(None).is_some());
    state.apply(&portrait("spotify:artist:r0", Some("old")));
    type_text(&mut state, "abcd");
    state.apply(&Update::Search {
        query: "abcd".to_owned(),
        tracks: (0..8)
            .map(|index| {
                rich(
                    &format!("u{index}"),
                    &format!("b{index}"),
                    &format!("s{index}"),
                )
            })
            .collect(),
    });
    // The new answer's own first two artists, never the held answer's.
    assert_eq!(
        state.request_portraits(None),
        Some(portrait_command(
            "abcd",
            &["spotify:artist:s0", "spotify:artist:s1"]
        ))
    );
}

#[test]
fn a_newer_page_query_never_asks_for_the_previous_answers_artists() {
    let mut state = paged(25);
    assert!(state.request_portraits(Some("abc")).is_some());
    state.apply(&Update::Search {
        query: "abc".to_owned(),
        tracks: Vec::new(),
    });
    // A page for another query is the one on screen; the old answer's faces
    // are not asked for.
    state.open_results("xy");
    assert_eq!(state.request_portraits(Some("xy")), None);
}

#[test]
fn wants_is_true_exactly_when_request_portraits_would_send() {
    let mut state = banded();
    assert!(state.wants(None));
    assert!(state.request_portraits(None).is_some());
    assert!(!state.wants(None));
    assert_eq!(state.request_portraits(None), None);

    let mut page = paged(25);
    assert!(page.wants(Some("abc")));
    assert!(page.request_portraits(Some("abc")).is_some());
    assert!(!page.wants(Some("abc")));
    assert_eq!(page.request_portraits(Some("abc")), None);
}

#[test]
fn an_artist_met_in_a_previous_query_is_known_and_asks_nothing() {
    let mut state = banded();
    assert!(state.request_portraits(None).is_some());
    state.apply(&portrait("spotify:artist:r0", Some("0")));
    state.apply(&portrait("spotify:artist:r1", Some("1")));
    type_text(&mut state, "abcd");
    state.apply(&rich_reply("abcd", 8));
    assert_eq!(
        state.portrait("spotify:artist:r0"),
        crate::portraits::Portrait::Known(&takofy_model::CoverSet::single(
            takofy_model::CoverUrl::from_wire("https://i.scdn.co/image/0").expect("a cover url")
        ))
    );
    assert_eq!(
        state.request_portraits(None),
        None,
        "both rows the new answer shows are already held"
    );
}

#[test]
fn a_portrait_failure_settles_the_rows_on_the_stand_in() {
    let mut state = banded();
    assert!(state.request_portraits(None).is_some());
    state.apply(&Update::PortraitsFailed {
        query: "abc".to_owned(),
    });
    assert_eq!(state.request_portraits(None), None, "a failed key is kept");
    for artist in ["spotify:artist:r0", "spotify:artist:r1"] {
        assert_eq!(state.portrait(artist), Portrait::Absent);
    }
}

#[test]
fn a_page_ask_is_not_current_after_the_page_is_left() {
    let mut state = paged(25);
    assert!(state.request_portraits(Some("abc")).is_some());
    // The reader navigates to a playlist: no page is open this frame, and
    // the reply lands after it is gone.
    assert_eq!(state.request_portraits(None), None);
    state.apply(&Update::PortraitsFailed {
        query: "abc".to_owned(),
    });
    assert_eq!(
        state.portrait("spotify:artist:r0"),
        Portrait::Unread,
        "a failure that lands after the page is left must not mark its artists absent"
    );
}

#[test]
fn a_menu_reply_that_lands_on_its_own_page_is_current() {
    let mut state = banded();
    assert_eq!(
        state.request_portraits(None),
        Some(portrait_command(
            "abc",
            &["spotify:artist:r0", "spotify:artist:r1"]
        ))
    );
    // Enter opens the page under the same key before the menu's reply
    // lands; the page's own ask waits for the read already on the wire, but
    // the frame that opens it is what makes the page the surface on screen.
    state.open_results("abc");
    state.close();
    assert_eq!(state.request_portraits(Some("abc")), None);
    state.apply(&Update::Portraits {
        query: "abc".to_owned(),
        portraits: vec![
            takofy_model::ArtistPortrait {
                artist: "spotify:artist:r0".to_owned(),
                portrait: Some(takofy_model::CoverSet::single(
                    takofy_model::CoverUrl::from_wire("https://i.scdn.co/image/0")
                        .expect("a cover url"),
                )),
            },
            takofy_model::ArtistPortrait {
                artist: "spotify:artist:r1".to_owned(),
                portrait: Some(takofy_model::CoverSet::single(
                    takofy_model::CoverUrl::from_wire("https://i.scdn.co/image/1")
                        .expect("a cover url"),
                )),
            },
        ],
    });
    assert_eq!(
        state.portrait("spotify:artist:r0"),
        Portrait::Known(&takofy_model::CoverSet::single(
            takofy_model::CoverUrl::from_wire("https://i.scdn.co/image/0").expect("a cover url")
        )),
        "the menu's faces are the faces the page draws"
    );
    let expected: Vec<String> = uris(2..8);
    let expected: Vec<&str> = expected.iter().map(String::as_str).collect();
    assert_eq!(
        state.request_portraits(Some("abc")),
        Some(portrait_command("abc", &expected)),
        "the page asks only for the artists the menu did not answer"
    );
}

#[test]
fn the_menu_closing_hands_the_page_a_distinct_slot_in_the_next_frame() {
    let mut state = banded();
    state.open_results("abc");
    assert_eq!(
        state.request_portraits(Some("abc")),
        Some(portrait_command(
            "abc",
            &["spotify:artist:r0", "spotify:artist:r1"]
        )),
        "the menu wins the slot while it is open"
    );
    state.apply(&portrait("spotify:artist:r0", Some("0")));
    state.apply(&portrait("spotify:artist:r1", Some("1")));
    state.close();
    let expected: Vec<String> = uris(2..8);
    let expected: Vec<&str> = expected.iter().map(String::as_str).collect();
    assert_eq!(
        state.request_portraits(Some("abc")),
        Some(portrait_command("abc", &expected)),
        "the page asks under the same query for the rest"
    );
}

#[test]
fn taking_the_artists_reveal_widens_the_page_window() {
    let mut state = paged(25);
    assert!(state.request_portraits(Some("abc")).is_some());
    state.apply(&Update::Portraits {
        query: "abc".to_owned(),
        portraits: uris(0..10)
            .into_iter()
            .map(|artist| takofy_model::ArtistPortrait {
                artist,
                portrait: None,
            })
            .collect(),
    });
    state.take_more("abc", Kind::Artists);
    let expected: Vec<String> = uris(10..20);
    let expected: Vec<&str> = expected.iter().map(String::as_str).collect();
    assert_eq!(
        state.request_portraits(Some("abc")),
        Some(portrait_command("abc", &expected)),
        "the next ten unknowns, in window order"
    );
}
