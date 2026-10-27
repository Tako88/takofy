use super::{ASK_MAX, Ask, KNOWN_MAX, Portrait, Portraits, Slot, Surface};
use crate::message::{Command, Update};
use takofy_model::{ArtistPortrait, CoverSet, CoverUrl};

fn cover(tail: &str) -> CoverSet {
    CoverSet::single(
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url"),
    )
}

fn portrait(artist: &str, cover: Option<CoverSet>) -> ArtistPortrait {
    ArtistPortrait {
        artist: artist.to_owned(),
        portrait: cover,
    }
}

fn reply(query: &str, portraits: Vec<ArtistPortrait>) -> Update {
    Update::Portraits {
        query: query.to_owned(),
        portraits,
    }
}

fn failed(query: &str) -> Update {
    Update::PortraitsFailed {
        query: query.to_owned(),
    }
}

fn shown(artists: &[&str]) -> Vec<String> {
    artists.iter().map(|artist| (*artist).to_owned()).collect()
}

fn refs<'a>(artists: &'a [&'a str]) -> Vec<&'a str> {
    artists.to_vec()
}

fn menu(q: &str) -> Option<(Surface, &str)> {
    Some((Surface::Menu, q))
}

fn page(q: &str) -> Option<(Surface, &str)> {
    Some((Surface::Page, q))
}

fn slot(surface: Surface, key: &str, upto: usize) -> Slot {
    Slot {
        surface,
        key: key.to_owned(),
        upto,
    }
}

#[test]
fn get_reports_unread_absent_and_known() {
    let mut portraits = Portraits::default();
    assert_eq!(portraits.get("a"), Portrait::Unread);
    portraits.apply(
        &reply("q", vec![portrait("a", Some(cover("1")))]),
        menu("q"),
    );
    assert_eq!(
        portraits.get("a"),
        Portrait::Known(&cover("1")),
        "a known portrait hands back its url"
    );
    portraits.apply(&reply("q", vec![portrait("b", None)]), menu("q"));
    assert_eq!(portraits.get("b"), Portrait::Absent);
}

#[test]
fn due_returns_only_unknown_artists_in_order() {
    let mut portraits = Portraits::default();
    portraits.apply(&reply("q", vec![portrait("a", None)]), menu("q"));
    let ask = portraits
        .due(Surface::Page, "q2", &refs(&["b", "a", "c"]))
        .expect("two are unread");
    assert_eq!(ask.artists, shown(&["b", "c"]));
    assert_eq!(ask.slot, slot(Surface::Page, "q2", 3));
}

#[test]
fn nothing_is_due_when_every_shown_artist_is_known() {
    let mut portraits = Portraits::default();
    portraits.apply(
        &reply("q", vec![portrait("a", None), portrait("b", None)]),
        menu("q"),
    );
    assert_eq!(portraits.due(Surface::Menu, "q2", &refs(&["a", "b"])), None);
    assert_eq!(portraits.due(Surface::Menu, "q2", &[]), None);
}

#[test]
fn ask_puts_the_read_in_flight_and_a_second_due_waits() {
    let mut portraits = Portraits::default();
    let ask = portraits
        .due(Surface::Menu, "q", &refs(&["a"]))
        .expect("a is unread");
    assert_eq!(
        portraits.ask(ask),
        Command::LoadPortraits {
            query: "q".to_owned(),
            artists: shown(&["a"]),
        }
    );
    assert_eq!(
        portraits.due(Surface::Menu, "q", &refs(&["b"])),
        None,
        "one read at a time"
    );
    portraits.apply(&failed("q"), menu("q"));
    assert_eq!(portraits.due(Surface::Menu, "q", &refs(&["a"])), None);
}

#[test]
fn a_current_reply_fills_known_including_absences() {
    let mut portraits = Portraits::default();
    let ask = portraits
        .due(Surface::Menu, "q", &refs(&["a", "b"]))
        .expect("both are unread");
    portraits.ask(ask);
    portraits.apply(
        &reply(
            "q",
            vec![portrait("a", Some(cover("1"))), portrait("b", None)],
        ),
        menu("q"),
    );
    assert_eq!(portraits.get("a"), Portrait::Known(&cover("1")));
    assert_eq!(portraits.get("b"), Portrait::Absent);
}

#[test]
fn a_current_reply_records_an_unanswered_request_as_absent() {
    let mut portraits = Portraits::default();
    let ask = portraits
        .due(Surface::Menu, "q", &refs(&["a", "b"]))
        .expect("both are unread");
    portraits.ask(ask);
    portraits.apply(&reply("q", vec![portrait("a", None)]), menu("q"));
    assert_eq!(
        portraits.get("b"),
        Portrait::Absent,
        "an omitted artist falls back to its album cover, not a raised slot"
    );
}

#[test]
fn a_reply_for_another_query_changes_nothing_but_frees_the_wire() {
    let mut portraits = Portraits::default();
    let ask = portraits
        .due(Surface::Menu, "old", &refs(&["a"]))
        .expect("a is unread");
    portraits.ask(ask);
    portraits.apply(
        &reply("old", vec![portrait("a", Some(cover("1")))]),
        menu("new"),
    );
    assert_eq!(portraits.get("a"), Portrait::Unread);
    assert_eq!(
        portraits.due(Surface::Menu, "new", &refs(&["a"])),
        Some(Ask {
            slot: slot(Surface::Menu, "new", 1),
            artists: shown(&["a"]),
        }),
        "the stale reply did not block the new key"
    );
}

#[test]
fn a_failure_falls_back_to_the_album_cover() {
    let mut portraits = Portraits::default();
    let ask = portraits
        .due(Surface::Menu, "q", &refs(&["a"]))
        .expect("a is unread");
    portraits.ask(ask);
    portraits.apply(&failed("q"), menu("q"));
    assert_eq!(
        portraits.get("a"),
        Portrait::Absent,
        "a face that never came is the stand-in, not an empty slot"
    );
    assert_eq!(
        portraits.due(Surface::Menu, "q", &refs(&["a"])),
        None,
        "and the same key does not ask again"
    );
}

#[test]
fn a_key_change_frees_the_ask_slot_for_its_own_artists() {
    let mut portraits = Portraits::default();
    let ask = portraits
        .due(Surface::Menu, "ab", &refs(&["a"]))
        .expect("a is unread");
    portraits.ask(ask);
    portraits.apply(&failed("ab"), menu("ab"));
    assert_eq!(
        portraits.due(Surface::Menu, "abc", &refs(&["b"])),
        Some(Ask {
            slot: slot(Surface::Menu, "abc", 1),
            artists: shown(&["b"]),
        }),
        "another key is not blocked by the failed one"
    );
    assert_eq!(portraits.due(Surface::Menu, "ab", &refs(&["a"])), None);
}

#[test]
fn a_stale_reply_clears_the_ask_slot_when_it_matches() {
    let mut portraits = Portraits::default();
    let ask = portraits
        .due(Surface::Menu, "old", &refs(&["a"]))
        .expect("a is unread");
    portraits.ask(ask);
    portraits.apply(&reply("old", vec![portrait("a", None)]), menu("new"));
    assert_eq!(
        portraits.due(Surface::Menu, "old", &refs(&["a"])),
        Some(Ask {
            slot: slot(Surface::Menu, "old", 1),
            artists: shown(&["a"]),
        }),
        "coming back to the old query asks again"
    );
}

#[test]
fn known_max_evicts_a_face_no_surface_is_showing_and_keeps_the_shown_ones() {
    let mut portraits = Portraits::default();
    let mut batch: Vec<ArtistPortrait> = (0..KNOWN_MAX)
        .map(|n| portrait(&format!("artist{n}"), None))
        .collect();
    batch.push(portrait("menu", None));
    batch.push(portrait("page", None));
    portraits.set_shown(&shown(&["menu", "page"]));
    portraits.apply(&reply("q", batch), menu("q"));

    assert_eq!(
        portraits.get("menu"),
        Portrait::Absent,
        "both windows survive the eviction"
    );
    assert_eq!(portraits.get("page"), Portrait::Absent);
    assert!(portraits.known.len() <= KNOWN_MAX);
    assert!(
        !portraits.known.contains_key("artist0"),
        "the oldest face that was not shown went"
    );
}

#[test]
fn a_menu_ask_under_a_query_does_not_block_the_pages_ask_under_it() {
    let mut portraits = Portraits::default();
    let ask = portraits
        .due(Surface::Menu, "q", &refs(&["a", "b"]))
        .expect("the menu's two");
    portraits.ask(ask);
    portraits.apply(
        &reply("q", vec![portrait("a", None), portrait("b", None)]),
        menu("q"),
    );
    assert_eq!(
        portraits.due(Surface::Menu, "q", &refs(&["a", "b"])),
        None,
        "the menu already has both"
    );
    let ask = portraits
        .due(Surface::Page, "q", &refs(&["a", "b", "c"]))
        .expect("the page's own third artist");
    assert_eq!(
        ask,
        Ask {
            slot: slot(Surface::Page, "q", 3),
            artists: shown(&["c"]),
        }
    );
}

#[test]
fn one_ask_names_at_most_ten_artists_in_window_order() {
    let artists: Vec<String> = (0..25).map(|n| format!("a{n}")).collect();
    let window: Vec<&str> = artists.iter().map(String::as_str).collect();
    let portraits = Portraits::default();
    let ask = portraits
        .due(Surface::Page, "q", &window)
        .expect("all are unread");
    assert_eq!(ask.artists.len(), ASK_MAX);
    assert_eq!(ask.artists, artists[..ASK_MAX]);
    assert_eq!(ask.slot.upto, ASK_MAX);
}

#[test]
fn a_grown_window_is_a_new_slot_and_the_next_ask_takes_the_rest() {
    let artists: Vec<String> = (0..25).map(|n| format!("a{n}")).collect();
    let window: Vec<&str> = artists.iter().map(String::as_str).collect();
    let ten: Vec<&str> = artists[..10].iter().map(String::as_str).collect();
    let mut portraits = Portraits::default();

    let first = portraits
        .due(Surface::Page, "q", &ten)
        .expect("the ten on screen");
    assert_eq!(first.slot, slot(Surface::Page, "q", 10));
    portraits.ask(first);
    portraits.apply(
        &reply(
            "q",
            artists[..10]
                .iter()
                .map(|artist| portrait(artist, Some(cover("face"))))
                .collect(),
        ),
        page("q"),
    );

    let next = portraits
        .due(Surface::Page, "q", &window)
        .expect("the rest of the revealed window");
    assert_eq!(next.slot, slot(Surface::Page, "q", 20));
    assert_eq!(next.artists, artists[10..20]);
}

#[test]
fn the_same_slot_is_not_asked_twice() {
    let mut portraits = Portraits::default();
    let ask = portraits
        .due(Surface::Page, "q", &refs(&["a"]))
        .expect("a is unread");
    portraits.ask(ask);
    // The wire, not the slot, is what normally blocks the second ask; this
    // isolates the slot's own guard.
    portraits.inflight = None;
    assert_eq!(portraits.due(Surface::Page, "q", &refs(&["a"])), None);
    assert!(portraits.due(Surface::Menu, "q", &refs(&["a"])).is_some());
    assert!(
        portraits
            .due(Surface::Page, "q", &refs(&["a", "b"]))
            .is_some()
    );
    assert!(
        portraits
            .due(Surface::Page, "other", &refs(&["a"]))
            .is_some()
    );
}

#[test]
fn a_reply_is_current_only_for_the_surface_that_asked() {
    let mut portraits = Portraits::default();
    let ask = portraits
        .due(Surface::Page, "q", &refs(&["a"]))
        .expect("a is unread");
    portraits.ask(ask);
    portraits.apply(
        &reply("q", vec![portrait("a", Some(cover("1")))]),
        menu("q"),
    );
    assert_eq!(
        portraits.get("a"),
        Portrait::Unread,
        "the menu is on screen, so the page's reply changes nothing"
    );
    assert_eq!(
        portraits.due(Surface::Menu, "q", &refs(&["a"])),
        Some(Ask {
            slot: slot(Surface::Menu, "q", 1),
            artists: shown(&["a"]),
        }),
        "and the wire was freed for the surface on screen"
    );
}

#[test]
fn a_reply_for_the_page_while_the_page_is_on_screen_is_current() {
    let mut portraits = Portraits::default();
    let ask = portraits
        .due(Surface::Page, "q", &refs(&["a"]))
        .expect("a is unread");
    portraits.ask(ask);
    portraits.apply(
        &reply("q", vec![portrait("a", Some(cover("1")))]),
        page("q"),
    );
    assert_eq!(portraits.get("a"), Portrait::Known(&cover("1")));
}

#[test]
fn a_menu_reply_is_current_when_its_page_is_the_surface_on_screen() {
    let mut portraits = Portraits::default();
    let ask = portraits
        .due(Surface::Menu, "q", &refs(&["a", "b"]))
        .expect("the menu's two");
    portraits.ask(ask);
    // Enter opened the page under the same key before the reply landed. The
    // cache is keyed by artist URI, so the menu's faces are the page's.
    portraits.apply(
        &reply(
            "q",
            vec![portrait("a", Some(cover("1"))), portrait("b", None)],
        ),
        page("q"),
    );
    assert_eq!(portraits.get("a"), Portrait::Known(&cover("1")));
    assert_eq!(portraits.get("b"), Portrait::Absent);
    assert_eq!(
        portraits.due(Surface::Page, "q", &refs(&["a", "b"])),
        None,
        "the page draws the menu's two, so it asks nothing again for them"
    );
}

#[test]
fn note_clears_the_slot_only_for_another_surface_or_key() {
    let mut portraits = Portraits::default();
    let ask = portraits
        .due(Surface::Page, "q", &refs(&["a"]))
        .expect("a is unread");
    portraits.ask(ask);
    portraits.inflight = None;

    portraits.note(page("q"));
    assert_eq!(
        portraits.due(Surface::Page, "q", &refs(&["a"])),
        None,
        "kept"
    );
    portraits.note(menu("q"));
    assert!(
        portraits.due(Surface::Page, "q", &refs(&["a"])).is_some(),
        "the other surface freed it"
    );
}

#[test]
fn note_none_clears_the_slot_and_an_empty_slot_is_never_cleared() {
    let mut portraits = Portraits::default();
    assert_eq!(portraits.asked, None);
    portraits.note(None);
    assert_eq!(portraits.asked, None);

    let ask = portraits
        .due(Surface::Menu, "q", &refs(&["a"]))
        .expect("a is unread");
    portraits.ask(ask);
    portraits.inflight = None;
    portraits.note(None);
    assert_eq!(
        portraits.asked, None,
        "nothing on screen frees the slot for when it returns"
    );
    assert_eq!(portraits.asked_artists, Vec::<String>::new());
}

#[test]
fn an_in_flight_ask_keeps_the_slot_through_a_note() {
    let mut portraits = Portraits::default();
    let ask = portraits
        .due(Surface::Page, "q", &refs(&["a"]))
        .expect("a is unread");
    portraits.ask(ask);
    portraits.note(menu("q"));
    assert_eq!(portraits.asked, Some(slot(Surface::Page, "q", 1)));
}
