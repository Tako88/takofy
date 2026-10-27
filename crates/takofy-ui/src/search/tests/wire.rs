use super::super::*;
use super::test_support::*;

#[test]
fn collapse_trims_and_joins_runs_of_whitespace() {
    assert_eq!(
        collapse("  never   gonna  "),
        Some("never gonna".to_owned())
    );
    assert_eq!(collapse("never\tgonna"), Some("never gonna".to_owned()));
    assert_eq!(collapse(" a\nb "), Some("a b".to_owned()));
    assert_eq!(collapse("a"), Some("a".to_owned()));
    assert_eq!(collapse(""), None);
    assert_eq!(collapse("   "), None);
}

#[test]
fn a_burst_of_keystrokes_sends_one_command() {
    let mut state = Search::default();
    type_text(&mut state, "a");
    assert_eq!(state.request(None), Some(search("a")));
    type_text(&mut state, "ab");
    type_text(&mut state, "abc");
    assert_eq!(
        state.request(None),
        None,
        "one read is on the wire at a time"
    );

    // The answer for the superseded "a" lands: it fills nothing, and the
    // next request is for the query the user has actually typed.
    state.apply(&reply("a", &["spotify:track:a"]));
    assert_eq!(
        state.menu_view(),
        MenuView::Nothing,
        "a stale answer is never drawn"
    );
    assert_eq!(state.request(None), Some(search("abc")));

    // An "ab" answer cannot fill "abc": the key is part of the guard.
    state.apply(&reply("ab", &["spotify:track:ab"]));
    assert_eq!(state.menu_view(), MenuView::Nothing);
    state.apply(&reply("abc", &["spotify:track:abc"]));
    let answer = tracks(&["spotify:track:abc"]);
    let listing = state.listing().expect("the current answer has a shape");
    assert_eq!(listing.answer, answer.as_slice());
    assert_eq!(listing.shape, Index::derive(&answer).shape(MENU));
}

#[test]
fn a_reply_fills_both_reads_when_the_keys_match() {
    let mut state = typed("abc");
    state.open_results("abc");
    // The coalescing path: both reads are waiting on the same key.
    state.menu = Read {
        key: Some("abc".to_owned()),
        load: Load::Loading,
        index: None,
    };
    state.page = Read {
        key: Some("abc".to_owned()),
        load: Load::Loading,
        index: None,
    };
    state.inflight = Some("abc".to_owned());

    state.apply(&reply("abc", &["spotify:track:a"]));

    assert_eq!(
        state.menu.load,
        Load::Ready(tracks(&["spotify:track:a"])),
        "the menu takes the answer"
    );
    assert_eq!(
        state.page.load,
        Load::Ready(tracks(&["spotify:track:a"])),
        "and so does the page, from the one read"
    );
    assert_eq!(state.inflight, None, "the echo clears the wire marker");
}

#[test]
fn typing_away_and_back_fills_the_reply_that_was_still_on_the_wire() {
    let mut state = typed("abc");
    assert_eq!(state.request(None), Some(search("abc")));
    // On to another key and back: `want` puts the menu back to `Idle` on
    // "abc", and the "abc" read is still the one on the wire.
    type_text(&mut state, "abcd");
    type_text(&mut state, "abc");
    assert_eq!(state.menu.load, Load::Idle);
    assert_eq!(state.inflight.as_deref(), Some("abc"));

    state.apply(&reply("abc", &["spotify:track:a"]));

    let answer = tracks(&["spotify:track:a"]);
    let listing = state.listing().expect("the current answer has a shape");
    assert_eq!(
        listing.answer,
        answer.as_slice(),
        "the reply fills the idle read on its own key"
    );
    assert_eq!(state.inflight, None, "the reply was used, not orphaned");
    assert_eq!(state.request(None), None, "and is not asked for again");
}

#[test]
fn enter_while_the_menu_is_loading_still_fills_the_page() {
    let mut state = typed("abc");
    assert_eq!(state.request(None), Some(search("abc")));
    assert_eq!(state.step(key("enter")), Act::OpenResults);
    state.open_results("abc");
    assert_eq!(
        state.page_answer("abc"),
        Some(&Load::Idle),
        "a loading menu seeds nothing"
    );

    state.apply(&reply("abc", &["spotify:track:a"]));

    assert_eq!(
        state.page_answer("abc"),
        Some(&Load::Ready(tracks(&["spotify:track:a"]))),
        "the one read answers the page the menu had not answered yet"
    );
    assert_eq!(
        state.request(Some("abc")),
        None,
        "so the query is not fetched a second time"
    );
}

