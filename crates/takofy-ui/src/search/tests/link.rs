use super::super::*;
use super::test_support::*;
use crate::route::{Link, Page};

/// A Spotify id, so the tests read as the address they are about.
const ID: &str = "4uLU6hMCjMI75M1A2tKUQC";

fn track_link() -> String {
    format!("spotify:track:{ID}")
}

#[test]
fn typing_a_link_opens_no_menu_and_asks_no_read() {
    let mut state = typed(&track_link());
    assert!(
        !state.is_open(),
        "a link is not a query to open the menu on"
    );
    assert_eq!(
        state.key(),
        None,
        "a link is not the key either read is about"
    );
    assert_eq!(state.request(None), None, "a link asks no read");
    assert!(!state.wants(None), "and wants none");
}

#[test]
fn a_link_over_an_open_menu_closes_it_and_drops_the_answer() {
    let mut state = typed("abc");
    state.apply(&reply("abc", &["a"]));
    assert!(state.is_open());
    assert!(state.listing().is_some());

    type_text(&mut state, &track_link());
    assert!(!state.is_open());
    assert_eq!(state.listing(), None, "the menu keeps no answer for a link");
}

#[test]
fn arrows_on_a_link_open_nothing() {
    let mut state = typed(&track_link());
    assert_eq!(state.step(key("down")), Act::None);
    assert!(!state.is_open(), "Down re-opens only a query");
    assert_eq!(state.step(key("up")), Act::None);
    assert!(!state.is_open(), "Up re-opens only a query");
}

#[test]
fn a_refused_link_is_a_query() {
    for text in ["spotify:local:a:b:c", "https://example.com/track/x"] {
        let mut state = typed(text);
        assert!(state.is_open(), "{text:?} is a query, so the menu opens");
        assert_eq!(state.key(), Some(text.to_owned()));
        assert_eq!(state.request(None), Some(search(text)));
    }
}

#[test]
fn appending_to_a_link_makes_it_a_query_again() {
    let mut state = typed(&track_link());
    assert!(!state.is_open());

    let longer = format!("{} more", track_link());
    type_text(&mut state, &longer);
    assert!(state.is_open(), "text past the link is a query again");
    assert_eq!(state.request(None), Some(search(&longer)));
}

#[test]
fn a_paste_of_a_link_acts_at_once() {
    let mut state = Search::default();
    assert_eq!(paste_text(&mut state, &track_link()), Act::OpenLink);
    assert!(!state.is_open(), "the menu never opens on a link");
}

#[test]
fn a_paste_of_plain_text_is_only_typing() {
    let mut state = Search::default();
    assert_eq!(paste_text(&mut state, "never gonna"), Act::None);
    assert!(state.is_open(), "pasted text is a query like any other");
    assert_eq!(
        state.request(None),
        Some(search("never gonna")),
        "the paste armed the read"
    );
}

#[test]
fn a_paste_without_the_keyboard_does_nothing() {
    let mut state = Search::default();
    let act = state.step(KeyInput {
        typed: true,
        pasted: true,
        focused: false,
        ..KeyInput::default()
    });
    assert_eq!(act, Act::None, "another field's paste is not this one's");
    assert!(!state.is_open());
}

#[test]
fn enter_on_a_typed_link_acts() {
    let mut state = typed(&track_link());
    assert_eq!(state.step(key("enter")), Act::OpenLink);
}

#[test]
fn enter_on_a_query_still_opens_the_results() {
    let mut state = typed("never gonna");
    assert_eq!(state.step(key("enter")), Act::OpenResults);
}

#[test]
fn take_link_clears_the_field_and_closes_the_menu() {
    let mut state = typed("abc");
    type_text(&mut state, &format!("spotify:album:{ID}"));

    assert_eq!(
        state.take_link(),
        Some(Link::Page(Page::Album(format!("spotify:album:{ID}"))))
    );
    assert_eq!(state.text, "");
    assert_eq!(state.key(), None);
    assert!(!state.is_open());
    assert_eq!(state.take_link(), None, "the field holds one link, once");
    assert_eq!(
        state.request(None),
        None,
        "an emptied field asks for no read"
    );
}

#[test]
fn take_link_leaves_a_query_alone() {
    let mut state = typed("never gonna");
    assert_eq!(state.take_link(), None);
    assert_eq!(
        state.text, "never gonna",
        "a query is not the shell's to take"
    );
}

#[test]
fn wants_agrees_with_request_for_a_link() {
    let mut state = typed(&format!("spotify:show:{ID}"));
    assert!(!state.wants(None));
    assert_eq!(state.request(None), None);
}

#[test]
fn a_legacy_playlist_link_is_a_link_not_a_query() {
    let mut state = typed(&format!(
        "https://open.spotify.com/user/alice/playlist/{ID}"
    ));
    assert!(!state.is_open());
    assert_eq!(
        state.take_link(),
        Some(Link::Page(Page::Playlist(format!("spotify:playlist:{ID}"))))
    );
}
