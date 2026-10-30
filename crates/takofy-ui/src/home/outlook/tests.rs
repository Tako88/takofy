use super::*;

const UNREADABLE: Failure = Failure::Load;
const ALBUMS: Failure = Failure::SavedAlbums;
const LIKES: Failure = Failure::Other;

fn arrived() -> Intake {
    Intake {
        playlists: Phase::Arrived,
        albums: Phase::Arrived,
        likes: Phase::Arrived,
    }
}

fn tally(any: bool, unanswered: usize, usable: usize) -> Tally {
    Tally {
        any,
        unanswered,
        usable,
        label_failed: None,
    }
}

fn label_failed(any: bool, failure: Failure) -> Tally {
    Tally {
        any,
        unanswered: 0,
        usable: 0,
        label_failed: Some(failure),
    }
}

#[test]
fn a_failed_rootlist_is_the_whole_page() {
    let intake = Intake {
        playlists: Phase::Failed(UNREADABLE),
        albums: Phase::Pending,
        likes: Phase::Pending,
    };
    assert_eq!(
        outlook(intake, tally(false, 0, 0)),
        Outlook::Unreadable(UNREADABLE)
    );
    assert_eq!(
        outlook(intake, tally(true, 2, 4)),
        Outlook::Unreadable(UNREADABLE),
        "cards and pending sources cannot outrank it"
    );
}

#[test]
fn a_pending_source_holds_the_shelf_whole() {
    for field in 0..3 {
        let mut intake = arrived();
        match field {
            0 => intake.playlists = Phase::Pending,
            1 => intake.albums = Phase::Pending,
            _ => intake.likes = Phase::Pending,
        }
        assert_eq!(
            outlook(intake, tally(true, 0, 3)),
            Outlook::Waiting,
            "field {field}"
        );
    }
}

#[test]
fn an_unanswered_label_holds_the_shelf_whole() {
    assert_eq!(
        outlook(arrived(), tally(true, 1, 3)),
        Outlook::Waiting,
        "one late label would reshuffle a partial shelf"
    );
    assert_eq!(outlook(arrived(), tally(true, 1, 0)), Outlook::Waiting);
}

#[test]
fn a_usable_pool_draws_even_with_a_failed_source() {
    assert_eq!(
        outlook(
            Intake {
                albums: Phase::Failed(ALBUMS),
                ..arrived()
            },
            tally(true, 0, 2)
        ),
        Outlook::Cards,
        "a failed albums read costs only its own kind"
    );
    assert_eq!(
        outlook(
            Intake {
                likes: Phase::Failed(LIKES),
                ..arrived()
            },
            tally(true, 0, 1)
        ),
        Outlook::Cards
    );
}

#[test]
fn an_empty_pool_names_the_failure_that_emptied_it() {
    assert_eq!(
        outlook(
            Intake {
                albums: Phase::Failed(ALBUMS),
                ..arrived()
            },
            tally(false, 0, 0)
        ),
        Outlook::Incomplete(ALBUMS)
    );
    assert_eq!(
        outlook(
            Intake {
                likes: Phase::Failed(LIKES),
                ..arrived()
            },
            tally(true, 0, 0)
        ),
        Outlook::Incomplete(LIKES)
    );
    assert_eq!(
        outlook(arrived(), label_failed(true, LIKES)),
        Outlook::Incomplete(LIKES),
        "a failed label read is not a quiet library"
    );
    assert_eq!(
        outlook(arrived(), label_failed(false, LIKES)),
        Outlook::Incomplete(LIKES),
        "nor the day-one page"
    );
}

#[test]
fn a_usable_pool_outranks_a_failed_label_read() {
    assert_eq!(
        outlook(
            arrived(),
            Tally {
                label_failed: Some(LIKES),
                ..tally(true, 0, 1)
            }
        ),
        Outlook::Cards
    );
}

#[test]
fn albums_speak_before_likes() {
    assert_eq!(
        outlook(
            Intake {
                albums: Phase::Failed(ALBUMS),
                likes: Phase::Failed(LIKES),
                ..arrived()
            },
            tally(true, 0, 0)
        ),
        Outlook::Incomplete(ALBUMS)
    );
}

#[test]
fn nothing_saved_is_the_day_one_page() {
    assert_eq!(outlook(arrived(), tally(false, 0, 0)), Outlook::DayOne);
}

