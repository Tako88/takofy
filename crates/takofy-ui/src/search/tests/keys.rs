use super::super::*;
use super::test_support::*;

/// The field with an answer for its key, open as typing leaves it.
fn answered_open(text: &str, uris: &[&str]) -> Search {
    let mut state = typed(text);
    state.apply(&reply(text, uris));
    state
}

fn no_floor_reply(query: &str) -> Update {
    Update::Search {
        query: query.to_owned(),
        tracks: vec![
            rich("t0", "a0", "r0"),
            rich("t1", "a1", "r1"),
            rich("t2", "a1", "r1"),
        ],
    }
}

#[test]
fn down_marks_the_first_row_from_the_field() {
    let mut state = answered_open("abc", &["a", "b", "c"]);
    assert_eq!(
        state.mark(),
        None,
        "the menu opens with the mark on the field"
    );
    assert_eq!(state.step(key("down")), Act::None);
    assert_eq!(state.mark(), Some(0));
    assert_eq!(state.device(), Device::Keyboard);
}

#[test]
fn down_clamps_at_the_last_row() {
    let mut state = answered_open("abc", &["a", "b", "c"]);
    for expected in [0, 1, 2, 2, 2] {
        state.step(key("down"));
        assert_eq!(state.mark(), Some(expected));
    }
    let mut with_floor = answered_open("abc", &["a", "b", "c", "d"]);
    for _ in 0..9 {
        with_floor.step(key("down"));
    }
    assert_eq!(with_floor.mark(), Some(3), "three rows and a floor");
}

#[test]
fn up_from_the_first_row_returns_to_the_field() {
    let mut state = answered_open("abc", &["a", "b", "c"]);
    state.step(key("down"));
    assert_eq!(state.mark(), Some(0));
    assert_eq!(state.step(key("up")), Act::None);
    assert_eq!(state.mark(), None, "row 0 is one press from the field");
}

#[test]
fn up_from_the_field_marks_the_last_row() {
    let mut state = answered_open("abc", &["a", "b", "c"]);
    state.step(key("up"));
    assert_eq!(state.mark(), Some(2));

    let mut with_floor = answered_open("abc", &["a", "b", "c", "d"]);
    with_floor.step(key("up"));
    assert_eq!(
        with_floor.mark(),
        Some(3),
        "the floor is drawn, so it is last"
    );
}

#[test]
fn the_walk_crosses_tracks_then_albums_then_artists_then_the_floor() {
    let mut state = banded();
    let expected = [
        Row::Track(0),
        Row::Track(1),
        Row::Track(2),
        Row::Album(0),
        Row::Album(1),
        Row::Artist(0),
        Row::Artist(1),
        Row::Floor,
    ];
    for (ordinal, row) in expected.into_iter().enumerate() {
        state.step(key("down"));
        assert_eq!(state.mark(), Some(ordinal));
        assert_eq!(state.marked(), Some(row), "ordinal {ordinal}");
    }
    state.step(key("down"));
    assert_eq!(state.mark(), Some(7), "the floor is the last row");
}

#[test]
fn without_a_floor_the_walk_ends_on_the_last_shown_row() {
    let mut state = typed("abc");
    state.apply(&no_floor_reply("abc"));
    for _ in 0..9 {
        state.step(key("down"));
    }
    assert_eq!(state.marked(), Some(Row::Artist(1)));
    assert_eq!(state.mark(), Some(6));
}

#[test]
fn enter_on_a_track_plays_its_whole_answer_index() {
    let mut state = answered_open("abc", &["a", "b", "c"]);
    state.step(key("down"));
    state.step(key("down"));
    assert_eq!(state.step(key("enter")), Act::Play(1));
}

#[test]
fn enter_on_an_album_and_an_artist_opens_the_derived_row() {
    let mut state = banded();
    for _ in 0..4 {
        state.step(key("down"));
    }
    assert_eq!(state.marked(), Some(Row::Album(0)));
    assert_eq!(state.step(key("enter")), Act::OpenAlbum(0));

    let mut state = banded();
    for _ in 0..6 {
        state.step(key("down"));
    }
    assert_eq!(state.marked(), Some(Row::Artist(0)));
    assert_eq!(state.step(key("enter")), Act::OpenArtist(0));
}

#[test]
fn enter_on_the_floor_opens_the_results() {
    let mut state = answered_open("abc", &["a", "b", "c", "d"]);
    for _ in 0..6 {
        state.step(key("down"));
    }
    assert_eq!(state.marked(), Some(Row::Floor));
    assert_eq!(state.step(key("enter")), Act::OpenResults);
}

#[test]
fn a_floor_does_not_exist_when_no_band_is_capped() {
    let mut state = typed("abc");
    state.apply(&no_floor_reply("abc"));
    for _ in 0..12 {
        state.step(key("down"));
    }
    assert_eq!(
        state.marked(),
        Some(Row::Artist(1)),
        "the walk never lands on a floor"
    );
    assert!(state.listing().is_some_and(|listing| !listing.shape.floor));
}

