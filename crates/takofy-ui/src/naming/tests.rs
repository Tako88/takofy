use super::{NameError, Naming};
use takofy_model::PLAYLIST_NAME_MAX;

/// A naming field that is open with `text` in it.
fn typing(text: &str) -> Naming {
    let mut naming = Naming::default();
    naming.open();
    naming.edit().push_str(text);
    naming
}

#[test]
fn a_new_naming_is_closed_and_empty() {
    let naming = Naming::default();
    assert!(!naming.is_open());
    assert_eq!(naming.text(), "");
    assert!(!naming.writing());
    assert!(!naming.wants_focus());
    assert!(!naming.wants_button());
    assert_eq!(naming.name(), Err(NameError::Empty));
}

#[test]
fn open_sets_wants_focus_and_is_idempotent() {
    let mut naming = Naming::default();
    naming.open();
    assert!(naming.is_open());
    assert!(naming.wants_focus());

    naming.focus_taken();
    assert!(!naming.wants_focus());
    naming.edit().push_str("Mix");
    naming.open();
    assert!(naming.is_open(), "a second open closed the field");
    assert!(
        naming.wants_focus(),
        "a second open did not ask for the keyboard"
    );
    assert_eq!(naming.text(), "Mix", "a second open cleared the text");
}

#[test]
fn open_is_a_no_op_while_writing() {
    let mut naming = typing("Mix");
    assert_eq!(naming.begin_write(), Some("Mix".to_owned()));
    naming.cancel();
    assert!(naming.writing(), "a cancel ended a write in flight");
    naming.open();
    assert!(naming.writing());
    assert!(!naming.wants_focus(), "a write in flight took the keyboard");
}

#[test]
fn name_trims_both_ends_and_keeps_inner_spaces() {
    assert_eq!(
        typing("  Night  Drive \t").name(),
        Ok("Night  Drive".to_owned())
    );
}

#[test]
fn name_is_empty_for_blank_and_toolong_over_the_cap() {
    for blank in ["", " ", "\t\n"] {
        assert_eq!(typing(blank).name(), Err(NameError::Empty), "{blank:?}");
    }
    let mut naming = typing(&"a".repeat(PLAYLIST_NAME_MAX));
    assert!(naming.name().is_ok());
    naming.edit().push('a');
    assert_eq!(naming.name(), Err(NameError::TooLong));
}

#[test]
fn cancel_closes_clears_and_is_refused_while_writing() {
    let mut naming = typing("Mix");
    naming.cancel();
    assert!(!naming.is_open());
    assert_eq!(naming.text(), "");
    assert!(naming.wants_button(), "the keyboard was not handed back");

    let mut writing = typing("Mix");
    assert!(writing.begin_write().is_some());
    writing.cancel();
    assert!(writing.is_open(), "a cancel closed a field mid-write");
    assert_eq!(writing.text(), "Mix", "a cancel dropped the text mid-write");
}

#[test]
fn begin_write_requires_open_and_a_valid_name() {
    let mut closed = Naming::default();
    closed.edit().push_str("Mix");
    assert_eq!(closed.begin_write(), None, "a closed field wrote");

    let mut blank = typing("   ");
    assert_eq!(blank.begin_write(), None);
    assert!(!blank.writing());

    let mut naming = typing(" Mix ");
    assert_eq!(naming.begin_write(), Some("Mix".to_owned()));
    assert!(naming.writing());
    assert_eq!(naming.begin_write(), None, "a second write for one name");
}

#[test]
fn a_make_failure_reopens_for_editing_with_the_text_kept() {
    let mut naming = typing("Night Drive");
    assert!(naming.begin_write().is_some());
    naming.failed();
    assert!(!naming.writing());
    assert!(naming.is_open(), "a failed create closed the field");
    assert!(naming.wants_focus(), "a failed create left no keyboard");
    assert_eq!(naming.text(), "Night Drive");
}

fn held() -> Vec<String> {
    vec!["spotify:track:a".to_owned(), "spotify:track:b".to_owned()]
}

#[test]
fn open_with_keeps_the_pick_and_pick_reads_it() {
    let mut naming = Naming::default();
    naming.open_with(held());
    assert_eq!(naming.pick(), held());
    assert!(naming.is_open());
    assert!(naming.wants_focus());
}