#[test]
fn saved_but_undated_is_quiet() {
    assert_eq!(
        outlook(arrived(), tally(true, 0, 0)),
        Outlook::Quiet,
        "the day-one copy would be false for this library"
    );
}

#[test]
fn settled_needs_every_source_answered() {
    assert!(arrived().settled());
    for field in 0..3 {
        let mut intake = arrived();
        match field {
            0 => intake.playlists = Phase::Pending,
            1 => intake.albums = Phase::Pending,
            _ => intake.likes = Phase::Pending,
        }
        assert!(!intake.settled(), "a pending source: field {field}");
    }
}

#[test]
fn a_failed_rootlist_never_settles() {
    for albums in phases() {
        for likes in phases() {
            let intake = Intake {
                playlists: Phase::Failed(UNREADABLE),
                albums,
                likes,
            };
            assert!(!intake.settled(), "{intake:?}");
        }
    }
}

#[test]
fn a_failed_source_beside_the_rootlist_still_settles() {
    for phase in [Phase::Arrived, Phase::Failed(ALBUMS)] {
        let intake = Intake {
            playlists: Phase::Arrived,
            albums: phase,
            likes: phase,
        };
        assert!(intake.settled(), "{intake:?}");
    }
}

/// Every phase a source can be in, for the loop over all combinations.
fn phases() -> [Phase; 3] {
    [Phase::Pending, Phase::Arrived, Phase::Failed(ALBUMS)]
}

#[test]
fn all_twenty_seven_intakes_agree_with_the_rules() {
    // Every tally shape the loop meets, so no rule is only tested on an empty
    // tally.
    let tallies = [
        tally(false, 0, 0),
        tally(true, 0, 0),
        tally(true, 0, 2),
        tally(true, 1, 2),
        label_failed(true, LIKES),
    ];
    for playlists in phases() {
        for albums in phases() {
            for likes in phases() {
                let intake = Intake {
                    playlists,
                    albums,
                    likes,
                };
                for tally in tallies {
                    let outcome = outlook(intake, tally);
                    let unreadable = matches!(playlists, Phase::Failed(_));
                    assert_eq!(
                        matches!(outcome, Outlook::Unreadable(_)),
                        unreadable,
                        "{intake:?} {tally:?}"
                    );
                    if unreadable {
                        continue;
                    }
                    let waiting = matches!(outcome, Outlook::Waiting);
                    if !intake.settled() {
                        assert!(waiting, "an unsettled source waits: {intake:?}");
                        continue;
                    }
                    if tally.unanswered > 0 {
                        assert!(waiting, "a late label waits: {intake:?}");
                        continue;
                    }
                    assert!(!waiting, "{intake:?} {tally:?} has nothing to wait for");
                    if tally.label_failed.is_some() && tally.usable == 0 {
                        // A source failure speaks first; the label failure is
                        // the only one left to name an empty pool.
                        let expected = match (albums, likes) {
                            (Phase::Failed(failure), _) => failure,
                            (_, Phase::Failed(failure)) => failure,
                            _ => LIKES,
                        };
                        assert_eq!(
                            outcome,
                            Outlook::Incomplete(expected),
                            "a failed label read says so instead of going quiet"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn all_arrived_combinations_follow_the_pool() {
    for albums in [Phase::Arrived, Phase::Failed(ALBUMS)] {
        for likes in [Phase::Arrived, Phase::Failed(LIKES)] {
            let intake = Intake {
                playlists: Phase::Arrived,
                albums,
                likes,
            };
            assert_eq!(
                outlook(intake, tally(false, 0, 0)),
                match (albums, likes) {
                    (Phase::Failed(failure), _) => Outlook::Incomplete(failure),
                    (_, Phase::Failed(failure)) => Outlook::Incomplete(failure),
                    _ => Outlook::DayOne,
                }
            );
            assert_eq!(
                outlook(intake, tally(true, 0, 0)),
                match (albums, likes) {
                    (Phase::Failed(failure), _) => Outlook::Incomplete(failure),
                    (_, Phase::Failed(failure)) => Outlook::Incomplete(failure),
                    _ => Outlook::Quiet,
                }
            );
            assert_eq!(outlook(intake, tally(true, 0, 1)), Outlook::Cards);
        }
    }
}