#[test]
fn a_failure_clears_the_wire_marker_for_its_key() {
    let mut state = typed("abc");
    assert_eq!(state.request(None), Some(search("abc")));
    assert_eq!(state.inflight.as_deref(), Some("abc"));

    state.apply(&failed("abc"));

    assert_eq!(state.inflight, None, "the failed echo clears the wire");
    assert_eq!(state.menu_view(), MenuView::Failed(Failure::Search));
}

#[test]
fn a_failure_for_a_superseded_key_leaves_the_newer_read_alone() {
    let mut state = typed("abcd");
    // The wire is already working on the key the user is on now.
    state.menu = Read {
        key: Some("abcd".to_owned()),
        load: Load::Loading,
        index: None,
    };
    state.inflight = Some("abcd".to_owned());

    state.apply(&failed("abc"));

    assert_eq!(
        state.menu,
        Read {
            key: Some("abcd".to_owned()),
            load: Load::Loading,
            index: None,
        },
        "a stale failure is not drawn and does not fail the new key"
    );
    assert_eq!(
        state.inflight.as_deref(),
        Some("abcd"),
        "the newer read is still on the wire"
    );
}

#[test]
fn wants_agrees_with_request_over_the_four_load_pairs() {
    // (menu load, page load, the page holds the menu's key, a read goes out)
    let cases = [
        (Load::Idle, Load::Idle, true, true),
        (Load::Idle, Load::Idle, false, true),
        (Load::Idle, Load::Loading, true, false),
        (Load::Idle, Load::Loading, false, true),
        (Load::Loading, Load::Idle, true, false),
        (Load::Loading, Load::Idle, false, false),
        (Load::Loading, Load::Loading, true, false),
        (Load::Loading, Load::Loading, false, false),
    ];
    for (menu, page, same_key, due) in cases {
        let mut state = typed("abc");
        state.menu = Read {
            key: Some("abc".to_owned()),
            load: menu.clone(),
            index: None,
        };
        state.page = Read {
            key: Some(if same_key { "abc" } else { "xyz" }.to_owned()),
            load: page.clone(),
            index: None,
        };

        let case = format!("menu {menu:?}, page {page:?}, same key: {same_key}");
        assert_eq!(state.wants(Some("abc")), due, "wants: {case}");
        assert_eq!(
            state.request(Some("abc")),
            due.then(|| search("abc")),
            "request: {case}"
        );
    }
}

#[test]
fn a_sibling_read_on_the_same_key_waits_instead_of_asking_again() {
    // Rule 3: the other read is loading the same key, so this one joins it
    // and nothing goes on the wire.
    let mut state = typed("abc");
    state.page = Read {
        key: Some("abc".to_owned()),
        load: Load::Loading,
        index: None,
    };

    assert_eq!(state.request(None), None);
    assert_eq!(state.menu.load, Load::Loading, "the menu waits with it");
    assert_eq!(state.inflight, None, "nothing was sent");
}

/// A page control asks for the page's query even while the menu is open and
/// due: `request` would take the menu's read first, leaving the page idle.
#[test]
fn request_page_asks_for_the_page_even_when_the_menu_is_due() {
    let mut state = typed("abc");
    state.open_results("page");
    assert!(state.is_open(), "the menu is open on the typed key");

    assert_eq!(state.request_page(Some("page")), Some(search("page")));
    assert_eq!(state.page.load, Load::Loading);
    assert_eq!(state.menu.load, Load::Idle, "the menu was not the one sent");
    assert_eq!(state.request_page(Some("page")), None, "one read at a time");
}

/// The page-only path keeps the sibling rule: a menu already loading the
/// page's key is waited on, not asked twice.
#[test]
fn request_page_waits_for_a_sibling_loading_its_key() {
    let mut state = typed("abc");
    state.open_results("abc");
    state.menu = Read {
        key: Some("abc".to_owned()),
        load: Load::Loading,
        index: None,
    };

    assert_eq!(state.request_page(Some("abc")), None);
    assert_eq!(state.page.load, Load::Loading, "the page waits with it");
    assert_eq!(state.inflight, None, "nothing was sent");
}

