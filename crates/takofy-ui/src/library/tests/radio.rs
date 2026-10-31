use super::super::*;
use super::test_support::*;
use takofy_model::home::{SeedKind, StationSeed};

const RADIO: &str = "spotify:track:radio";
const OTHER: &str = "spotify:track:other";

fn seed(uri: &str) -> StationSeed {
    StationSeed {
        kind: SeedKind::Song,
        uri: uri.to_owned(),
    }
}

fn rows() -> Vec<TrackEntry> {
    vec![track("t0"), track("t1")]
}

#[test]
fn an_album_seed_then_an_artist_seed_lands_only_the_artists_answer() {
    let mut library = Library::default();
    let album = StationSeed {
        kind: SeedKind::Album,
        uri: "spotify:album:r".to_owned(),
    };
    let artist = StationSeed {
        kind: SeedKind::Artist,
        uri: "spotify:artist:a".to_owned(),
    };

    assert_eq!(
        library.start_radio(album.clone()),
        Command::StartRadio {
            seed: album.clone()
        }
    );
    assert_eq!(
        library.start_radio(artist.clone()),
        Command::StartRadio {
            seed: artist.clone()
        }
    );

    library.apply(&Update::Radio {
        seed: album.uri.clone(),
        rows: rows(),
    });
    assert!(
        !library.has_landed_play(),
        "the replaced album seed still landed"
    );
    assert_eq!(library.take_play(), None);

    library.apply(&Update::Radio {
        seed: artist.uri.clone(),
        rows: vec![track("t2")],
    });
    assert_eq!(
        library.take_play().map(|start| start.tracks),
        Some(vec![track("t2")])
    );
}

#[test]
fn a_landed_radio_waits_as_a_play_with_no_origin() {
    let mut library = Library::default();
    library.start_radio(seed(RADIO));

    library.apply(&Update::Radio {
        seed: RADIO.to_owned(),
        rows: rows(),
    });

    assert_eq!(
        library.take_play(),
        Some(Start {
            origin: None,
            tracks: rows(),
        })
    );
}

#[test]
fn a_press_sends_the_command() {
    let mut library = Library::default();

    assert_eq!(
        library.start_radio(seed(RADIO)),
        Command::StartRadio { seed: seed(RADIO) }
    );
}

#[test]
fn a_repeat_press_sends_again_and_keeps_the_first_answer() {
    let mut library = Library::default();
    let asked = seed(RADIO);

    assert_eq!(
        library.start_radio(asked.clone()),
        Command::StartRadio {
            seed: asked.clone()
        },
        "the first press sends"
    );
    assert_eq!(
        library.start_radio(asked.clone()),
        Command::StartRadio {
            seed: asked.clone()
        },
        "a repeated press is not refused"
    );

    library.apply(&Update::Radio {
        seed: RADIO.to_owned(),
        rows: rows(),
    });
    assert!(library.has_landed_play());
    assert_eq!(
        library.take_play(),
        Some(Start {
            origin: None,
            tracks: rows(),
        })
    );

    library.apply(&Update::Radio {
        seed: RADIO.to_owned(),
        rows: vec![track("late")],
    });
    assert!(
        !library.has_landed_play(),
        "a second answer for the seed landed a second play"
    );
    assert_eq!(library.take_play(), None);
    assert_eq!(library.sidebar_notice, None);
}

#[test]
fn a_different_seed_replaces_the_wanted_one() {
    let mut library = Library::default();
    library.start_radio(seed(RADIO));
    library.start_radio(seed(OTHER));

    library.apply(&Update::Radio {
        seed: RADIO.to_owned(),
        rows: rows(),
    });
    assert!(!library.has_landed_play(), "the replaced seed still landed");
    assert_eq!(library.sidebar_notice, None);

    library.apply(&Update::Radio {
        seed: OTHER.to_owned(),
        rows: vec![track("other")],
    });
    assert_eq!(
        library.take_play().map(|start| start.tracks),
        Some(vec![track("other")])
    );
}

#[test]
fn a_radio_for_a_seed_never_asked_is_ignored() {
    let mut library = Library::default();

    library.apply(&Update::Radio {
        seed: RADIO.to_owned(),
        rows: rows(),
    });

    assert!(!library.has_landed_play());
    assert_eq!(library.take_play(), None);
    assert_eq!(library.sidebar_notice, None);
}

#[test]
fn a_landed_radio_clears_the_radio_notice() {
    let mut library = Library::default();
    library.start_radio(seed(RADIO));
    library.sidebar_notice = Some(SidebarWrite::Radio);

    library.apply(&Update::Radio {
        seed: RADIO.to_owned(),
        rows: rows(),
    });

    assert!(library.has_landed_play());
    assert_eq!(library.sidebar_notice, None);
}

#[test]
fn a_failed_radio_says_so_and_starts_nothing() {
    let mut library = Library::default();
    library.start_radio(seed(RADIO));

    library.apply(&Update::RadioFailed {
        seed: RADIO.to_owned(),
    });

    assert_eq!(library.sidebar_notice, Some(SidebarWrite::Radio));
    assert!(!library.has_landed_play());
    assert_eq!(library.take_play(), None);
    assert_eq!(library.play_notice, None);
}

#[test]
fn an_answered_radio_takes_the_pending_seed() {
    let mut library = Library::default();
    library.start_radio(seed(RADIO));
    library.apply(&Update::RadioFailed {
        seed: RADIO.to_owned(),
    });
    library.sidebar_notice = None;

    library.apply(&Update::Radio {
        seed: RADIO.to_owned(),
        rows: rows(),
    });

    assert!(
        !library.has_landed_play(),
        "the seed answered twice after its failure"
    );
    assert_eq!(library.sidebar_notice, None);
}

#[test]
fn a_failure_for_a_stale_seed_does_nothing() {
    let mut library = Library::default();
    library.start_radio(seed(RADIO));
    library.start_radio(seed(OTHER));

    library.apply(&Update::RadioFailed {
        seed: RADIO.to_owned(),
    });

    assert_eq!(library.sidebar_notice, None);
    library.apply(&Update::Radio {
        seed: OTHER.to_owned(),
        rows: rows(),
    });
    assert!(
        library.has_landed_play(),
        "the stale failure dropped the live seed"
    );
}

#[test]
fn a_radio_with_no_rows_is_a_failure() {
    let mut library = Library::default();
    library.start_radio(seed(RADIO));

    library.apply(&Update::Radio {
        seed: RADIO.to_owned(),
        rows: Vec::new(),
    });

    assert!(!library.has_landed_play(), "an empty radio started a play");
    assert_eq!(library.sidebar_notice, Some(SidebarWrite::Radio));
}

#[test]
fn a_later_press_clears_the_notice() {
    let mut library = Library::default();
    library.start_radio(seed(RADIO));
    library.apply(&Update::RadioFailed {
        seed: RADIO.to_owned(),
    });
    assert_eq!(library.sidebar_notice, Some(SidebarWrite::Radio));

    library.start_radio(seed(RADIO));

    assert_eq!(library.sidebar_notice, None);
}

#[test]
fn navigating_does_not_drop_a_pending_radio() {
    let mut library = Library::default();
    library.start_radio(seed(RADIO));

    let _ = library.open(Page::Home);
    library.select("a");

    library.apply(&Update::Radio {
        seed: RADIO.to_owned(),
        rows: rows(),
    });

    assert!(
        library.has_landed_play(),
        "the answer was dropped because the page changed"
    );
}
