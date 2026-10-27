use super::*;

const MODES: [Repeat; 3] = [Repeat::Off, Repeat::All, Repeat::One];
const CAUSES: [Cause; 3] = [Cause::Ended, Cause::Unavailable, Cause::Manual];

/// The row a hop names from `index`, which is what the queue would play.
fn row(hop: Hop, index: usize) -> usize {
    match hop {
        Hop::Next => index + 1,
        Hop::Wrap => 0,
        Hop::Replay => index,
    }
}

#[test]
fn advance_matches_the_contract_table() {
    let len = 3;
    let cases: [(Repeat, Cause, [Option<Hop>; 3]); 9] = [
        (
            Repeat::Off,
            Cause::Ended,
            [Some(Hop::Next), Some(Hop::Next), None],
        ),
        (
            Repeat::Off,
            Cause::Unavailable,
            [Some(Hop::Next), Some(Hop::Next), None],
        ),
        (
            Repeat::Off,
            Cause::Manual,
            [Some(Hop::Next), Some(Hop::Next), None],
        ),
        (
            Repeat::All,
            Cause::Ended,
            [Some(Hop::Next), Some(Hop::Next), Some(Hop::Wrap)],
        ),
        (
            Repeat::All,
            Cause::Unavailable,
            [Some(Hop::Next), Some(Hop::Next), Some(Hop::Wrap)],
        ),
        (
            Repeat::All,
            Cause::Manual,
            [Some(Hop::Next), Some(Hop::Next), Some(Hop::Wrap)],
        ),
        (
            Repeat::One,
            Cause::Ended,
            [Some(Hop::Replay), Some(Hop::Replay), Some(Hop::Replay)],
        ),
        (
            Repeat::One,
            Cause::Unavailable,
            [Some(Hop::Next), Some(Hop::Next), None],
        ),
        (
            Repeat::One,
            Cause::Manual,
            [Some(Hop::Next), Some(Hop::Next), None],
        ),
    ];
    for (repeat, cause, expected) in cases {
        for (index, expected) in expected.into_iter().enumerate() {
            assert_eq!(
                advance(repeat, Some(index), len, cause),
                expected,
                "{repeat:?} {cause:?} at row {index}"
            );
        }
    }
}

#[test]
fn one_acts_as_off_for_unavailable_and_manual() {
    for len in 0..=5 {
        for index in 0..=len {
            for cause in [Cause::Unavailable, Cause::Manual] {
                assert_eq!(
                    advance(Repeat::One, Some(index), len, cause),
                    advance(Repeat::Off, Some(index), len, cause),
                    "len {len} at row {index}, {cause:?}"
                );
            }
        }
    }
}

#[test]
fn a_single_row_wraps_or_replays_onto_itself() {
    let cases = [
        (Repeat::Off, Cause::Ended, None),
        (Repeat::Off, Cause::Manual, None),
        (Repeat::All, Cause::Ended, Some(Hop::Wrap)),
        (Repeat::All, Cause::Manual, Some(Hop::Wrap)),
        (Repeat::One, Cause::Ended, Some(Hop::Replay)),
        (Repeat::One, Cause::Manual, None),
    ];
    for (repeat, cause, expected) in cases {
        let hop = advance(repeat, Some(0), 1, cause);
        assert_eq!(hop, expected, "{repeat:?} {cause:?}");
        if let Some(hop) = hop {
            assert_eq!(row(hop, 0), 0, "{repeat:?} {cause:?} left the only row");
        }
    }
}

#[test]
fn advance_with_no_rows_or_a_stale_index_is_none() {
    for repeat in MODES {
        for cause in CAUSES {
            assert_eq!(advance(repeat, None, 0, cause), None, "{repeat:?}");
            assert_eq!(advance(repeat, Some(0), 0, cause), None, "{repeat:?}");
            assert_eq!(advance(repeat, Some(3), 3, cause), None, "{repeat:?}");
            assert_eq!(advance(repeat, Some(9), 3, cause), None, "{repeat:?}");
        }
    }
}

#[test]
fn hops_always_name_a_row_in_range() {
    for len in 0..=6 {
        for index in 0..=len {
            for repeat in MODES {
                for cause in CAUSES {
                    let Some(hop) = advance(repeat, Some(index), len, cause) else {
                        continue;
                    };
                    let to = row(hop, index);
                    assert!(
                        to < len,
                        "{repeat:?} {cause:?} from {index} of {len} names row {to}"
                    );
                }
            }
        }
    }
}

#[test]
fn repeat_cycles_off_all_one_off() {
    assert_eq!(Repeat::default(), Repeat::Off);
    assert_eq!(
        [Repeat::Off, Repeat::All, Repeat::One].map(Repeat::next),
        [Repeat::All, Repeat::One, Repeat::Off]
    );
    let mut mode = Repeat::Off;
    let mut seen = Vec::new();
    for _ in 0..3 {
        seen.push(mode);
        mode = mode.next();
    }
    assert_eq!(seen, [Repeat::Off, Repeat::All, Repeat::One]);
    assert_eq!(mode, Repeat::Off, "three clicks come back to the start");
}

#[test]
fn labels_name_the_state_and_are_distinct() {
    let labels: Vec<&str> = MODES.iter().map(|mode| mode.label()).collect();
    assert_eq!(labels, ["Repeat off", "Repeat all", "Repeat one"]);
    for (index, label) in labels.iter().enumerate() {
        assert!(!labels[..index].contains(label), "{label} is named twice");
    }
}

#[test]
fn the_two_on_states_take_the_accent_and_only_one_carries_a_badge() {
    assert!(!Repeat::Off.lit());
    assert!(Repeat::All.lit());
    assert!(Repeat::One.lit());
    assert_eq!(Repeat::Off.badge(), None);
    assert_eq!(Repeat::All.badge(), None);
    assert_eq!(Repeat::One.badge(), Some("1"));
}

#[test]
fn repeat_keys_round_trip() {
    for mode in MODES {
        assert_eq!(
            Repeat::from_key(mode.key()),
            Some(mode),
            "{} does not read back as {mode:?}",
            mode.key()
        );
    }
}

#[test]
fn repeat_from_key_rejects_other_spellings() {
    for spelling in ["Off", "", "all ", "none"] {
        assert_eq!(
            Repeat::from_key(spelling),
            None,
            "{spelling:?} is not one of the keys the file stores"
        );
    }
}