#[test]
fn enter_on_a_stale_ordinal_opens_the_results() {
    let mut state = banded();
    state.step(key("down"));
    state.step(key("down"));
    state.step(key("down"));
    state.step(key("down"));
    assert_eq!(state.marked(), Some(Row::Album(0)));
    // The key moves and the new answer is too short for the old ordinal.
    type_text(&mut state, "abcd");
    state.apply(&rich_reply("abcd", 1));
    state.mark = Some(3);
    assert_eq!(state.mark(), Some(3));
    assert_eq!(state.marked(), None);
    assert_eq!(state.step(key("enter")), Act::OpenResults);
}

#[test]
fn a_stale_key_counts_zero_rows_and_opens_nothing() {
    let mut state = answered_open("abc", &["a", "b", "c"]);
    state.step(key("down"));
    assert_eq!(state.mark(), Some(0));

    // The frame that types moves the key before the arrow is read.
    state.text = "abcd".to_owned();
    let typed_and_down = KeyInput {
        typed: true,
        down: true,
        focused: true,
        ..KeyInput::default()
    };
    assert_eq!(state.step(typed_and_down), Act::None);
    assert_eq!(state.mark(), None, "the stale answer contributes no rows");
}

#[test]
fn album_and_artist_uris_resolve_only_for_the_current_key() {
    let mut state = banded();
    assert_eq!(state.album_uri(0).as_deref(), Some("spotify:album:a0"));
    assert_eq!(state.artist_uri(0).as_deref(), Some("spotify:artist:r0"));
    assert_eq!(state.album_uri(9), None, "out of range");
    assert_eq!(state.artist_uri(9), None, "out of range");

    type_text(&mut state, "abcd");
    assert_eq!(state.album_uri(0), None, "the key moved");
    assert_eq!(state.artist_uri(0), None, "the key moved");
}

#[test]
fn down_in_the_frame_that_types_is_a_no_op() {
    let mut state = answered_open("abc", &["a", "b", "c"]);
    let frame = KeyInput {
        typed: true,
        down: true,
        focused: true,
        ..KeyInput::default()
    };
    state.text = "abcd".to_owned();
    assert_eq!(state.step(frame), Act::None);
    assert_eq!(state.mark(), None);
    assert_eq!(state.menu_view(), MenuView::Nothing);
}

/// The field with an answered menu, closed again as an applied action
/// leaves it.
fn answered_closed(text: &str, uris: &[&str]) -> Search {
    let mut state = answered_open(text, uris);
    state.close();
    assert!(!state.is_open());
    state
}

#[test]
fn down_on_a_closed_menu_reopens_it_and_marks_the_first_row() {
    let mut state = answered_closed("abc", &["a", "b", "c"]);
    assert_eq!(state.step(key("down")), Act::None);
    assert!(state.is_open(), "the arrow re-opens the menu it acts on");
    assert_eq!(state.mark(), Some(0));
    assert_eq!(state.device(), Device::Keyboard);
}

#[test]
fn up_on_a_closed_menu_reopens_it_and_marks_the_last_row() {
    let mut state = answered_closed("abc", &["a", "b", "c"]);
    assert_eq!(state.step(key("up")), Act::None);
    assert!(state.is_open());
    assert_eq!(state.mark(), Some(2));

    let mut with_floor = answered_closed("abc", &["a", "b", "c", "d"]);
    with_floor.step(key("up"));
    assert_eq!(with_floor.mark(), Some(3), "the floor is the last row");
}

#[test]
fn an_arrow_on_a_closed_blank_field_does_nothing() {
    let mut blank = Search::default();
    assert_eq!(blank.step(key("down")), Act::None);
    assert_eq!(blank.step(key("up")), Act::None);
    assert!(!blank.is_open(), "a blank key re-opens nothing");
    assert_eq!(blank.mark(), None);

    // Whitespace is not a key either.
    let mut spaces = typed("   ");
    assert_eq!(spaces.step(key("down")), Act::None);
    assert_eq!(spaces.step(key("up")), Act::None);
    assert!(!spaces.is_open());
    assert_eq!(spaces.mark(), None);
}

#[test]
fn an_arrow_marks_nothing_when_no_row_is_drawn() {
    // Open, and the answer is still on the wire: nothing to mark.
    let mut loading = typed("abc");
    assert_eq!(loading.request(None), Some(search("abc")));
    assert_eq!(loading.step(key("down")), Act::None);
    assert_eq!(loading.mark(), None);
    assert_eq!(loading.step(key("up")), Act::None);
    assert_eq!(loading.mark(), None);

    // Open and failed: nothing to mark.
    loading.apply(&failed("abc"));
    assert_eq!(loading.step(key("down")), Act::None);
    assert_eq!(loading.mark(), None);

    // Open and empty: nothing to mark.
    let mut empty = typed("abc");
    empty.apply(&reply("abc", &[]));
    assert_eq!(empty.step(key("down")), Act::None);
    assert_eq!(empty.mark(), None);

    // Closed with a query but no answer to draw: it re-opens, marks
    // nothing, because a mark must be a row that exists.
    let mut closed = typed("abc");
    closed.close();
    closed.step(key("down"));
    assert!(closed.is_open());
    assert_eq!(closed.mark(), None);
}

