//! A play decodes into the transfer plan its context, rows and options describe.

use librespot::core::dealer::protocol::PlayOptions;
use librespot::protocol::context_player_options::ContextPlayerOptionOverrides;

use super::super::*;
use super::test_support::*;

#[test]
fn play_with_a_uri_resolves_it() {
    let plan = play_plan(play(
        Some(PLAYLIST),
        vec![page(&[TRACK_1, TRACK_2])],
        play_options(),
    ));

    assert_eq!(plan.resolve_uri(), Some(PLAYLIST));
    assert_eq!(plan.context.as_deref(), Some(PLAYLIST));
    assert_eq!(plan.current, TRACK_1, "a play with no skip_to starts it");
    assert_eq!(
        plan.pages.len(),
        2,
        "the phone's pages are kept for a read that fails"
    );
}

#[test]
fn play_with_pages_only_uses_them() {
    for context in [None, Some(""), Some("-")] {
        let plan = play_plan(play(
            context,
            vec![page(&[TRACK_1, TRACK_2, TRACK_3])],
            play_options(),
        ));

        assert_eq!(plan.resolve_uri(), None, "{context:?}");
        assert_eq!(plan.context, None, "{context:?}");
        assert_eq!(
            plan.pages,
            vec![TRACK_1.to_owned(), TRACK_2.to_owned(), TRACK_3.to_owned()],
            "{context:?}"
        );
        assert_eq!(plan.current, TRACK_1, "{context:?}");
    }
}

#[test]
fn play_with_neither_is_refused() {
    assert_eq!(
        instruction_of(&request(play(None, Vec::new(), play_options()))),
        Instruction::Refused(Refusal::NoData)
    );
}

#[test]
fn play_skip_to_by_uri_lands_on_that_row() {
    let plan = play_plan(play(
        Some(PLAYLIST),
        Vec::new(),
        skip_to(Some(TRACK_3), Some(0), Some("uid-1")),
    ));

    let landing = landed(&plan, &[TRACK_1, TRACK_2, TRACK_3, TRACK_4]);
    assert_eq!(landing.index, 2, "the uri names the row, not the index");
    assert_eq!(landing.rows.len(), 4);
}

#[test]
fn play_skip_to_by_index_lands_on_that_row_when_there_is_no_uri() {
    let plan = play_plan(play(
        Some(PLAYLIST),
        Vec::new(),
        skip_to(None, Some(3), Some("uid-4")),
    ));

    let landing = landed(&plan, &[TRACK_1, TRACK_2, TRACK_3, TRACK_4]);
    assert_eq!(landing.index, 3);
    assert_eq!(landing.rows[3], TRACK_4);
}

#[test]
fn play_skip_to_with_no_fields_starts_the_context() {
    let plan = play_plan(play(Some(PLAYLIST), Vec::new(), skip_to(None, None, None)));

    let landing = landed(&plan, &[TRACK_1, TRACK_2, TRACK_3]);
    assert_eq!(landing.index, 0);
    assert_eq!(landing.rows[0], TRACK_1);
}

#[test]
fn play_skip_to_an_index_past_the_context_lands_on_its_last_row() {
    let plan = play_plan(play(
        Some(PLAYLIST),
        Vec::new(),
        skip_to(None, Some(99), None),
    ));

    let landing = landed(&plan, &[TRACK_1, TRACK_2]);
    assert_eq!(
        landing.index, 1,
        "a position past the rows the read answered is the last one"
    );
    assert_eq!(landing.rows.len(), 2);
}

#[test]
fn play_applies_seek_and_initially_paused() {
    let options = PlayOptions {
        seek_to: Some(42_000),
        initially_paused: Some(true),
        ..play_options()
    };
    let plan = play_plan(play(
        Some(PLAYLIST),
        vec![page(&[TRACK_1, TRACK_2])],
        options,
    ));

    assert_eq!(plan.position_ms, Some(42_000));
    assert!(plan.paused);
    let landing = landed(&plan, &[TRACK_1, TRACK_2]);
    assert_eq!(landing.position_ms, 42_000, "a seek is taken as it is");
    assert!(!landing.playing, "initially paused means it does not start");

    let playing = play_plan(play(Some(PLAYLIST), vec![page(&[TRACK_1])], play_options()));
    assert_eq!(playing.position_ms, None);
    assert!(!playing.paused, "a play that says nothing starts at once");
}

#[test]
fn play_carries_the_options_override() {
    let options = PlayOptions {
        player_options_override: Some(ContextPlayerOptionOverrides {
            shuffling_context: Some(true),
            repeating_context: Some(false),
            repeating_track: Some(true),
            ..Default::default()
        }),
        ..play_options()
    };
    let plan = play_plan(play(None, vec![page(&[TRACK_1])], options));

    assert_eq!(plan.shuffle, Some(true));
    assert_eq!(plan.repeat_context, Some(false));
    assert_eq!(plan.repeat_track, Some(true));

    // A play that names no options names no mode, so the queue keeps the
    // ones it has rather than having them read back as off.
    let bare = play_plan(play(None, vec![page(&[TRACK_1])], play_options()));
    assert_eq!(bare.shuffle, None);
    assert_eq!(bare.repeat_context, None);
    assert_eq!(bare.repeat_track, None);
}

#[test]
fn play_of_an_unsupported_context_is_refused() {
    assert_eq!(
        instruction_of(&request(play(
            Some(ARTIST),
            Vec::new(),
            skip_to(Some(TRACK_1), None, None),
        ))),
        Instruction::Refused(Refusal::UnsupportedContext(ContextKind::Artist))
    );
}
