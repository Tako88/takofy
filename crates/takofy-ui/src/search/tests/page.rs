use super::super::*;
use super::test_support::*;

// The page's own view.

fn shares(state: &Search, query: &str) -> Vec<String> {
    match state.page_view(query) {
        Some(PageView::Ready(listing)) => listing
            .shape
            .bands
            .iter()
            .map(crate::search_index::Band::share)
            .collect(),
        other => panic!("expected a ready page, got {other:?}"),
    }
}

#[test]
fn the_page_view_is_none_for_another_query() {
    let state = typed("abc");
    assert_eq!(state.page_view("xyz"), None);
}

#[test]
fn the_page_view_follows_the_load() {
    let mut state = typed("abc");
    state.open_results("abc");
    assert_eq!(state.page_view("abc"), Some(PageView::Waiting));
    state.request_page(Some("abc"));
    assert_eq!(state.page_view("abc"), Some(PageView::Waiting));
    state.apply(&failed("abc"));
    assert_eq!(
        state.page_view("abc"),
        Some(PageView::Failed(Failure::Search))
    );
    state.retry_page("abc");
    state.apply(&reply("abc", &[]));
    assert_eq!(state.page_view("abc"), Some(PageView::Empty));

    let ready = paged(2);
    let Some(PageView::Ready(listing)) = ready.page_view("abc") else {
        panic!("a ready page");
    };
    assert_eq!(listing.answer.len(), 2);
    assert_eq!(listing.index.tracks, 2);
}

#[test]
fn a_ready_page_is_capped_at_ten_of_each_kind_until_a_reveal_is_taken() {
    let mut state = paged(25);
    assert_eq!(shares(&state, "abc"), ["10 of 25", "10 of 25", "10 of 25"]);

    state.take_more("abc", Kind::Tracks);
    assert_eq!(shares(&state, "abc"), ["25", "10 of 25", "10 of 25"]);

    state.take_more("abc", Kind::Artists);
    assert_eq!(shares(&state, "abc"), ["25", "10 of 25", "25"]);
}

#[test]
fn a_reveal_changes_one_block_and_leaves_the_others_whole() {
    let before = paged(25);
    let Some(PageView::Ready(before)) = before.page_view("abc") else {
        panic!("a ready page");
    };
    let albums = before.index.albums.clone();
    let artists = before.index.artists.clone();

    let mut after = paged(25);
    after.take_more("abc", Kind::Tracks);
    let Some(PageView::Ready(after)) = after.page_view("abc") else {
        panic!("a ready page");
    };
    assert_eq!(after.index.albums, albums);
    assert_eq!(after.index.artists, artists);
    assert_eq!(after.shape.bands[0].shown, 25);
    assert_eq!(
        (after.shape.bands[1].shown, after.shape.bands[2].shown),
        (10, 10)
    );
}

#[test]
fn a_take_for_another_query_or_a_read_that_is_not_ready_does_nothing() {
    let mut state = paged(25);
    state.take_more("xy", Kind::Tracks);
    assert_eq!(shares(&state, "abc"), ["10 of 25", "10 of 25", "10 of 25"]);

    let mut waiting = typed("abc");
    waiting.open_results("abc");
    assert_eq!(waiting.page_view("abc"), Some(PageView::Waiting));
    waiting.take_more("abc", Kind::Tracks);
    waiting.apply(&rich_reply("abc", 25));
    assert_eq!(
        shares(&waiting, "abc"),
        ["10 of 25", "10 of 25", "10 of 25"]
    );
}

#[test]
fn a_new_query_resets_the_reveal_and_the_same_one_keeps_it() {
    let mut state = paged(25);
    state.take_more("abc", Kind::Tracks);
    state.open_results("abc");
    assert_eq!(shares(&state, "abc")[0], "25");
    state.retry_page("abc");
    state.apply(&rich_reply("abc", 25));
    assert_eq!(shares(&state, "abc")[0], "25", "a retry keeps it");

    state.open_results("abcd");
    state.apply(&rich_reply("abcd", 25));
    assert_eq!(
        shares(&state, "abcd")[0],
        "10 of 25",
        "a new query starts over"
    );
    state.open_results("abc");
    state.apply(&rich_reply("abc", 25));
    assert_eq!(
        shares(&state, "abc")[0],
        "10 of 25",
        "and coming back through another query starts over too"
    );
}

#[test]
fn a_page_visit_moves_for_a_new_query_and_keeps_a_reentry() {
    let mut state = paged(25);
    let first = state.page_visit();
    state.open_results("abc");
    assert_eq!(
        state.page_visit(),
        first,
        "the same query is the same visit, so the page keeps its offset"
    );
    state.retry_page("abc");
    assert_eq!(state.page_visit(), first, "a retry keeps its offset too");

    state.open_results("abcd");
    let second = state.page_visit();
    assert_ne!(second, first, "another query is another visit");
    state.open_results("abc");
    assert_ne!(
        state.page_visit(),
        first,
        "A→B→A is a fresh page, so it starts at the top"
    );
    assert_ne!(state.page_visit(), second);
}

#[test]
fn clearing_the_text_closes_nothing_by_itself() {
    // The clear control edits the text programmatically: only the key moves
    // here, and the shell's `settle` is what closes the menu.
    let mut state = typed("abc");
    assert_eq!(state.request(None), Some(search("abc")));
    state.apply(&reply("abc", &["spotify:track:a"]));
    state.text.clear();
    state.text_changed();
    assert_eq!(state.menu_view(), MenuView::Nothing);
    assert_eq!(state.request(None), None, "a blank key asks for nothing");
    assert!(state.is_open(), "only `settle` or Escape closes the menu");
}