#[test]
fn enter_after_close_is_the_fields_enter_not_a_play() {
    let mut state = answered_closed("abc", &["a", "b", "c"]);
    // A mark from before the close must not survive into an Enter.
    state.step(key("down"));
    assert_eq!(state.mark(), Some(0));
    state.close();
    assert_eq!(state.mark(), None);
    assert_eq!(state.step(key("enter")), Act::OpenResults);
}

#[test]
fn enter_on_the_field_opens_the_results() {
    let mut state = answered_open("abc", &["a", "b", "c"]);
    assert_eq!(state.mark(), None);
    assert_eq!(state.step(key("enter")), Act::OpenResults);
}

#[test]
fn enter_with_a_blank_key_does_nothing() {
    let mut state = typed("   ");
    assert_eq!(state.step(key("enter")), Act::None);
    // A mark left behind a blanked field opens nothing either.
    state.step(key("down"));
    assert_eq!(state.step(key("enter")), Act::None);
}

#[test]
fn escape_from_a_row_keeps_the_query_and_the_menu() {
    let mut state = answered_open("abc", &["a", "b", "c"]);
    state.step(key("down"));
    assert_eq!(state.step(key("escape")), Act::None);
    assert_eq!(state.mark(), None);
    assert_eq!(state.text, "abc", "the query is kept");
    assert!(state.is_open(), "and the menu stays up");
    assert!(state.escapes(), "Escape still has something to undo");
}

#[test]
fn escape_from_the_field_clears_the_query_and_closes_the_menu() {
    let mut state = typed("abc");
    assert_eq!(state.step(key("escape")), Act::Close);
    assert_eq!(state.text, "");
    assert!(!state.is_open());
    assert_eq!(state.mark(), None);
    assert!(!state.escapes(), "nothing left to undo");
}

#[test]
fn escape_with_an_empty_closed_field_is_not_ours() {
    let mut state = Search::default();
    assert!(!state.escapes());
    assert_eq!(state.step(key("escape")), Act::None);
    assert_eq!(state.text, "");
    assert!(!state.is_open());
}

#[test]
fn a_frame_that_starts_unfocused_does_nothing() {
    let mut state = answered_open("abc", &["a", "b", "c"]);
    state.step(key("down"));
    let blurred = KeyInput {
        focused: false,
        ..KeyInput::default()
    };
    assert_eq!(state.step(blurred), Act::None);
    assert_eq!(state.mark(), Some(0), "the mark is the shell's to clear");

    let blurred_escape = KeyInput {
        escape: true,
        focused: false,
        ..KeyInput::default()
    };
    assert_eq!(state.step(blurred_escape), Act::None);
    assert_eq!(state.text, "abc", "a blurred field does not eat keys");
    assert!(state.is_open());
}

#[test]
fn typing_returns_the_rule_to_the_field() {
    let mut state = answered_open("abc", &["a", "b", "c"]);
    state.step(key("down"));
    assert_eq!(state.mark(), Some(0));
    type_text(&mut state, "abcd");
    assert_eq!(state.mark(), None, "typing puts the rule back on the field");
    assert!(state.is_open());
}

#[test]
fn a_click_on_the_field_returns_the_rule_and_keeps_the_menu() {
    let mut state = answered_open("abc", &["a", "b", "c"]);
    state.pointer_moved();
    state.step(key("down"));
    assert_eq!(state.device(), Device::Keyboard);
    state.settle(false, false);
    assert!(!state.is_open(), "the menu closes when the field is left");

    let click = KeyInput {
        clicked_field: true,
        focused: true,
        ..KeyInput::default()
    };
    assert_eq!(state.step(click), Act::None);
    state.pointer_moved();
    assert!(
        state.is_open(),
        "a click re-opens it on the query it already held"
    );
    assert_eq!(state.mark(), None);
}

#[test]
fn settle_closes_the_menu_only_when_nothing_is_engaged() {
    let mut state = typed("abc");
    state.settle(true, false);
    assert!(state.is_open(), "a focused field keeps the menu");

    state.settle(false, true);
    assert!(state.is_open(), "the pointer on the menu keeps it");

    state.settle(false, false);
    assert!(!state.is_open());
    assert_eq!(state.mark(), None);

    // A closed menu is not disturbed by a later settled frame.
    state.settle(false, false);
    assert!(!state.is_open());
}

#[test]
fn closing_an_applied_action_keeps_the_query() {
    let mut state = answered_open("abc", &["a", "b", "c"]);
    state.step(key("down"));
    state.close();
    assert!(!state.is_open());
    assert_eq!(state.mark(), None);
    assert_eq!(state.text, "abc");
}

/// A control outside the field can ask for its keyboard, and the ask is taken
/// once: a press is one move, not a lock.
#[test]
fn the_focus_ask_is_consumed_once() {
    let mut state = Search::default();
    assert!(!state.take_focus_ask(), "nothing was asked for");

    state.ask_focus();
    assert!(state.take_focus_ask());
    assert!(!state.take_focus_ask(), "one press is one ask");
}