#[test]
fn a_failure_is_not_retried_until_the_key_changes() {
    let mut state = typed("abc");
    assert_eq!(state.request(None), Some(search("abc")));
    state.apply(&failed("abc"));
    assert_eq!(state.menu_view(), MenuView::Failed(Failure::Search));

    assert_eq!(state.request(None), None, "a failed key is not re-asked");
    type_text(&mut state, "abcd");
    assert_eq!(
        state.request(None),
        Some(search("abcd")),
        "typing is the retry"
    );
}

#[test]
fn empty_text_and_focus_alone_send_nothing() {
    let mut state = Search::default();
    let focused = KeyInput {
        focused: true,
        ..KeyInput::default()
    };
    assert_eq!(state.step(focused), Act::None);
    assert_eq!(state.request(None), None);
    assert!(!state.is_open(), "focus alone opens nothing");

    // Spaces are not a query either.
    type_text(&mut state, "   ");
    assert_eq!(state.request(None), None);
    assert!(!state.is_open());
}

#[test]
fn a_ready_menu_seeds_the_page_without_a_command() {
    let mut state = typed("abc");
    assert_eq!(state.request(None), Some(search("abc")));
    state.apply(&reply("abc", &["spotify:track:a", "spotify:track:b"]));

    state.open_results("abc");
    assert_eq!(
        state.page_answer("abc"),
        Some(&Load::Ready(tracks(&[
            "spotify:track:a",
            "spotify:track:b"
        ])))
    );
    assert_eq!(
        state.request(Some("abc")),
        None,
        "seeded, so no second read"
    );
}

#[test]
fn enter_after_a_failed_menu_asks_exactly_once() {
    let mut state = typed("abc");
    assert_eq!(state.request(None), Some(search("abc")));
    state.apply(&failed("abc"));

    state.open_results("abc");
    assert_eq!(state.page_answer("abc"), Some(&Load::Idle), "not seeded");
    assert_eq!(state.request(Some("abc")), Some(search("abc")));
    assert_eq!(
        state.request(Some("abc")),
        None,
        "the retry is the one read, not a loop"
    );
}

#[test]
fn a_menu_answer_for_another_key_does_not_seed_the_page() {
    let mut state = typed("abc");
    assert_eq!(state.request(None), Some(search("abc")));
    state.apply(&reply("abc", &["spotify:track:a"]));
    // The user typed on, so the held answer is for a key nobody is on.
    type_text(&mut state, "abcd");

    state.open_results("abcd");
    assert_eq!(state.page_answer("abcd"), Some(&Load::Idle));
    assert_eq!(state.request(Some("abcd")), Some(search("abcd")));
}

#[test]
fn retry_page_never_seeds_from_the_menu() {
    let mut state = typed("abc");
    assert_eq!(state.request(None), Some(search("abc")));
    state.apply(&reply("abc", &["spotify:track:a"]));
    // The menu still holds this key's answer while the page has failed.
    state.open_results("abc");
    state.retry_page("abc");
    assert_eq!(state.request(Some("abc")), Some(search("abc")));
    state.apply(&failed("abc"));
    assert_eq!(
        state.page_answer("abc"),
        Some(&Load::Failed(Failure::Search))
    );

    state.retry_page("abc");
    assert_eq!(
        state.page_answer("abc"),
        Some(&Load::Idle),
        "a retry on a failure must not draw the menu's old answer"
    );
    assert_eq!(state.request(Some("abc")), Some(search("abc")));
}

#[test]
fn wants_is_true_exactly_when_request_would_send() {
    let mut state = Search::default();
    assert!(!state.wants(None));

    type_text(&mut state, "a");
    assert!(state.wants(None));
    assert_eq!(state.request(None), Some(search("a")));
    assert!(!state.wants(None), "already on the wire");

    state.apply(&reply("a", &["spotify:track:a"]));
    assert!(!state.wants(None), "answered");
    assert!(!state.wants(Some("a")), "the menu holds it and is not open");

    type_text(&mut state, "ab");
    assert!(state.wants(Some("ab")), "a page with no answer asks");
    assert_eq!(state.request(Some("ab")), Some(search("ab")));
}
