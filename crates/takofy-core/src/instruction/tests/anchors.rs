//! Which row a plan anchors on, given the rows a context read answers with.

use crate::Resolution;

use super::test_support::*;

/// The uris of `rows`, in the order given.
fn uris(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|uri| (*uri).to_owned()).collect()
}

#[test]
fn a_play_that_names_its_row_by_position_anchors_once_the_read_answers() {
    let plan = play_plan(play(
        Some(PLAYLIST),
        Vec::new(),
        skip_to(None, Some(1), None),
    ));

    assert_eq!(
        plan.anchor(&Resolution::Failed),
        None,
        "nothing was named by uri and the read brought no rows"
    );
    assert_eq!(
        plan.anchor(&Resolution::Resolved(Vec::new())),
        None,
        "an empty read is nothing to load either"
    );
    assert_eq!(
        plan.anchor(&Resolution::Resolved(vec![
            TRACK_1.to_owned(),
            TRACK_2.to_owned()
        ])),
        Some(TRACK_2.to_owned()),
        "the row the position names is the one to load"
    );
}

#[test]
fn a_play_that_names_its_row_by_uri_anchors_without_a_read() {
    let plan = play_plan(play(
        Some(PLAYLIST),
        Vec::new(),
        skip_to(Some(TRACK_3), None, None),
    ));

    assert_eq!(
        plan.anchor(&Resolution::Failed),
        Some(TRACK_3.to_owned()),
        "a named row is the row even when the context read fails"
    );
}

#[test]
fn a_play_of_a_local_or_unsupported_row_has_nothing_to_anchor_on() {
    let by_uri = play_plan(play(
        Some(PLAYLIST),
        Vec::new(),
        skip_to(Some(LOCAL), None, None),
    ));
    assert_eq!(
        by_uri.anchor(&Resolution::Resolved(uris(&[TRACK_1, TRACK_2]))),
        None,
        "a local file cannot be fetched, so a play of one is not a load"
    );

    let by_index = play_plan(play(
        Some(PLAYLIST),
        Vec::new(),
        skip_to(None, Some(1), None),
    ));
    assert_eq!(
        by_index.anchor(&Resolution::Resolved(uris(&[TRACK_1, LOCAL]))),
        None,
        "the row a position lands on is refused when that row is a local file"
    );
}

#[test]
fn a_failed_read_resolves_the_named_position_against_the_phones_pages() {
    // The read brought nothing, so the pages the play carried are all there
    // is to find the row its position names.
    let plan = play_plan(play(
        Some(PLAYLIST),
        vec![page(&[TRACK_1, TRACK_2, TRACK_3])],
        skip_to(None, Some(1), None),
    ));

    assert_eq!(
        plan.anchor(&Resolution::Failed),
        Some(TRACK_2.to_owned()),
        "a failed read still has the pages the phone sent"
    );

    // Past the pages, the last row they carry is the closest the position
    // can mean; the row it names is not among them.
    let past = play_plan(play(
        Some(PLAYLIST),
        vec![page(&[TRACK_1, TRACK_2])],
        skip_to(None, Some(3), None),
    ));
    assert_eq!(
        past.anchor(&Resolution::Failed),
        Some(TRACK_2.to_owned()),
        "the position is resolved against the pages, not against nothing"
    );
}

#[test]
fn a_play_with_a_track_uri_the_rows_lack_lands_on_that_row_alone() {
    // The phone named a row the context does not carry. It is still the row
    // it asked for, so it lands on that row alone rather than on a neighbour
    // or on nothing at all.
    let plan = play_plan(play(
        Some(PLAYLIST),
        Vec::new(),
        skip_to(Some(TRACK_4), None, None),
    ));

    assert_eq!(
        plan.anchor(&Resolution::Resolved(uris(&[TRACK_1, TRACK_2]))),
        Some(TRACK_4.to_owned())
    );
    let landing = landed(&plan, &[TRACK_1, TRACK_2]);
    assert_eq!(landing.rows, vec![TRACK_4.to_owned()]);
    assert_eq!(landing.index, 0);
}