#[test]
fn a_plain_open_drops_a_held_pick() {
    let mut naming = Naming::default();
    naming.open_with(held());
    naming.open();
    assert!(naming.pick().is_empty(), "a plain open kept the last pick");
}

#[test]
fn finished_and_cancel_drop_the_pick_but_failed_keeps_it() {
    let mut finished = Naming::default();
    finished.open_with(held());
    finished.edit().push_str("Mix");
    assert!(finished.begin_write().is_some());
    finished.finished();
    assert!(finished.pick().is_empty(), "a made list keeps no pick");

    let mut cancelled = Naming::default();
    cancelled.open_with(held());
    cancelled.cancel();
    assert!(
        cancelled.pick().is_empty(),
        "a cancelled field keeps no pick"
    );

    let mut failed = Naming::default();
    failed.open_with(held());
    failed.edit().push_str("Mix");
    assert!(failed.begin_write().is_some());
    failed.failed();
    assert_eq!(failed.pick(), held(), "a retry lands the same pick");
}

#[test]
fn an_open_with_while_writing_leaves_it_empty() {
    let mut naming = typing("Mix");
    assert!(naming.begin_write().is_some());
    naming.open_with(held());
    assert!(
        naming.pick().is_empty(),
        "a write in flight took a new pick"
    );
    assert!(naming.writing());
    assert!(!naming.wants_focus());
}

#[test]
fn a_link_failure_or_success_closes_and_clears() {
    // Both ends are `finished`: a success and a failed link leave the same
    // field, because the playlist exists either way.
    let mut naming = typing("Night Drive");
    assert!(naming.begin_write().is_some());
    naming.finished();
    assert!(!naming.is_open());
    assert!(!naming.writing());
    assert_eq!(naming.text(), "");
    assert!(naming.wants_button());
}

mod renaming_tests {
    use super::super::{NameError, Renaming, Surface};
    use takofy_model::PLAYLIST_NAME_MAX;

    const URI: &str = "spotify:playlist:a";

    fn typing(text: &str) -> Renaming {
        let mut renaming = Renaming::default();
        renaming.open(URI, "Old Name", Surface::Row);
        *renaming.edit() = text.to_owned();
        renaming
    }

    #[test]
    fn a_new_rename_is_closed_and_empty() {
        let renaming = Renaming::default();
        assert!(!renaming.is_open());
        assert_eq!(renaming.uri(), "");
        assert_eq!(renaming.text(), "");
        assert!(!renaming.writing());
        assert!(!renaming.wants_focus());
        assert!(!renaming.wants_return());
        assert_eq!(renaming.name(), Err(NameError::Empty));
    }

    #[test]
    fn open_prefills_the_current_name_and_asks_for_focus() {
        let mut renaming = Renaming::default();
        renaming.open(URI, "Night Drive", Surface::Band);
        assert!(renaming.is_open());
        assert_eq!(renaming.uri(), URI);
        assert_eq!(renaming.surface(), Surface::Band);
        assert_eq!(renaming.text(), "Night Drive");
        assert!(renaming.wants_focus());
        assert!(!renaming.wants_return());
        assert!(renaming.is_open_on(URI, Surface::Band));
        assert!(!renaming.is_open_on(URI, Surface::Row));
        assert!(!renaming.is_open_on("spotify:playlist:b", Surface::Band));
    }

    #[test]
    fn open_is_refused_while_writing() {
        let mut renaming = typing("Mix");
        assert!(renaming.begin_write().is_some());
        renaming.open("spotify:playlist:b", "Other", Surface::Band);
        assert_eq!(renaming.uri(), URI);
        assert_eq!(renaming.text(), "Mix");
        assert!(renaming.writing());
    }

    #[test]
    fn name_uses_the_same_rules_as_the_create() {
        assert_eq!(
            typing("  Night  Drive \t").name(),
            Ok("Night  Drive".to_owned())
        );
        for blank in ["", " ", "\t\n"] {
            assert_eq!(typing(blank).name(), Err(NameError::Empty), "{blank:?}");
        }
        let mut long = typing(&"a".repeat(PLAYLIST_NAME_MAX));
        assert!(long.name().is_ok());
        long.edit().push('a');
        assert_eq!(long.name(), Err(NameError::TooLong));
    }

