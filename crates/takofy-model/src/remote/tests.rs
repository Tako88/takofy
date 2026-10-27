use super::{Landing, Options};

fn landing(
    shuffle: Option<bool>,
    repeat_context: Option<bool>,
    repeat_track: Option<bool>,
) -> Landing {
    Landing {
        rows: vec!["spotify:track:a".to_owned()],
        index: 0,
        position_ms: 0,
        playing: false,
        context: None,
        shuffle,
        repeat_context,
        repeat_track,
        epoch: 1,
    }
}

#[test]
fn a_landing_that_names_every_mode_asserts_them() {
    let report = landing(Some(true), Some(false), Some(true)).report();
    assert_eq!(
        report.options,
        Some(Options {
            shuffle: true,
            repeat_context: false,
            repeat_track: true,
        })
    );
}

#[test]
fn a_landing_that_names_only_some_modes_asserts_none() {
    for (shuffle, repeat_context, repeat_track) in [
        (Some(true), None, Some(true)),
        (Some(true), Some(true), None),
        (None, Some(true), Some(true)),
        (None, None, None),
    ] {
        let report = landing(shuffle, repeat_context, repeat_track).report();
        assert_eq!(
            report.options, None,
            "must assert none for {shuffle:?}/{repeat_context:?}/{repeat_track:?}"
        );
    }
}