    #[test]
    fn begin_write_requires_open_and_a_valid_name_and_runs_once() {
        let mut closed = Renaming::default();
        closed.edit().push_str("Mix");
        assert_eq!(closed.begin_write(), None);

        let mut blank = typing("   ");
        assert_eq!(blank.begin_write(), None);
        assert!(!blank.writing());

        let mut renaming = typing(" Mix ");
        assert_eq!(renaming.begin_write(), Some("Mix".to_owned()));
        assert!(renaming.writing());
        assert!(!renaming.wants_focus());
        assert_eq!(renaming.begin_write(), None, "a second write for one name");
    }

    #[test]
    fn cancel_closes_and_returns_the_keyboard_but_not_while_writing() {
        let mut renaming = typing("Mix");
        renaming.cancel();
        assert!(!renaming.is_open());
        assert_eq!(renaming.text(), "");
        assert!(renaming.wants_return());

        let mut writing = typing("Mix");
        assert!(writing.begin_write().is_some());
        writing.cancel();
        assert!(writing.is_open(), "a cancel closed a field mid-write");
        assert_eq!(writing.text(), "Mix");
        assert!(!writing.wants_return());
    }

    #[test]
    fn dismiss_closes_without_returning_the_keyboard_and_is_refused_while_writing() {
        let mut renaming = typing("Mix");
        renaming.dismiss();
        assert!(!renaming.is_open());
        assert_eq!(renaming.text(), "");
        assert!(!renaming.wants_return());

        let mut writing = typing("Mix");
        assert!(writing.begin_write().is_some());
        writing.dismiss();
        assert!(writing.is_open(), "a dismiss closed a field mid-write");
    }

    #[test]
    fn failed_keeps_the_text_and_asks_for_focus_again() {
        let mut renaming = typing("Night Drive");
        assert!(renaming.begin_write().is_some());
        renaming.failed();
        assert!(!renaming.writing());
        assert!(renaming.is_open());
        assert!(renaming.wants_focus());
        assert_eq!(renaming.text(), "Night Drive");
    }

    #[test]
    fn finished_closes_clears_and_returns_the_keyboard() {
        let mut renaming = typing("Night Drive");
        assert!(renaming.begin_write().is_some());
        renaming.finished();
        assert!(!renaming.is_open());
        assert!(!renaming.writing());
        assert_eq!(renaming.text(), "");
        assert!(renaming.wants_return());
    }

    #[test]
    fn focus_and_return_are_taken_once() {
        let mut renaming = typing("Mix");
        assert!(renaming.wants_focus());
        renaming.focus_taken();
        assert!(!renaming.wants_focus());

        renaming.cancel();
        assert!(renaming.wants_return());
        renaming.return_taken();
        assert!(!renaming.wants_return());
    }

    #[test]
    fn a_closed_field_that_fails_asks_for_nothing() {
        let mut renaming = Renaming::default();
        renaming.failed();

        assert!(!renaming.is_open());
        assert!(
            !renaming.wants_focus(),
            "a field nobody drew took the keyboard"
        );
    }

    #[test]
    fn a_cancel_or_a_dismiss_is_the_user_leaving() {
        let mut cancelled = typing("Mix");
        assert!(!cancelled.take_left());
        cancelled.cancel();
        assert!(cancelled.take_left());
        assert!(!cancelled.take_left(), "the leave was read twice");

        let mut dismissed = typing("Mix");
        dismissed.dismiss();
        assert!(dismissed.take_left());

        let mut finished = typing("Mix");
        assert!(finished.begin_write().is_some());
        finished.finished();
        assert!(
            !finished.take_left(),
            "a confirmed rename is the reply's, not the user's leaving"
        );
    }

    #[test]
    fn force_close_forgets_the_target_and_returns_nothing() {
        let mut renaming = typing("Mix");
        renaming.force_close();
        assert!(!renaming.is_open());
        assert_eq!(renaming.uri(), "");
        assert!(!renaming.wants_return());
    }
    #[test]
    fn a_return_keeps_the_target_until_the_keyboard_is_back() {
        let mut cancelled = typing("Mix");
        cancelled.cancel();
        assert_eq!(cancelled.uri(), URI, "a cancel forgot where to return to");
        cancelled.return_taken();
        assert_eq!(cancelled.uri(), "");

        let mut finished = typing("Mix");
        assert!(finished.begin_write().is_some());
        finished.finished();
        assert_eq!(finished.uri(), URI, "a success forgot where to return to");
        finished.return_taken();
        assert_eq!(finished.uri(), "");
    }
}
